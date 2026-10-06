use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::locator::CrateLocator;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceMemberInfo {
    pub name: String,
    pub version: String,
    pub rel_path: String,
    pub abs_path: PathBuf,
    pub manifest_path: PathBuf,
    pub description: Option<String>,
    pub internal_deps: Vec<String>,
    pub external_deps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceInfo {
    pub root_dir: PathBuf,
    pub manifest_path: PathBuf,
    pub members: Vec<WorkspaceMemberInfo>,
    pub lockfile_present: bool,
}

impl WorkspaceInfo {
    /// Detects if `dir` or its ancestors is a Cargo workspace root.
    pub fn find_root(dir: &Path) -> Option<PathBuf> {
        let mut curr = if dir.is_file() {
            dir.parent()?.to_path_buf()
        } else {
            dir.to_path_buf()
        };

        let mut package_root = None;

        loop {
            let manifest = curr.join("Cargo.toml");
            if manifest.exists()
                && let Ok(content) = std::fs::read_to_string(&manifest)
                && let Ok(val) = toml::from_str::<toml::Value>(&content) {
                    if val.get("workspace").is_some() {
                        return Some(curr);
                    }
                    if package_root.is_none() && val.get("package").is_some() {
                        package_root = Some(curr.clone());
                    }
                }

            if !curr.pop() {
                break;
            }
        }

        package_root
    }

    /// Loads workspace information from the workspace root directory.
    pub fn load(root_dir: &Path) -> Result<Self> {
        let manifest_path = root_dir.join("Cargo.toml");
        let content = std::fs::read_to_string(&manifest_path)
            .with_context(|| format!("Failed to read {}", manifest_path.display()))?;

        let toml_val: toml::Value = toml::from_str(&content)
            .with_context(|| format!("Failed to parse {}", manifest_path.display()))?;

        let mut member_patterns = Vec::new();

        if let Some(ws) = toml_val.get("workspace").and_then(|w| w.as_table())
            && let Some(members) = ws.get("members").and_then(|m| m.as_array()) {
                for m in members {
                    if let Some(s) = m.as_str() {
                        member_patterns.push(s.to_string());
                    }
                }
            }

        // If package exists in root Cargo.toml, the root itself is also a crate
        let has_root_package = toml_val.get("package").is_some();
        if member_patterns.is_empty() && has_root_package {
            member_patterns.push(".".to_string());
        }

        let mut member_dirs = Vec::new();
        for pattern in member_patterns {
            expand_member_pattern(root_dir, &pattern, &mut member_dirs);
        }

        // If root has package and wasn't added, add it
        if has_root_package && !member_dirs.iter().any(|d| d == root_dir) {
            member_dirs.insert(0, root_dir.to_path_buf());
        }

        // Parse each member crate
        let mut raw_members = Vec::new();
        for dir in &member_dirs {
            let m_path = dir.join("Cargo.toml");
            if m_path.exists()
                && let Ok(info) = CrateLocator::locate(&dir.to_string_lossy()) {
                    raw_members.push((dir.clone(), m_path, info));
                }
        }

        let all_member_names: HashSet<String> = raw_members.iter().map(|(_, _, info)| info.name.clone()).collect();

        let mut members = Vec::new();
        for (dir, m_path, info) in raw_members {
            let mut internal_deps = Vec::new();
            let mut external_deps = Vec::new();

            for dep in &info.dependencies {
                if all_member_names.contains(dep) {
                    internal_deps.push(dep.clone());
                } else {
                    external_deps.push(dep.clone());
                }
            }

            internal_deps.sort();
            external_deps.sort();

            let rel_path = dir
                .strip_prefix(root_dir)
                .unwrap_or(&dir)
                .to_string_lossy()
                .to_string();

            let rel_path = if rel_path.is_empty() {
                ".".to_string()
            } else {
                rel_path
            };

            members.push(WorkspaceMemberInfo {
                name: info.name,
                version: info.version,
                rel_path,
                abs_path: dir,
                manifest_path: m_path,
                description: info.description,
                internal_deps,
                external_deps,
            });
        }

        members.sort_by(|a, b| a.name.cmp(&b.name));

        let lockfile_present = root_dir.join("Cargo.lock").exists();

        Ok(Self {
            root_dir: root_dir.to_path_buf(),
            manifest_path,
            members,
            lockfile_present,
        })
    }

    /// Renders a concise, high-density architecture blueprint of the workspace for LLMs.
    pub fn render_blueprint(&self) -> String {
        use std::fmt::Write;
        let mut out = String::new();

        let is_single = self.members.len() == 1 && self.members[0].rel_path == ".";
        let title = if is_single { "Project Architecture" } else { "Workspace Architecture" };

        let _ = writeln!(out, "# {}: `{}`\n", title, self.root_dir.display());
        let _ = writeln!(out, "**Member Crates:** {} | **Cargo.lock:** {}\n",
            self.members.len(),
            if self.lockfile_present { "Present" } else { "Not found" }
        );

        let _ = writeln!(out, "## {}\n", if is_single { "Crate Overview" } else { "Member Crates Overview" });

        for m in &self.members {
            let _ = writeln!(out, "### `{}` (v{})", m.name, m.version);
            let _ = writeln!(out, "- **Path:** `{}`", m.rel_path);
            if let Some(ref desc) = m.description {
                let _ = writeln!(out, "- **Description:** {}", desc);
            }

            if !m.internal_deps.is_empty() {
                let _ = writeln!(out, "- **Internal Workspace Deps:** `{}`", m.internal_deps.join("`, `"));
            }

            if !m.external_deps.is_empty() {
                let _ = writeln!(out, "- **External Deps ({}):** `{}`", m.external_deps.len(), m.external_deps.join("`, `"));
            }
            let _ = writeln!(out);
        }

        // Graph visualization if there are inter-dependencies
        let has_internal_edges = self.members.iter().any(|m| !m.internal_deps.is_empty());
        if has_internal_edges {
            let _ = writeln!(out, "## Workspace Dependency Hierarchy\n");
            let _ = writeln!(out, "```text");
            for m in &self.members {
                if !m.internal_deps.is_empty() {
                    let _ = writeln!(out, "{} -> {}", m.name, m.internal_deps.join(", "));
                } else {
                    let _ = writeln!(out, "{} (leaf/core)", m.name);
                }
            }
            let _ = writeln!(out, "```\n");
        }

        let _ = writeln!(out, "*Tips for LLMs:*");
        let _ = writeln!(out, "- Inspect any member crate: `cratemd cheat <member>`");
        let _ = writeln!(out, "- Search across entire workspace: `cratemd find <query> -w`");
        let _ = writeln!(out, "- Search workspace & all dependencies: `cratemd find <query>`\n");

        out
    }
}

fn expand_member_pattern(root: &Path, pattern: &str, out: &mut Vec<PathBuf>) {
    if pattern == "." {
        out.push(root.to_path_buf());
        return;
    }

    if let Some(prefix) = pattern.strip_suffix("/*") {
        let parent = root.join(prefix);
        if parent.is_dir()
            && let Ok(entries) = std::fs::read_dir(parent) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() && path.join("Cargo.toml").exists() {
                        out.push(path);
                    }
                }
            }
    } else {
        let path = root.join(pattern);
        if path.is_dir() && path.join("Cargo.toml").exists() {
            out.push(path);
        }
    }
}

