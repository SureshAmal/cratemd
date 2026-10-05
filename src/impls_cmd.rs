use std::fmt::Write;
use serde::{Deserialize, Serialize};
use crate::model::{CrateIndex, SymbolKind};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraitImplsReport {
    pub crate_name: String,
    pub crate_version: String,
    pub query: Option<String>,
    pub matches: Vec<TraitImplMatch>,
}

impl TraitImplsReport {
    pub fn render_ascii(&self) -> String {
        ImplsQuery::render_markdown(self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraitImplMatch {
    pub target_type: String,
    pub trait_name: String,
    pub file_path: String,
    pub line: usize,
    pub is_derived: bool,
}

pub struct ImplsQuery;

impl ImplsQuery {
    pub fn query(index: &CrateIndex, query_str: Option<&str>) -> TraitImplsReport {
        let q = query_str.map(|s| s.trim()).filter(|s| !s.is_empty());

        let mut matches = Vec::new();

        if let Some(target) = q {
            let target_lower = target.to_lowercase();

            // 1. Check if target is a struct/enum/type (finding traits it implements)
            for sym in &index.symbols {
                if sym.kind == SymbolKind::Struct || sym.kind == SymbolKind::Enum {
                    if sym.name.eq_ignore_ascii_case(target) || sym.id.ends_with(&format!("::{}", target)) {
                        // Derived traits
                        for derived in &sym.trait_impls {
                            matches.push(TraitImplMatch {
                                target_type: sym.name.clone(),
                                trait_name: derived.clone(),
                                file_path: sym.file_path.clone(),
                                line: sym.line_start,
                                is_derived: true,
                            });
                        }
                    }
                }
            }

            // Explicit impls where parent == target
            for sym in &index.symbols {
                if sym.kind == SymbolKind::Impl {
                    let parent_match = sym.parent.as_ref().map_or(false, |p| p.eq_ignore_ascii_case(target));
                    if parent_match {
                        if let Some(ref t_name) = sym.detail {
                            matches.push(TraitImplMatch {
                                target_type: sym.parent.clone().unwrap_or_default(),
                                trait_name: t_name.clone(),
                                file_path: sym.file_path.clone(),
                                line: sym.line_start,
                                is_derived: false,
                            });
                        }
                    }
                }
            }

            // 2. Also check if target is a trait (finding types that implement it)
            for sym in &index.symbols {
                if sym.kind == SymbolKind::Impl {
                    let trait_match = sym.detail.as_ref().map_or(false, |t| {
                        t.eq_ignore_ascii_case(target) || t.to_lowercase().contains(&target_lower)
                    });
                    if trait_match {
                        let target_ty = sym.parent.clone().unwrap_or_else(|| "Unknown".to_string());
                        let already_present = matches.iter().any(|m| m.target_type == target_ty && m.line == sym.line_start);
                        if !already_present {
                            matches.push(TraitImplMatch {
                                target_type: target_ty,
                                trait_name: sym.detail.clone().unwrap_or_else(|| target.to_string()),
                                file_path: sym.file_path.clone(),
                                line: sym.line_start,
                                is_derived: false,
                            });
                        }
                    }
                }
            }
        } else {
            // No query: collect all explicit trait implementations in the crate
            for sym in &index.symbols {
                if sym.kind == SymbolKind::Impl {
                    if let Some(ref t_name) = sym.detail {
                        matches.push(TraitImplMatch {
                            target_type: sym.parent.clone().unwrap_or_else(|| "Unknown".to_string()),
                            trait_name: t_name.clone(),
                            file_path: sym.file_path.clone(),
                            line: sym.line_start,
                            is_derived: false,
                        });
                    }
                }
            }
        }

        TraitImplsReport {
            crate_name: index.info.name.clone(),
            crate_version: index.info.version.clone(),
            query: q.map(String::from),
            matches,
        }
    }

    pub fn render_markdown(report: &TraitImplsReport) -> String {
        let mut out = String::new();

        if let Some(ref q) = report.query {
            let _ = writeln!(out, "# Trait Implementations for '{}' in {} v{}\n", q, report.crate_name, report.crate_version);

            if report.matches.is_empty() {
                let _ = writeln!(out, "No implementations found for '{}'.", q);
                return out;
            }

            // Group: traits implemented by type Q vs types implementing trait Q
            let traits_for_type: Vec<&TraitImplMatch> = report.matches.iter()
                .filter(|m| m.target_type.eq_ignore_ascii_case(q))
                .collect();

            let types_for_trait: Vec<&TraitImplMatch> = report.matches.iter()
                .filter(|m| !m.target_type.eq_ignore_ascii_case(q))
                .collect();

            if !traits_for_type.is_empty() {
                let _ = writeln!(out, "### Traits implemented by `{}` ({} traits):\n", q, traits_for_type.len());
                for m in traits_for_type {
                    let badge = if m.is_derived { " [derive]" } else { "" };
                    let _ = writeln!(out, "- `{}`{} ({}:{})", m.trait_name, badge, m.file_path, m.line);
                }
                let _ = writeln!(out);
            }

            if !types_for_trait.is_empty() {
                let _ = writeln!(out, "### Types implementing trait `{}` ({} types):\n", q, types_for_trait.len());
                for m in types_for_trait {
                    let _ = writeln!(out, "- `{}` ({}:{})", m.target_type, m.file_path, m.line);
                }
            }
        } else {
            let _ = writeln!(out, "# Trait Implementations in {} v{} ({} total impls)\n", report.crate_name, report.crate_version, report.matches.len());

            // Group by trait name
            let mut trait_map: std::collections::BTreeMap<String, Vec<&TraitImplMatch>> = std::collections::BTreeMap::new();
            for m in &report.matches {
                trait_map.entry(m.trait_name.clone()).or_default().push(m);
            }

            for (t_name, impls) in &trait_map {
                let _ = writeln!(out, "### Trait `{}` ({} implementors)", t_name, impls.len());
                let preview: Vec<String> = impls.iter().take(8).map(|m| format!("`{}` ({}:{})", m.target_type, m.file_path, m.line)).collect();
                let more = if impls.len() > 8 {
                    format!(", ... ({} total)", impls.len())
                } else {
                    String::new()
                };
                let _ = writeln!(out, "  Implementors: {}{}\n", preview.join(", "), more);
            }
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CrateInfo, CrateStats, Symbol, SymbolKind, Visibility};
    use std::path::PathBuf;

    #[test]
    fn test_trait_impls_query() {
        let info = CrateInfo {
            name: "test-crate".to_string(),
            version: "0.1.0".to_string(),
            edition: "2021".to_string(),
            description: None,
            root_dir: PathBuf::from("/tmp/test"),
            manifest_path: PathBuf::from("/tmp/test/Cargo.toml"),
            lib_path: None,
            bin_paths: vec![],
            features: vec![],
            feature_defs: vec![],
            dependencies: vec![],
        };

        let mut sym_struct = Symbol::new(
            "MyStruct".to_string(),
            "test::MyStruct".to_string(),
            SymbolKind::Struct,
            Visibility::Public,
            "struct MyStruct;".to_string(),
            "src/lib.rs".to_string(),
            10,
            12,
        );
        sym_struct.trait_impls = vec!["Clone".to_string(), "Debug".to_string()];

        let mut sym_impl = Symbol::new(
            "impl Display for MyStruct".to_string(),
            "test::(impl Display for MyStruct)".to_string(),
            SymbolKind::Impl,
            Visibility::Public,
            "impl Display for MyStruct {}".to_string(),
            "src/lib.rs".to_string(),
            20,
            25,
        );
        sym_impl.parent = Some("MyStruct".to_string());
        sym_impl.detail = Some("Display".to_string());

        let index = CrateIndex {
            info,
            stats: CrateStats::default(),
            symbols: vec![sym_struct, sym_impl],
            root_module: crate::model::ModuleNode::default(),
            standalone_examples: vec![],
        };

        let report = ImplsQuery::query(&index, Some("MyStruct"));
        assert_eq!(report.matches.len(), 3); // Clone (derived), Debug (derived), Display (explicit)
        let traits: Vec<&str> = report.matches.iter().map(|m| m.trait_name.as_str()).collect();
        assert!(traits.contains(&"Clone"));
        assert!(traits.contains(&"Debug"));
        assert!(traits.contains(&"Display"));
    }
}

