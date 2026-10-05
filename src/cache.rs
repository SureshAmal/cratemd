use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use anyhow::{Context, Result};
use crate::model::CrateIndex;

pub struct CacheManager {
    cache_dir: Option<PathBuf>,
    enabled: bool,
    force_refresh: bool,
}

impl CacheManager {
    pub fn new(enabled: bool, force_refresh: bool) -> Self {
        let cache_dir = if enabled {
            get_cache_dir()
        } else {
            None
        };

        if let Some(ref dir) = cache_dir {
            let _ = fs::create_dir_all(dir);
        }

        Self {
            cache_dir,
            enabled,
            force_refresh,
        }
    }

    pub fn get_cache_path(&self, crate_name: &str, version: &str) -> Option<PathBuf> {
        self.cache_dir.as_ref().map(|dir| {
            dir.join(format!("{}-{}.json", crate_name, version))
        })
    }

    pub fn load(&self, crate_name: &str, version: &str, crate_root: &Path) -> Option<CrateIndex> {
        if !self.enabled || self.force_refresh {
            return None;
        }

        let cache_path = self.get_cache_path(crate_name, version)?;
        if !cache_path.exists() {
            return None;
        }

        // For local crates, verify cache isn't stale
        if is_local_crate(crate_root) {
            if let Ok(cache_meta) = cache_path.metadata() {
                if let Ok(cache_mtime) = cache_meta.modified() {
                    if is_source_newer(crate_root, cache_mtime) {
                        return None; // Cache is stale
                    }
                }
            }
        }

        let file = File::open(&cache_path).ok()?;
        let reader = BufReader::new(file);
        serde_json::from_reader(reader).ok()
    }

    pub fn store(&self, index: &CrateIndex) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        let cache_path = match self.get_cache_path(&index.info.name, &index.info.version) {
            Some(p) => p,
            None => return Ok(()),
        };

        if let Some(parent) = cache_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let file = File::create(&cache_path)
            .with_context(|| format!("Failed to create cache file {}", cache_path.display()))?;
        let writer = BufWriter::new(file);
        serde_json::to_writer(writer, index)
            .with_context(|| format!("Failed to write cache file {}", cache_path.display()))?;

        Ok(())
    }
}

fn get_cache_dir() -> Option<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        Some(PathBuf::from(xdg).join("cratemd"))
    } else if let Ok(home) = std::env::var("HOME") {
        Some(PathBuf::from(home).join(".cache").join("cratemd"))
    } else {
        std::env::temp_dir().join("cratemd").into()
    }
}

fn is_local_crate(path: &Path) -> bool {
    // Registry crates are in ~/.cargo/registry/src/ and are immutable
    let path_str = path.to_string_lossy();
    !path_str.contains(".cargo/registry")
}

fn is_source_newer(root: &Path, cache_time: SystemTime) -> bool {
    let check_files = ["Cargo.toml", "Cargo.lock", "src/lib.rs", "src/main.rs"];
    for rel in check_files {
        let f = root.join(rel);
        if let Ok(meta) = f.metadata() {
            if let Ok(mtime) = meta.modified() {
                if mtime > cache_time {
                    return true;
                }
            }
        }
    }
    false
}
