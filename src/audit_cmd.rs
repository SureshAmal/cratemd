use anyhow::{bail, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use crate::locator::CrateLocator;
use crate::workspace::WorkspaceInfo;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditReport {
    pub project_name: String,
    pub root_dir: PathBuf,
    pub total_dependencies: usize,
    pub unique_crates: usize,
    pub duplicate_splits: usize,
    pub offline_ready_count: usize,
    pub offline_missing_count: usize,
    pub offline_ready_pct: f64,
    pub duplicates: Vec<DuplicateSplit>,
    pub missing_offline: Vec<MissingDependency>,
}

impl AuditReport {
    pub fn render_ascii(&self) -> String {
        DependencyAuditor::render_markdown(self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateSplit {
    pub crate_name: String,
    pub versions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissingDependency {
    pub crate_name: String,
    pub version: String,
}

pub struct DependencyAuditor;

impl DependencyAuditor {
    pub fn audit(target_dir: Option<&Path>) -> Result<AuditReport> {
        let start_dir = match target_dir {
            Some(p) => p.to_path_buf(),
            None => std::env::current_dir()?,
        };

        let ws_root = WorkspaceInfo::find_root(&start_dir).unwrap_or_else(|| start_dir.clone());
        let lock_path = ws_root.join("Cargo.lock");
        if !lock_path.exists() {
            bail!("No Cargo.lock found in {}", ws_root.display());
        }

        let lock_content = std::fs::read_to_string(&lock_path)?;
        let lock_val: toml::Value = toml::from_str(&lock_content)?;

        let mut crate_versions: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut all_pairs: Vec<(String, String)> = Vec::new();

        if let Some(packages) = lock_val.get("package").and_then(|p| p.as_array()) {
            for pkg in packages {
                if let (Some(name), Some(ver)) = (
                    pkg.get("name").and_then(|n| n.as_str()),
                    pkg.get("version").and_then(|v| v.as_str()),
                ) {
                    crate_versions.entry(name.to_string()).or_default().insert(ver.to_string());
                    all_pairs.push((name.to_string(), ver.to_string()));
                }
            }
        }

        let total_dependencies = all_pairs.len();
        let unique_crates = crate_versions.len();

        let mut duplicates = Vec::new();
        for (name, versions) in &crate_versions {
            if versions.len() > 1 {
                duplicates.push(DuplicateSplit {
                    crate_name: name.clone(),
                    versions: versions.iter().cloned().collect(),
                });
            }
        }

        let mut missing_offline = Vec::new();
        let mut offline_ready_count = 0;

        for (name, ver) in &all_pairs {
            // Check if crate is a local workspace member or cached in registry
            let spec = format!("{}@{}", name, ver);
            if CrateLocator::locate(&spec).is_ok() {
                offline_ready_count += 1;
            } else {
                // If not found with exact version, try name alone
                if CrateLocator::locate(name).is_ok() {
                    offline_ready_count += 1;
                } else {
                    missing_offline.push(MissingDependency {
                        crate_name: name.clone(),
                        version: ver.clone(),
                    });
                }
            }
        }

        let offline_missing_count = missing_offline.len();
        let offline_ready_pct = if total_dependencies > 0 {
            (offline_ready_count as f64 / total_dependencies as f64) * 100.0
        } else {
            100.0
        };

        let project_name = ws_root.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("project")
            .to_string();

        Ok(AuditReport {
            project_name,
            root_dir: ws_root,
            total_dependencies,
            unique_crates,
            duplicate_splits: duplicates.len(),
            offline_ready_count,
            offline_missing_count,
            offline_ready_pct,
            duplicates,
            missing_offline,
        })
    }

    pub fn render_markdown(report: &AuditReport) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "# Offline Dependency Audit: {} ({})\n", report.project_name, report.root_dir.display());

        let _ = writeln!(out, "- **Total Dependencies:** {} crates ({} unique)", report.total_dependencies, report.unique_crates);
        let _ = writeln!(
            out,
            "- **Offline Readiness:** {:.1}% ({}/{} ready)",
            report.offline_ready_pct, report.offline_ready_count, report.total_dependencies
        );
        let _ = writeln!(out, "- **Version Splits:** {} duplicates detected\n", report.duplicate_splits);

        if !report.duplicates.is_empty() {
            let _ = writeln!(out, "### Duplicate Dependency Splits Detected ({} crates)\n", report.duplicates.len());
            for d in &report.duplicates {
                let _ = writeln!(out, "- `{}`: versions {}", d.crate_name, d.versions.join(", "));
            }
            let _ = writeln!(out);
        } else {
            let _ = writeln!(out, "No duplicate crate version splits detected.\n");
        }

        if !report.missing_offline.is_empty() {
            let _ = writeln!(out, "### Missing Offline Dependencies ({} crates)\n", report.missing_offline.len());
            for m in &report.missing_offline {
                let _ = writeln!(out, "- `{}` v{}", m.crate_name, m.version);
            }
            let _ = writeln!(out, "\n*(Run `cargo fetch` while online to populate offline Cargo cache)*\n");
        }

        out
    }
}
