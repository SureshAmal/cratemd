use std::collections::BTreeSet;
use std::fmt::Write;
use std::path::Path;
use std::time::Instant;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use crate::analyzer::CrateAnalyzer;
use crate::cache::CacheManager;
use crate::locator::CrateLocator;
use crate::workspace::{parse_lockfile, WorkspaceInfo};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WarmedCrate {
    pub name: String,
    pub version: String,
    pub symbols_count: usize,
    pub was_cached: bool,
    pub duration_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WarmReport {
    pub target_scope: String,
    pub total_crates: usize,
    pub newly_warmed: usize,
    pub already_cached: usize,
    pub failed_count: usize,
    pub total_duration_ms: u128,
    pub items: Vec<WarmedCrate>,
    pub failed: Vec<String>,
}

impl WarmReport {
    pub fn render_ascii(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "Cache Pre-warm Report: {}", self.target_scope);
        let _ = writeln!(out, "{}", "=".repeat(60));
        let _ = writeln!(
            out,
            "Summary: {} crates processed ({} newly warmed, {} cached, {} failed) in {} ms",
            self.total_crates, self.newly_warmed, self.already_cached, self.failed_count, self.total_duration_ms
        );
        let _ = writeln!(out);

        if !self.items.is_empty() {
            let _ = writeln!(out, "{:<24} {:<12} {:<10} {:<12} {:<10}", "Crate", "Version", "Symbols", "Status", "Time");
            let _ = writeln!(out, "{}", "-".repeat(60));
            for item in &self.items {
                let status = if item.was_cached { "cached" } else { "warmed" };
                let time_str = format!("{} ms", item.duration_ms);
                let _ = writeln!(
                    out,
                    "{:<24} {:<12} {:<10} {:<12} {:<10}",
                    item.name, item.version, item.symbols_count, status, time_str
                );
            }
        }

        if !self.failed.is_empty() {
            let _ = writeln!(out);
            let _ = writeln!(out, "Failed to resolve/warm:");
            for f in &self.failed {
                let _ = writeln!(out, "  - {}", f);
            }
        }

        out
    }
}

pub struct CacheWarmer;

impl CacheWarmer {
    pub fn warm(target: Option<&str>, warm_all: bool, refresh: bool) -> Result<WarmReport> {
        let start_time = Instant::now();
        let cache = CacheManager::new(true, refresh);

        // Check if target is a specific crate or if we should inspect local project/workspace
        let mut crates_to_warm: Vec<String> = Vec::new();
        let mut scope_title = String::new();

        if let Some(target_str) = target {
            let path_cand = Path::new(target_str);
            if path_cand.is_dir() && path_cand.join("Cargo.toml").exists() {
                // It's a local crate/workspace directory
                collect_from_directory(path_cand, warm_all, &mut crates_to_warm, &mut scope_title)?;
            } else {
                // Single crate name or spec
                crates_to_warm.push(target_str.to_string());
                scope_title = format!("Crate '{}'", target_str);
            }
        } else {
            // Default: inspect current directory
            let cwd = std::env::current_dir()?;
            collect_from_directory(&cwd, warm_all, &mut crates_to_warm, &mut scope_title)?;
        }

        let mut items = Vec::new();
        let mut failed = Vec::new();

        for crate_spec in crates_to_warm {
            let item_start = Instant::now();
            let locate_res = CrateLocator::locate(&crate_spec);
            let info = match locate_res {
                Ok(info) => info,
                Err(err) => {
                    failed.push(format!("{}: {}", crate_spec, err));
                    continue;
                }
            };

            // Check if already in cache and not refreshed
            if !refresh {
                if let Some(cached_idx) = cache.load(&info.name, &info.version, &info.root_dir) {
                    items.push(WarmedCrate {
                        name: info.name,
                        version: info.version,
                        symbols_count: cached_idx.symbols.len(),
                        was_cached: true,
                        duration_ms: item_start.elapsed().as_millis(),
                    });
                    continue;
                }
            }

            // Analyze and store
            match CrateAnalyzer::new(info.clone()).analyze() {
                Ok(index) => {
                    let symbols_count = index.symbols.len();
                    let _ = cache.store(&index);
                    items.push(WarmedCrate {
                        name: info.name,
                        version: info.version,
                        symbols_count,
                        was_cached: false,
                        duration_ms: item_start.elapsed().as_millis(),
                    });
                }
                Err(err) => {
                    failed.push(format!("{}: {}", info.name, err));
                }
            }
        }

        let total_crates = items.len() + failed.len();
        let newly_warmed = items.iter().filter(|i| !i.was_cached).count();
        let already_cached = items.iter().filter(|i| i.was_cached).count();
        let failed_count = failed.len();
        let total_duration_ms = start_time.elapsed().as_millis();

        Ok(WarmReport {
            target_scope: scope_title,
            total_crates,
            newly_warmed,
            already_cached,
            failed_count,
            total_duration_ms,
            items,
            failed,
        })
    }
}