/// Parses Cargo.lock and returns a map of (crate_name -> exact_version).
pub fn parse_lockfile(lockfile_path: &Path) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if !lockfile_path.exists() {
        return map;
    }

    let Ok(content) = std::fs::read_to_string(lockfile_path) else {
        return map;
    };

    let Ok(toml_val) = toml::from_str::<toml::Value>(&content) else {
        return map;
    };

    if let Some(packages) = toml_val.get("package").and_then(|p| p.as_array()) {
        for pkg in packages {
            if let (Some(name), Some(version)) = (
                pkg.get("name").and_then(|n| n.as_str()),
                pkg.get("version").and_then(|v| v.as_str()),
            ) {
                map.insert(name.to_string(), version.to_string());
            }
        }
    }

    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_root_non_workspace() {
        let root = WorkspaceInfo::find_root(Path::new("/tmp"));
        assert!(root.is_none());
    }

    #[test]
    fn test_find_root_single_crate() {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let root = WorkspaceInfo::find_root(manifest_dir);
        assert_eq!(root, Some(manifest_dir.to_path_buf()));
    }

    #[test]
    fn test_parse_lockfile_empty() {
        let map = parse_lockfile(Path::new("/nonexistent/Cargo.lock"));
        assert!(map.is_empty());
    }
}

