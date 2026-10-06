use anyhow::{bail, Context, Result};
use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::model::{CrateInfo, FeatureDef};

pub struct CrateLocator;

impl CrateLocator {
    /// Locates a crate on the local system completely offline.
    /// Can accept:
    /// - "serde"
    /// - "serde@1.0.228"
    /// - "./relative/path/to/crate"
    /// - "/absolute/path/to/crate"
    pub fn locate(crate_spec: &str) -> Result<CrateInfo> {
        let (name, req_version) = parse_spec(crate_spec);

        // 1. If it's a direct path on disk
        let direct_path = PathBuf::from(&name);
        if direct_path.exists() {
            let manifest = if direct_path.is_file() && direct_path.file_name().is_some_and(|f| f == "Cargo.toml") {
                direct_path.clone()
            } else {
                direct_path.join("Cargo.toml")
            };

            if manifest.exists() {
                let root_dir = manifest.parent().unwrap().to_path_buf();
                return parse_cargo_manifest(&root_dir, &manifest);
            }
        }

        // 2. Check current directory / workspace / vendor
        if let Ok(info) = check_current_workspace(&name) {
            return Ok(info);
        }

        // 3. Search ~/.cargo/registry/src/
        if let Some(home) = home_dir() {
            let registry_src = home.join(".cargo/registry/src");
            if registry_src.exists()
                && let Some(info) = search_registry_dirs(&registry_src, &name, req_version.as_deref())? {
                    return Ok(info);
                }

            // 4. Search ~/.cargo/git/checkouts/
            let git_checkouts = home.join(".cargo/git/checkouts");
            if git_checkouts.exists()
                && let Some(info) = search_git_checkouts(&git_checkouts, &name)? {
                    return Ok(info);
                }
        }

        bail!(
            "Crate '{}' not found in local Cargo cache (~/.cargo/registry/src), git checkouts, or workspace.\n\
             Tip: Ensure the crate was fetched or compiled locally by cargo, or provide a path directly.",
            crate_spec
        );
    }

    /// List all crates available in the local cargo registry cache.
    pub fn list_cached(filter: Option<&str>) -> Result<Vec<(String, String, PathBuf)>> {
        let mut results = Vec::new();
        let Some(home) = home_dir() else {
            return Ok(results);
        };
        let registry_src = home.join(".cargo/registry/src");
        if !registry_src.exists() {
            return Ok(results);
        }

        for entry in std::fs::read_dir(&registry_src)? {
            let entry = entry?;
            let index_dir = entry.path();
            if index_dir.is_dir() {
                for crate_dir in std::fs::read_dir(&index_dir)? {
                    let crate_dir = crate_dir?;
                    let path = crate_dir.path();
                    if path.is_dir() {
                        let dir_name = crate_dir.file_name().to_string_lossy().to_string();
                        if let Some((crate_name, version)) = split_crate_version(&dir_name) {
                            if let Some(f) = filter
                                && !crate_name.contains(f) {
                                    continue;
                                }
                            results.push((crate_name, version, path));
                        }
                    }
                }
            }
        }

        results.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| compare_semver(&b.1, &a.1)));
        Ok(results)
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn parse_spec(spec: &str) -> (String, Option<String>) {
    if let Some((name, ver)) = spec.split_once('@') {
        (name.trim().to_string(), Some(ver.trim().to_string()))
    } else if let Some((name, ver)) = spec.split_once(':') {
        (name.trim().to_string(), Some(ver.trim().to_string()))
    } else {
        (spec.trim().to_string(), None)
    }
}

fn split_crate_version(dir_name: &str) -> Option<(String, String)> {
    // In Cargo cache, directory names are <crate-name>-<semver>.
    // Crate names only contain [a-zA-Z0-9_-], never '+' or '.'.
    // Find the hyphen where the version part begins with a digit.
    for (i, c) in dir_name.char_indices() {
        if c == '-' {
            let after = &dir_name[i + 1..];
            if after.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
                let name = &dir_name[..i];
                if !name.contains('+') && !name.contains('.') {
                    return Some((name.to_string(), after.to_string()));
                }
            }
        }
    }
    None
}

