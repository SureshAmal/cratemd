use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use anyhow::{Context, Result};
use walkdir::WalkDir;
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

    pub fn get_cache_path(&self, crate_name: &str, version: &str, crate_root: &Path) -> Option<PathBuf> {
        let root = crate_root.canonicalize().unwrap_or_else(|_| crate_root.to_path_buf());
        // A stable path digest keeps separate local crates with the same name/version apart.
        let digest = root.as_os_str().as_encoded_bytes().iter().fold(0xcbf29ce484222325u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        });
        self.cache_dir.as_ref().map(|dir| {
            dir.join(format!("v2-{}-{}-{digest:016x}.json", crate_name, version))
        })
    }

    pub fn load(&self, crate_name: &str, version: &str, crate_root: &Path) -> Option<CrateIndex> {
        if !self.enabled || self.force_refresh {
            return None;
        }

        let cache_path = self.get_cache_path(crate_name, version, crate_root)?;
        if !cache_path.exists() {
            return None;
        }

        // For local crates, verify cache isn't stale
        if is_local_crate(crate_root)
            && let Ok(cache_meta) = cache_path.metadata()
                && let Ok(cache_mtime) = cache_meta.modified()
                    && is_source_newer(crate_root, cache_mtime) {
                        return None; // Cache is stale
                    }

        let file = File::open(&cache_path).ok()?;
        let reader = BufReader::new(file);
        let index: CrateIndex = serde_json::from_reader(reader).ok()?;
        let indexed_root = index.info.root_dir.canonicalize().ok()?;
        let requested_root = crate_root.canonicalize().ok()?;
        (index.info.name == crate_name && index.info.version == version && indexed_root == requested_root)
            .then_some(index)
    }

    pub fn store(&self, index: &CrateIndex) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        let cache_path = match self.get_cache_path(&index.info.name, &index.info.version, &index.info.root_dir) {
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
    if let Ok(mtime) = root.metadata().and_then(|meta| meta.modified())
        && mtime > cache_time {
            return true;
        }
    let check_files = ["Cargo.toml", "Cargo.lock"];
    for rel in check_files {
        let f = root.join(rel);
        if let Ok(meta) = f.metadata()
            && let Ok(mtime) = meta.modified()
                && mtime > cache_time {
                    return true;
                }
    }
    for dir_name in ["src", "examples"] {
        let dir = root.join(dir_name);
        if !dir.exists() {
            continue;
        }
        for entry in WalkDir::new(&dir) {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => return true,
            };
            let path = entry.path();
            if (entry.file_type().is_dir() || path.extension().is_some_and(|ext| ext == "rs"))
                && match path.metadata().and_then(|meta| meta.modified()) {
                    Ok(mtime) => mtime > cache_time,
                    Err(_) => true,
                }
            {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_paths_distinguish_crate_roots() {
        let cache = CacheManager::new(true, false);
        let first = cache.get_cache_path("demo", "0.1.0", Path::new("/tmp/project-a/demo"));
        let second = cache.get_cache_path("demo", "0.1.0", Path::new("/tmp/project-b/demo"));
        assert_ne!(first, second);
    }

    #[test]
    fn nested_source_changes_invalidate_cache() {
        let root = std::env::temp_dir().join(format!("cratemd-cache-test-{}-{:?}", std::process::id(), std::thread::current().id()));
        let nested = root.join("src/nested");
        fs::create_dir_all(&nested).unwrap();
        let old_time = SystemTime::UNIX_EPOCH;
        fs::write(nested.join("module.rs"), "pub fn changed() {}").unwrap();
        assert!(is_source_newer(&root, old_time));
        fs::remove_dir_all(root).unwrap();
    }
}