fn collect_from_directory(
    dir: &Path,
    warm_all: bool,
    crates: &mut Vec<String>,
    scope_title: &mut String,
) -> Result<()> {
    let ws_root = WorkspaceInfo::find_root(dir);
    let root = ws_root.unwrap_or_else(|| dir.to_path_buf());

    let manifest_path = root.join("Cargo.toml");
    if !manifest_path.exists() {
        bail!("No Cargo.toml found in {}", root.display());
    }

    let is_workspace = WorkspaceInfo::find_root(&root).is_some();
    if is_workspace {
        let ws = WorkspaceInfo::load(&root)?;
        *scope_title = format!("Workspace at {}", root.display());

        // Add workspace members
        for m in &ws.members {
            crates.push(m.abs_path.to_string_lossy().to_string());
        }

        if warm_all {
            let lock_path = root.join("Cargo.lock");
            if lock_path.exists() {
                let lock_map = parse_lockfile(&lock_path);
                let member_names: BTreeSet<&str> = ws.members.iter().map(|m| m.name.as_str()).collect();
                for (dep, ver) in lock_map {
                    if !member_names.contains(dep.as_str()) {
                        crates.push(format!("{}@{}", dep, ver));
                    }
                }
            }
        } else {
            // Direct external dependencies of workspace members
            let mut direct_deps = BTreeSet::new();
            for m in &ws.members {
                for ext in &m.external_deps {
                    direct_deps.insert(ext.clone());
                }
            }
            let lock_path = root.join("Cargo.lock");
            let lock_map = if lock_path.exists() {
                parse_lockfile(&lock_path)
            } else {
                std::collections::HashMap::new()
            };

            for dep in direct_deps {
                if let Some(ver) = lock_map.get(&dep) {
                    crates.push(format!("{}@{}", dep, ver));
                } else {
                    crates.push(dep);
                }
            }
        }
    } else {
        // Single package
        *scope_title = format!("Project at {}", root.display());
        crates.push(root.to_string_lossy().to_string());

        let lock_path = root.join("Cargo.lock");
        if lock_path.exists() {
            let lock_map = parse_lockfile(&lock_path);
            let content = std::fs::read_to_string(&manifest_path)?;
            let manifest_val: toml::Value = toml::from_str(&content)?;

            let mut targets = BTreeSet::new();
            if warm_all {
                for (dep, ver) in lock_map {
                    targets.insert(format!("{}@{}", dep, ver));
                }
            } else {
                let mut direct_names = Vec::new();
                if let Some(deps) = manifest_val.get("dependencies").and_then(|d| d.as_table()) {
                    direct_names.extend(deps.keys().cloned());
                }
                if let Some(deps) = manifest_val.get("dev-dependencies").and_then(|d| d.as_table()) {
                    direct_names.extend(deps.keys().cloned());
                }
                for dep in direct_names {
                    if let Some(ver) = lock_map.get(&dep) {
                        targets.insert(format!("{}@{}", dep, ver));
                    } else {
                        targets.insert(dep);
                    }
                }
            }
            crates.extend(targets);
        }
    }

    Ok(())
}
