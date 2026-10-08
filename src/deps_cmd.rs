use std::collections::HashMap;
use std::fmt::Write;
use std::path::PathBuf;
use anyhow::Result;
use serde::Serialize;

use crate::locator::CrateLocator;
use crate::workspace::{parse_lockfile, parse_locked_package_deps, WorkspaceInfo};

#[derive(Debug, Clone, Serialize)]
pub struct DepItem {
    pub name: String,
    pub resolved_version: Option<String>,
    pub used_by: Vec<String>,
    pub description: Option<String>,
    pub cached_offline: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DepsReport {
    pub title: String,
    pub is_workspace: bool,
    pub total_deps: usize,
    pub items: Vec<DepItem>,
}

pub struct DepsInspector;

impl DepsInspector {
    pub fn inspect(target: Option<&str>) -> Result<DepsReport> {
        let path = target.map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
        let abs_path = if path.is_relative() {
            std::env::current_dir()?.join(&path)
        } else {
            path
        };

        let ws_root = WorkspaceInfo::find_root(&abs_path);

        if let Some(ref root) = ws_root {
            // Workspace inspection
            let ws = WorkspaceInfo::load(root)?;
            let lock_map = parse_lockfile(&root.join("Cargo.lock"));

            let mut dep_users: HashMap<(String, String), Vec<String>> = HashMap::new();
            for m in &ws.members {
                let member_versions = parse_locked_package_deps(&root.join("Cargo.lock"), &m.name, &m.version);
                for ext in &m.external_deps {
                    let versions = member_versions.get(ext).or_else(|| lock_map.get(ext));
                    if let Some(versions) = versions {
                        for version in versions {
                            dep_users.entry((ext.clone(), version.clone())).or_default().push(m.name.clone());
                        }
                    } else {
                        dep_users.entry((ext.clone(), String::new())).or_default().push(m.name.clone());
                    }
                }
            }

            let mut items = Vec::new();
            let mut dep_keys: Vec<_> = dep_users.keys().cloned().collect();
            dep_keys.sort();

            for (dep_name, version) in dep_keys {
                let used_by = dep_users.get(&(dep_name.clone(), version.clone())).cloned().unwrap_or_default();
                let resolved_ver = (!version.is_empty()).then_some(version);
                let spec = resolved_ver.as_ref().map_or_else(|| dep_name.clone(), |v| format!("{}@{}", dep_name, v));
                let (cached_offline, description) = if let Ok(info) = CrateLocator::locate(&spec) {
                    (true, info.description)
                } else {
                    (false, None)
                };
                items.push(DepItem {
                    name: dep_name,
                    resolved_version: resolved_ver,
                    used_by,
                    description,
                    cached_offline,
                });
            }

            let total_deps = items.len();
            Ok(DepsReport {
                title: format!("Workspace `{}` ({} members)", root.display(), ws.members.len()),
                is_workspace: true,
                total_deps,
                items,
            })
        } else {
            // Single crate inspection
            let info = CrateLocator::locate(&abs_path.to_string_lossy())?;
            let lock_map = parse_lockfile(&info.root_dir.join("Cargo.lock"));
            let package_versions = parse_locked_package_deps(&info.root_dir.join("Cargo.lock"), &info.name, &info.version);

            let mut items = Vec::new();
            let mut deps = info.dependencies.clone();
            deps.sort();

            for dep_name in deps {
                let versions = package_versions.get(&dep_name).or_else(|| lock_map.get(&dep_name)).cloned().unwrap_or_else(|| vec![String::new()]);
                for version in versions {
                    let resolved_ver = (!version.is_empty()).then_some(version);
                    let spec = resolved_ver.as_ref().map_or_else(|| dep_name.clone(), |v| format!("{}@{}", dep_name, v));
                    let (cached_offline, description) = if let Ok(dep_info) = CrateLocator::locate(&spec) {
                        (true, dep_info.description)
                    } else {
                        (false, None)
                    };
                    items.push(DepItem {
                        name: dep_name.clone(),
                        resolved_version: resolved_ver,
                        used_by: vec![info.name.clone()],
                        description,
                        cached_offline,
                    });
                }
            }

            let total_deps = items.len();
            Ok(DepsReport {
                title: format!("{} v{}", info.name, info.version),
                is_workspace: false,
                total_deps,
                items,
            })
        }
    }