fn compare_semver(a: &str, b: &str) -> Ordering {
    let parse_nums = |s: &str| -> Vec<u64> {
        s.split(['.', '-', '+'])
            .filter_map(|part| part.parse::<u64>().ok())
            .collect()
    };
    let a_nums = parse_nums(a);
    let b_nums = parse_nums(b);
    a_nums.cmp(&b_nums)
}

fn search_registry_dirs(
    registry_src: &Path,
    target_name: &str,
    req_version: Option<&str>,
) -> Result<Option<CrateInfo>> {
    let mut matches = Vec::new();

    for index_entry in std::fs::read_dir(registry_src)? {
        let index_entry = index_entry?;
        let index_dir = index_entry.path();
        if !index_dir.is_dir() {
            continue;
        }

        for crate_entry in std::fs::read_dir(&index_dir)? {
            let crate_entry = crate_entry?;
            let path = crate_entry.path();
            if !path.is_dir() {
                continue;
            }

            let dir_name = crate_entry.file_name().to_string_lossy().to_string();
            if let Some((c_name, c_ver)) = split_crate_version(&dir_name)
                && c_name == target_name {
                    if let Some(req_ver) = req_version {
                        if c_ver == req_ver || c_ver.starts_with(req_ver) {
                            matches.push((c_ver, path));
                        }
                    } else {
                        matches.push((c_ver, path));
                    }
                }
        }
    }

    if matches.is_empty() {
        return Ok(None);
    }

    // Sort by version descending to get highest version
    matches.sort_by(|a, b| compare_semver(&b.0, &a.0));
    let (_, best_path) = &matches[0];
    let manifest = best_path.join("Cargo.toml");
    if manifest.exists() {
        let info = parse_cargo_manifest(best_path, &manifest)?;
        return Ok(Some(info));
    }

    Ok(None)
}

fn search_git_checkouts(git_checkouts: &Path, target_name: &str) -> Result<Option<CrateInfo>> {
    for repo_entry in WalkDir::new(git_checkouts)
        .max_depth(4)
        .into_iter()
        .filter_map(Result::ok)
    {
        if repo_entry.file_name() == "Cargo.toml" {
            let manifest = repo_entry.path();
            let root_dir = manifest.parent().unwrap();
            if let Ok(info) = parse_cargo_manifest(root_dir, manifest)
                && info.name == target_name {
                    return Ok(Some(info));
                }
        }
    }
    Ok(None)
}

fn check_current_workspace(target_name: &str) -> Result<CrateInfo> {
    let cwd = std::env::current_dir()?;
    // check if current dir has Cargo.toml and matches
    let cwd_manifest = cwd.join("Cargo.toml");
    if cwd_manifest.exists()
        && let Ok(info) = parse_cargo_manifest(&cwd, &cwd_manifest)
            && info.name == target_name {
                return Ok(info);
            }

    // check subdirectories
    let sub = cwd.join(target_name).join("Cargo.toml");
    if sub.exists() {
        let root = sub.parent().unwrap();
        return parse_cargo_manifest(root, &sub);
    }

    // check vendor/
    let vendor = cwd.join("vendor").join(target_name).join("Cargo.toml");
    if vendor.exists() {
        let root = vendor.parent().unwrap();
        return parse_cargo_manifest(root, &vendor);
    }

    bail!("Not in current workspace")
}

