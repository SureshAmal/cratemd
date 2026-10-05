use std::collections::HashMap;
use std::fmt::Write;
use std::path::PathBuf;
use anyhow::Result;
use serde::Serialize;

use crate::locator::CrateLocator;
use crate::workspace::{parse_lockfile, WorkspaceInfo};

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

            let mut dep_users: HashMap<String, Vec<String>> = HashMap::new();
            for m in &ws.members {
                for ext in &m.external_deps {
                    dep_users.entry(ext.clone()).or_default().push(m.name.clone());
                }
            }

            let mut items = Vec::new();
            let mut dep_names: Vec<String> = dep_users.keys().cloned().collect();
            dep_names.sort();

            for dep_name in dep_names {
                let used_by = dep_users.get(&dep_name).cloned().unwrap_or_default();
                let resolved_ver = lock_map.get(&dep_name).cloned();

                let spec = if let Some(ref v) = resolved_ver {
                    format!("{}@{}", dep_name, v)
                } else {
                    dep_name.clone()
                };

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

            let mut items = Vec::new();
            let mut deps = info.dependencies.clone();
            deps.sort();

            for dep_name in deps {
                let resolved_ver = lock_map.get(&dep_name).cloned();
                let spec = if let Some(ref v) = resolved_ver {
                    format!("{}@{}", dep_name, v)
                } else {
                    dep_name.clone()
                };

                let (cached_offline, description) = if let Ok(dep_info) = CrateLocator::locate(&spec) {
                    (true, dep_info.description)
                } else {
                    (false, None)
                };

                items.push(DepItem {
                    name: dep_name,
                    resolved_version: resolved_ver,
                    used_by: vec![info.name.clone()],
                    description,
                    cached_offline,
                });
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