    pub fn render_markdown(report: &DepsReport) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "# Dependencies: {} ({} dependencies)\n", report.title, report.total_deps);

        if report.items.is_empty() {
            let _ = writeln!(out, "No external dependencies found.");
            return out;
        }

        let _ = writeln!(out, "| Dependency | Version | Ready Offline | Used By | Description |");
        let _ = writeln!(out, "|---|---|:---:|---|---|");

        for item in &report.items {
            let ver_str = item.resolved_version.as_deref().unwrap_or("latest");
            let cached_str = if item.cached_offline { "ready" } else { "missing" };
            let used_by_str = if report.is_workspace {
                item.used_by.join(", ")
            } else {
                "-".to_string()
            };
            let desc = item.description.as_deref().unwrap_or("");
            let clean_desc: String = desc
                .replace('|', "\\|")
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .chars()
                .filter(|c| c.is_ascii())
                .collect();

            let _ = writeln!(out, "| `{}` | `{}` | {} | {} | {} |",
                item.name, ver_str, cached_str, used_by_str, clean_desc);
        }

        let _ = writeln!(out, "\n*Tips:*");
        let _ = writeln!(out, "- Search API of any dependency: `cratemd search <dep> <query>`");
        let _ = writeln!(out, "- Find functions across all dependencies: `cratemd find <query>`");
        let _ = writeln!(out, "- View cheat sheet: `cratemd cheat <dep>`\n");

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_each_locked_dependency_version() {
        let root = std::env::temp_dir().join(format!("cratemd-deps-test-{}-{:?}", std::process::id(), std::thread::current().id()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n\n[dependencies]\nserde = \"*\"\n").unwrap();
        std::fs::write(root.join("Cargo.lock"), "[[package]]\nname = \"serde\"\nversion = \"1.0.0\"\n\n[[package]]\nname = \"serde\"\nversion = \"2.0.0\"\n").unwrap();
        let report = DepsInspector::inspect(Some(root.to_str().unwrap())).unwrap();
        let versions: Vec<_> = report.items.iter().filter_map(|item| item.resolved_version.as_deref()).collect();
        assert_eq!(versions, vec!["1.0.0", "2.0.0"]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn attributes_split_versions_to_the_right_workspace_members() {
        let root = std::env::temp_dir().join(format!("cratemd-split-test-{}-{:?}", std::process::id(), std::thread::current().id()));
        std::fs::create_dir_all(root.join("member/src")).unwrap();
        std::fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = [\".\", \"member\"]\n\n[package]\nname = \"root-crate\"\nversion = \"0.1.0\"\n\n[dependencies]\nsyn = \"3\"\n").unwrap();
        std::fs::write(root.join("member/Cargo.toml"), "[package]\nname = \"member-crate\"\nversion = \"0.1.0\"\n\n[dependencies]\nsyn = \"2\"\n").unwrap();
        std::fs::write(root.join("Cargo.lock"), "[[package]]\nname = \"root-crate\"\nversion = \"0.1.0\"\ndependencies = [\"syn 3.0.6\"]\n\n[[package]]\nname = \"member-crate\"\nversion = \"0.1.0\"\ndependencies = [\"syn 2.0.119\"]\n\n[[package]]\nname = \"syn\"\nversion = \"2.0.119\"\n\n[[package]]\nname = \"syn\"\nversion = \"3.0.6\"\n").unwrap();
        let report = DepsInspector::inspect(Some(root.to_str().unwrap())).unwrap();
        let older = report.items.iter().find(|item| item.resolved_version.as_deref() == Some("2.0.119")).unwrap();
        let newer = report.items.iter().find(|item| item.resolved_version.as_deref() == Some("3.0.6")).unwrap();
        assert_eq!(older.used_by, vec!["member-crate"]);
        assert_eq!(newer.used_by, vec!["root-crate"]);
        std::fs::remove_dir_all(root).unwrap();
    }
}