fn parse_cargo_manifest(root_dir: &Path, manifest_path: &Path) -> Result<CrateInfo> {
    let content = std::fs::read_to_string(manifest_path)
        .with_context(|| format!("Failed to read {}", manifest_path.display()))?;

    let toml_val: toml::Value = toml::from_str(&content)
        .with_context(|| format!("Failed to parse {}", manifest_path.display()))?;

    let pkg = toml_val.get("package");
    let name = pkg
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or_else(|| {
            root_dir
                .file_name()
                .map_or("unknown", |s| s.to_str().unwrap_or("unknown"))
        })
        .to_string();

    let version = pkg
        .and_then(|p| p.get("version"))
        .and_then(|v| v.as_str())
        .unwrap_or("0.0.0")
        .to_string();

    let edition = pkg
        .and_then(|p| p.get("edition"))
        .and_then(|e| e.as_str())
        .unwrap_or("2021")
        .to_string();

    let description = pkg
        .and_then(|p| p.get("description"))
        .and_then(|d| d.as_str())
        .map(|s| s.trim().to_string());

    // Locate lib.rs
    let lib_candidate = root_dir.join("src/lib.rs");
    let lib_path = if lib_candidate.exists() {
        Some(lib_candidate)
    } else {
        None
    };

    // Locate bin paths
    let mut bin_paths = Vec::new();
    let main_candidate = root_dir.join("src/main.rs");
    if main_candidate.exists() {
        bin_paths.push(main_candidate);
    }

    let bin_dir = root_dir.join("src/bin");
    if bin_dir.exists() && bin_dir.is_dir()
        && let Ok(entries) = std::fs::read_dir(bin_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().is_some_and(|ext| ext == "rs") {
                    bin_paths.push(p);
                }
            }
        }

    // Dependencies (standard, build, and target-specific)
    let mut dependencies = Vec::new();
    if let Some(deps) = toml_val.get("dependencies").and_then(|d| d.as_table()) {
        for key in deps.keys() {
            dependencies.push(key.clone());
        }
    }
    if let Some(deps) = toml_val.get("build-dependencies").and_then(|d| d.as_table()) {
        for key in deps.keys() {
            if !dependencies.contains(key) {
                dependencies.push(key.clone());
            }
        }
    }
    if let Some(targets) = toml_val.get("target").and_then(|t| t.as_table()) {
        for target in targets.values() {
            if let Some(deps) = target.get("dependencies").and_then(|d| d.as_table()) {
                for key in deps.keys() {
                    if !dependencies.contains(key) {
                        dependencies.push(key.clone());
                    }
                }
            }
        }
    }

    // Features
    let mut features = Vec::new();
    let mut feature_defs = Vec::new();
    if let Some(feats) = toml_val.get("features").and_then(|f| f.as_table()) {
        let default_list: Vec<String> = feats
            .get("default")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default();

        for (key, val) in feats {
            features.push(key.clone());
            let sub_features: Vec<String> = val
                .as_array()
                .map(|arr| arr.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                .unwrap_or_default();
            let is_default = key == "default" || default_list.contains(key);
            feature_defs.push(FeatureDef {
                name: key.clone(),
                is_default,
                sub_features,
            });
        }
    }

    Ok(CrateInfo {
        name,
        version,
        edition,
        description,
        root_dir: root_dir.to_path_buf(),
        manifest_path: manifest_path.to_path_buf(),
        lib_path,
        bin_paths,
        dependencies,
        features,
        feature_defs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_crate_version_standard() {
        let (name, ver) = split_crate_version("serde-1.0.229").unwrap();
        assert_eq!(name, "serde");
        assert_eq!(ver, "1.0.229");
    }

    #[test]
    fn test_split_crate_version_with_hyphens_in_name() {
        let (name, ver) = split_crate_version("tree-sitter-rust-0.24.2").unwrap();
        assert_eq!(name, "tree-sitter-rust");
        assert_eq!(ver, "0.24.2");

        let (name2, ver2) = split_crate_version("proc-macro2-1.0.107").unwrap();
        assert_eq!(name2, "proc-macro2");
        assert_eq!(ver2, "1.0.107");
    }

    #[test]
    fn test_split_crate_version_with_build_metadata() {
        let (name, ver) = split_crate_version("toml-1.1.6+spec-1.1.0").unwrap();
        assert_eq!(name, "toml");
        assert_eq!(ver, "1.1.6+spec-1.1.0");
    }

    #[test]
    fn test_compare_semver() {
        assert_eq!(compare_semver("1.0.0", "1.0.1"), Ordering::Less);
        assert_eq!(compare_semver("2.0.0", "1.99.99"), Ordering::Greater);
        assert_eq!(compare_semver("1.0.0", "1.0.0"), Ordering::Equal);
    }
}

