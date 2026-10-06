use std::collections::HashSet;
use std::path::PathBuf;
use anyhow::Result;
use serde::Serialize;

use crate::cache::CacheManager;
use crate::locator::CrateLocator;
use crate::model::Symbol;
use crate::search::{CrateSearcher, SearchQuery};
use crate::workspace::{parse_lockfile, WorkspaceInfo};

#[derive(Debug, Clone, Serialize)]
pub struct CrossHit {
    pub origin_crate: String,
    pub origin_version: String,
    pub is_workspace: bool,
    pub symbol: Symbol,
    pub score: i32,
    pub matched_in: Vec<&'static str>,
}

#[derive(Debug, Clone)]
pub struct FindArgs {
    pub query: String,
    pub path: Option<PathBuf>,
    pub workspace_only: bool,
    pub deps_only: bool,
    pub specific_crate: Option<String>,
    pub kind: Option<crate::cli::CliSymbolKind>,
    pub returns: Option<String>,
    pub takes: Option<String>,
    pub all: bool,
    pub doc: bool,
    pub limit: usize,
}

pub struct CrossSearcher;

impl CrossSearcher {
    pub fn find(args: &FindArgs, cache: &CacheManager) -> Result<Vec<CrossHit>> {
        let start_dir = args.path.clone().unwrap_or_else(|| PathBuf::from("."));
        let abs_start = if start_dir.is_relative() {
            std::env::current_dir()?.join(&start_dir)
        } else {
            start_dir
        };

        // 1. Detect if we are in a workspace or a single crate
        let ws_root = WorkspaceInfo::find_root(&abs_start);
        let mut target_crates: Vec<(String, PathBuf, bool)> = Vec::new(); // (name, path, is_workspace)
        let mut dep_specs: Vec<(String, Option<String>)> = Vec::new(); // (name, version)

        if let Some(ref root) = ws_root {
            if let Ok(ws) = WorkspaceInfo::load(root) {
                let lockfile_map = parse_lockfile(&root.join("Cargo.lock"));

                // Add workspace members
                if !args.deps_only {
                    for m in &ws.members {
                        if let Some(ref filter_crate) = args.specific_crate
                            && &m.name != filter_crate {
                                continue;
                            }
                        target_crates.push((m.name.clone(), m.abs_path.clone(), true));
                    }
                }

                // Add external dependencies
                if !args.workspace_only {
                    let mut seen_deps = HashSet::new();
                    for m in &ws.members {
                        for dep_name in &m.external_deps {
                            if seen_deps.insert(dep_name.clone()) {
                                if let Some(ref filter_crate) = args.specific_crate
                                    && dep_name != filter_crate {
                                        continue;
                                    }
                                let resolved_ver = lockfile_map.get(dep_name).cloned();
                                dep_specs.push((dep_name.clone(), resolved_ver));
                            }
                        }
                    }
                }
            }
        } else {
            // Single crate project
            if let Ok(info) = CrateLocator::locate(&abs_start.to_string_lossy()) {
                if !args.deps_only {
                    if let Some(ref filter_crate) = args.specific_crate {
                        if &info.name == filter_crate {
                            target_crates.push((info.name.clone(), info.root_dir.clone(), true));
                        }
                    } else {
                        target_crates.push((info.name.clone(), info.root_dir.clone(), true));
                    }
                }

                if !args.workspace_only {
                    let lock_map = parse_lockfile(&info.root_dir.join("Cargo.lock"));
                    for dep in &info.dependencies {
                        if let Some(ref filter_crate) = args.specific_crate
                            && dep != filter_crate {
                                continue;
                            }
                        let resolved_ver = lock_map.get(dep).cloned();
                        dep_specs.push((dep.clone(), resolved_ver));
                    }
                }
            }
        }

        let mut all_hits = Vec::new();

        let base_query = SearchQuery {
            text: args.query.clone(),
            kind_filter: args.kind.map(|k| k.to_model_kind()),
            pub_only: !args.all,
            search_docs: args.doc,
            search_signatures: true,
            returns_filter: args.returns.clone(),
            takes_filter: args.takes.clone(),
            limit: args.limit * 2, // Allow more per crate before overall truncation
        };

        // 2. Search workspace crates (direct on disk)
        for (c_name, c_path, is_ws) in target_crates {
            if let Ok(c_info) = CrateLocator::locate(&c_path.to_string_lossy()) {
                let index = if let Some(cached) = cache.load(&c_info.name, &c_info.version, &c_info.root_dir) {
                    cached
                } else {
                    let analyzer = crate::analyzer::CrateAnalyzer::new(c_info.clone());
                    if let Ok(idx) = analyzer.analyze() {
                        let _ = cache.store(&idx);
                        idx
                    } else {
                        continue;
                    }
                };

                let hits = CrateSearcher::search(&index, &base_query);
                for hit in hits {
                    // Boost score for workspace members so local code ranks higher
                    let score = hit.score + 35;
                    all_hits.push(CrossHit {
                        origin_crate: c_name.clone(),
                        origin_version: index.info.version.clone(),
                        is_workspace: is_ws,
                        symbol: hit.symbol,
                        score,
                        matched_in: hit.matched_in,
                    });
                }
            }
        }

        // 3. Search external dependencies (from local cache / registry)
        for (dep_name, dep_ver) in dep_specs {
            let spec = if let Some(ref v) = dep_ver {
                format!("{}@{}", dep_name, v)
            } else {
                dep_name.clone()
            };

            if let Ok(dep_info) = CrateLocator::locate(&spec) {
                let index = if let Some(cached) = cache.load(&dep_info.name, &dep_info.version, &dep_info.root_dir) {
                    cached
                } else {
                    let analyzer = crate::analyzer::CrateAnalyzer::new(dep_info.clone());
                    if let Ok(idx) = analyzer.analyze() {
                        let _ = cache.store(&idx);
                        idx
                    } else {
                        continue;
                    }
                };

                let hits = CrateSearcher::search(&index, &base_query);
                for hit in hits {
                    all_hits.push(CrossHit {
                        origin_crate: dep_name.clone(),
                        origin_version: index.info.version.clone(),
                        is_workspace: false,
                        symbol: hit.symbol,
                        score: hit.score,
                        matched_in: hit.matched_in,
                    });
                }
            }
        }

        // Sort descending by score
        all_hits.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.origin_crate.cmp(&b.origin_crate)));

        // Deduplicate
        all_hits.dedup_by(|a, b| a.symbol.id == b.symbol.id && a.origin_crate == b.origin_crate);

        if all_hits.len() > args.limit {
            all_hits.truncate(args.limit);
        }

        Ok(all_hits)
    }
}
