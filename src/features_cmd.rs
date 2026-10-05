use std::fmt::Write;
use serde::{Deserialize, Serialize};
use crate::model::CrateIndex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeaturesReport {
    pub crate_name: String,
    pub crate_version: String,
    pub total_features: usize,
    pub default_features: Vec<String>,
    pub features: Vec<FeatureReportItem>,
    pub inspected_feature: Option<String>,
    pub feature_symbols: Vec<String>,
}

impl FeaturesReport {
    pub fn render_ascii(&self) -> String {
        FeaturesInspector::render_markdown(self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureReportItem {
    pub name: String,
    pub is_default: bool,
    pub sub_features: Vec<String>,
}

pub struct FeaturesInspector;

impl FeaturesInspector {
    pub fn inspect(index: &CrateIndex, feature_filter: Option<&str>) -> FeaturesReport {
        let mut default_features = Vec::new();
        let mut features = Vec::new();

        for def in &index.info.feature_defs {
            if def.is_default && def.name != "default" {
                default_features.push(def.name.clone());
            }
            features.push(FeatureReportItem {
                name: def.name.clone(),
                is_default: def.is_default,
                sub_features: def.sub_features.clone(),
            });
        }

        // If feature_defs was empty, fallback to basic features list
        if features.is_empty() && !index.info.features.is_empty() {
            for f in &index.info.features {
                features.push(FeatureReportItem {
                    name: f.clone(),
                    is_default: false,
                    sub_features: Vec::new(),
                });
            }
        }

        features.sort_by(|a, b| a.name.cmp(&b.name));

        let mut feature_symbols = Vec::new();
        if let Some(target_f) = feature_filter {
            for sym in &index.symbols {
                if let Some(ref sym_f) = sym.feature {
                    if sym_f == target_f {
                        feature_symbols.push(format!("[{}] {}", sym.kind.as_str(), sym.id));
                    }
                }
            }
        }

        FeaturesReport {
            crate_name: index.info.name.clone(),
            crate_version: index.info.version.clone(),
            total_features: features.len(),
            default_features,
            features,
            inspected_feature: feature_filter.map(String::from),
            feature_symbols,
        }
    }

    pub fn render_markdown(report: &FeaturesReport) -> String {
        let mut out = String::new();

        if let Some(ref feat) = report.inspected_feature {
            let _ = writeln!(out, "# Feature `{}` in {} v{}\n", feat, report.crate_name, report.crate_version);

            let feat_item = report.features.iter().find(|f| f.name == *feat);
            if let Some(item) = feat_item {
                let def_badge = if item.is_default { "yes" } else { "no" };
                let _ = writeln!(out, "- **Default:** {}", def_badge);
                if !item.sub_features.is_empty() {
                    let _ = writeln!(out, "- **Sub-features / dependencies:** `{}`", item.sub_features.join("`, `"));
                }
            } else {
                let _ = writeln!(out, "- Note: Feature `{}` is not explicitly declared in Cargo.toml [features].", feat);
            }

            let _ = writeln!(out);
            if report.feature_symbols.is_empty() {
                let _ = writeln!(out, "No symbols explicitly gated by `#[cfg(feature = \"{}\")]` found in public API.", feat);
            } else {
                let _ = writeln!(out, "### Symbols gated by feature `{}` ({} items):\n", feat, report.feature_symbols.len());
                for sym in &report.feature_symbols {
                    let _ = writeln!(out, "- `{}`", sym);
                }
            }
            return out;
        }

        let _ = writeln!(out, "# Features in {} v{} ({} total)\n", report.crate_name, report.crate_version, report.total_features);

        if !report.default_features.is_empty() {
            let _ = writeln!(out, "### Default Features: `{}`\n", report.default_features.join("`, `"));
        } else {
            let _ = writeln!(out, "### Default Features: none\n");
        }

        if report.features.is_empty() {
            let _ = writeln!(out, "No feature flags declared in Cargo.toml.");
            return out;
        }

        let _ = writeln!(out, "### Available Features\n");
        let _ = writeln!(out, "| Feature | Default | Sub-features / Dependencies |");
        let _ = writeln!(out, "|:--------|:-------:|:----------------------------|");

        for item in &report.features {
            let def_badge = if item.is_default { "yes" } else { "no" };
            let sub = if item.sub_features.is_empty() {
                "-".to_string()
            } else {
                format!("`{}`", item.sub_features.join("`, `"))
            };
            let _ = writeln!(out, "| `{}` | {} | {} |", item.name, def_badge, sub);
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CrateInfo, CrateStats, FeatureDef};
    use std::path::PathBuf;

    #[test]
    fn test_features_inspection() {
        let info = CrateInfo {
            name: "test-crate".to_string(),
            version: "1.0.0".to_string(),
            edition: "2021".to_string(),
            description: None,
            root_dir: PathBuf::from("/tmp/test"),
            manifest_path: PathBuf::from("/tmp/test/Cargo.toml"),
            lib_path: None,
            bin_paths: vec![],
            features: vec!["extra".to_string()],
            feature_defs: vec![
                FeatureDef {
                    name: "default".to_string(),
                    sub_features: vec!["extra".to_string()],
                    is_default: true,
                },
                FeatureDef {
                    name: "extra".to_string(),
                    sub_features: vec![],
                    is_default: true,
                },
            ],
            dependencies: vec![],
        };
        let index = CrateIndex {
            info,
            stats: CrateStats::default(),
            symbols: vec![],
            root_module: crate::model::ModuleNode::default(),
            standalone_examples: vec![],
        };

        let report = FeaturesInspector::inspect(&index, None);
        assert_eq!(report.crate_name, "test-crate");
        assert_eq!(report.default_features, vec!["extra".to_string()]);
        assert_eq!(report.features.len(), 2);
    }
}

