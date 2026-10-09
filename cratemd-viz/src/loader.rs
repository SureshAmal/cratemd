use anyhow::Result;
use egui::Pos2;
use petgraph::graph::NodeIndex;
use std::collections::HashMap;
use std::path::Path;

use cratemd::analyzer::CrateAnalyzer;
use cratemd::cache::CacheManager;
use cratemd::docgen::DocGenerator;
use cratemd::locator::CrateLocator;
use cratemd::model::{CrateIndex, Symbol, SymbolKind};
use cratemd::workspace::WorkspaceInfo;

use crate::db::{MemoryNote, MetaEntry};
use crate::graph::{NodeKind, VizGraph};

pub struct ProjectLoader;

impl ProjectLoader {
    /// Loads the crate index from disk cache or analyzes source code directly offline.
    pub fn get_or_load_crate(crate_spec: &str, cache: &CacheManager) -> Option<CrateIndex> {
        let info = CrateLocator::locate(crate_spec).ok()?;
        if let Some(cached) = cache.load(&info.name, &info.version, &info.root_dir) {
            return Some(cached);
        }
        let analyzer = CrateAnalyzer::new(info);
        if let Ok(index) = analyzer.analyze() {
            let _ = cache.store(&index);
            Some(index)
        } else {
            None
        }
    }

    pub fn populate_graph(
        graph: &mut VizGraph,
        project_root: &Path,
        memories: &[MemoryNote],
        meta: &[MetaEntry],
        current_focus: Option<&str>,
    ) -> Result<()> {
        graph.clear();

        let cache = CacheManager::new(true, false);

        // Detect workspace if present
        let ws_root = WorkspaceInfo::find_root(project_root).unwrap_or_else(|| project_root.to_path_buf());
        let ws_opt = WorkspaceInfo::load(&ws_root).ok();

        // 1. Root Workspace node
        let root_label = ws_opt
            .as_ref()
            .and_then(|w| w.root_dir.file_name().and_then(|n| n.to_str()))
            .or_else(|| {
                meta.iter()
                    .find(|m| m.key == "root_dir")
                    .and_then(|m| Path::new(&m.value).file_name().and_then(|n| n.to_str()))
            })
            .unwrap_or("Project");

        let mut root_details = if let Some(ref ws) = ws_opt {
            ws.render_blueprint()
        } else {
            format!("# Project: {}\n\n", root_label)
        };

        if !meta.is_empty() {
            root_details.push_str("\n### Metadata\n");
            for m in meta {
                root_details.push_str(&format!("- **{}**: {}\n", m.key, m.value));
            }
        }

        let root_idx = graph.add_or_get_node(
            "root",
            root_label,
            NodeKind::Workspace,
            &root_details,
            "workspace",
            Pos2::new(450.0, 300.0),
        );

        match current_focus {
            None => {
                // =========================================================================
                // LEVEL 0: TOP-LEVEL OVERVIEW
                // Shows workspace root, member crates, and top-level dependencies
                // =========================================================================
                let mut member_crates = Vec::new();
                let mut direct_deps = Vec::new();

                if let Some(ref ws) = ws_opt {
                    for member in &ws.members {
                        member_crates.push(member.clone());
                        for dep in &member.external_deps {
                            if !direct_deps.contains(dep) {
                                direct_deps.push(dep.clone());
                            }
                        }
                    }
                } else if let Ok(info) = CrateLocator::locate(&project_root.to_string_lossy()) {
                    for dep in &info.dependencies {
                        if !direct_deps.contains(dep) {
                            direct_deps.push(dep.clone());
                        }
                    }
                }

                // Place member crates in an inner orbit
                let member_count = member_crates.len().max(1);
                let member_angle_step = std::f32::consts::TAU / (member_count as f32);
                let mut member_nodes: HashMap<String, NodeIndex> = HashMap::new();

                for (i, member) in member_crates.iter().enumerate() {
                    let angle = (i as f32) * member_angle_step;
                    let pos = Pos2::new(450.0 + 180.0 * angle.cos(), 300.0 + 180.0 * angle.sin());

                    let crate_id = format!("crate:{}", member.name);

                    // Load full crate index to generate rich details
                    let crate_details = if let Some(index) = Self::get_or_load_crate(&member.abs_path.to_string_lossy(), &cache) {
                        DocGenerator::generate_llm_doc(&index, true, 3)
                    } else {
                        format!("# Crate: {} v{}\nPath: `{}`\n", member.name, member.version, member.rel_path)
                    };

                    let c_idx = graph.add_or_get_node(
                        &crate_id,
                        &member.name,
                        NodeKind::Crate,
                        &crate_details,
                        "crates",
                        pos,
                    );
                    graph.add_edge(root_idx, c_idx, "workspace_member", 1.2);
                    member_nodes.insert(member.name.clone(), c_idx);
                }

                // Connect internal workspace dependencies
                for member in &member_crates {
                    if let Some(&source_idx) = member_nodes.get(&member.name) {
                        for dep_name in &member.internal_deps {
                            if let Some(&dep_idx) = member_nodes.get(dep_name) {
                                graph.add_edge(source_idx, dep_idx, "internal_dep", 1.0);
                            }
                        }
                    }
                }

                // Place direct external dependencies in an outer orbit
                let dep_count = direct_deps.len().max(1);
                let dep_angle_step = std::f32::consts::TAU / (dep_count as f32);

                for (j, dep_name) in direct_deps.iter().enumerate() {
                    let angle = (j as f32) * dep_angle_step + 0.3;
                    let radius = 320.0 + ((j % 2) as f32) * 40.0;
                    let pos = Pos2::new(450.0 + radius * angle.cos(), 300.0 + radius * angle.sin());

                    let dep_id = format!("dep:{}", dep_name);

                    let dep_details = if let Some(index) = Self::get_or_load_crate(dep_name, &cache) {
                        DocGenerator::generate_cheat_sheet(&index)
                    } else {
                        format!("# Dependency: {}\nExternal dependency from cargo cache.\n", dep_name)
                    };

                    let d_idx = graph.add_or_get_node(
                        &dep_id,
                        dep_name,
                        NodeKind::Dependency,
                        &dep_details,
                        "dependencies",
                        pos,
                    );
                    graph.add_edge(root_idx, d_idx, "depends_on", 0.8);
                }

                // Include any custom user memory notes (excluding symbols)
                for mem in memories {
                    if !mem.key.starts_with("symbol:")
                        && !mem.key.starts_with("crate:")
                        && !mem.key.starts_with("dep:")
                        && mem.key != "workspace:blueprint"
                    {
                        let m_idx = graph.add_or_get_node(
                            &mem.key,
                            &mem.key,
                            NodeKind::Memory,
                            &mem.content,
                            &mem.category,
                            Pos2::new(450.0, 120.0),
                        );
                        graph.add_edge(root_idx, m_idx, "memory_note", 0.7);
                    }
                }
            }
            Some(target_focus) => {
                // =========================================================================
                // HIERARCHICAL DRILL-DOWN: CRATE LEVEL vs MODULE LEVEL
                // =========================================================================
                let is_mod = target_focus.starts_with("mod:");
                let (crate_lookup_name, target_mod_path) = if is_mod {
                    let mod_path = target_focus.trim_start_matches("mod:");
                    let first_segment = mod_path.split("::").next().unwrap_or(mod_path);
                    (first_segment.to_string(), Some(mod_path.to_string()))
                } else {
                    let clean = target_focus
                        .trim_start_matches("crate:")
                        .trim_start_matches("dep:");
                    (clean.to_string(), None)
                };

                // Resolve crate path or name
                let crate_lookup = if let Some(ref ws) = ws_opt {
                    ws.members
                        .iter()
                        .find(|m| m.name == crate_lookup_name)
                        .map(|m| m.abs_path.to_string_lossy().to_string())
                        .unwrap_or_else(|| crate_lookup_name.clone())
                } else {
                    crate_lookup_name.clone()
                };

                let index_opt = Self::get_or_load_crate(&crate_lookup, &cache);

                if let Some(index) = index_opt {
                    let crate_root_path = index.info.root_dir.clone();

                    if let Some(ref target_module) = target_mod_path {
                        // -----------------------------------------------------------------
                        // LEVEL 2: MODULE DEEP DIVE (e.g. mod:rmcp::transport)
                        // Shows items directly inside this module and direct submodules!
                        // -----------------------------------------------------------------
                        let short_mod_name = target_module.rsplit("::").next().unwrap_or(target_module);
                        let center_idx = graph.add_or_get_node(
                            target_focus,
                            short_mod_name,
                            NodeKind::Module,
                            &format!("# Module: `{}`\n\nPart of crate `{}`.", target_module, index.info.name),
                            "module",
                            Pos2::new(450.0, 300.0),
                        );

                        // Direct symbols inside this module
                        let direct_symbols: Vec<&Symbol> = index
                            .symbols
                            .iter()
                            .filter(|s| s.visibility.is_public() && s.kind != SymbolKind::Module)
                            .filter(|s| s.module_path == *target_module || (s.module_path.is_empty() && target_module == &index.info.name))
                            .collect();

                        // Immediate child submodules
                        let prefix = format!("{}::", target_module);
                        let mut direct_submods: Vec<String> = Vec::new();
                        for sym in &index.symbols {
                            let mpath = if sym.kind == SymbolKind::Module {
                                if sym.module_path.is_empty() {
                                    format!("{}::{}", index.info.name, sym.name)
                                } else {
                                    format!("{}::{}", sym.module_path, sym.name)
                                }
                            } else {
                                sym.module_path.clone()
                            };

                            if let Some(rest) = mpath.strip_prefix(&prefix) {
                                let child_submod = rest.split("::").next().unwrap_or("");
                                if !child_submod.is_empty() {
                                    let full_submod = format!("{}{}", prefix, child_submod);
                                    if !direct_submods.contains(&full_submod) {
                                        direct_submods.push(full_submod);
                                    }
                                }
                            }
                        }

                        // Orbit 1: Direct submodules
                        let submod_count = direct_submods.len().max(1);
                        let submod_angle_step = std::f32::consts::TAU / (submod_count as f32);
                        for (i, submod) in direct_submods.iter().enumerate() {
                            let angle = (i as f32) * submod_angle_step;
                            let pos = Pos2::new(450.0 + 170.0 * angle.cos(), 300.0 + 170.0 * angle.sin());
                            let short_name = submod.rsplit("::").next().unwrap_or(submod);
                            let mod_id = format!("mod:{}", submod);
                            let sub_idx = graph.add_or_get_node(
                                &mod_id,
                                short_name,
                                NodeKind::Module,
                                &format!("# Submodule: `{}`\nClick or double-click to explore.", submod),
                                "submodule",
                                pos,
                            );
                            graph.add_edge(center_idx, sub_idx, "submodule", 1.4);
                        }

                        // Orbit 2: Direct symbols inside module
                        let sym_count = direct_symbols.len().max(1);
                        let sym_angle_step = std::f32::consts::TAU / (sym_count as f32);
                        for (j, sym) in direct_symbols.iter().enumerate() {
                            let angle = (j as f32) * sym_angle_step + 0.2;
                            let radius = 290.0 + ((j % 3) as f32) * 35.0;
                            let pos = Pos2::new(450.0 + radius * angle.cos(), 300.0 + radius * angle.sin());

                            let kind = match sym.kind {
                                SymbolKind::Struct => NodeKind::Struct,
                                SymbolKind::Enum => NodeKind::Enum,
                                SymbolKind::Trait => NodeKind::Trait,
                                SymbolKind::Function => NodeKind::Function,
                                _ => NodeKind::Symbol,
                            };

                            let sym_details = DocGenerator::render_symbol_detail_with_source(
                                sym,
                                Some(&crate_root_path),
                                true,
                            );

                            let sym_id = format!("symbol:{}::{}", index.info.name, sym.name);
                            let sym_idx = graph.add_or_get_node(
                                &sym_id,
                                &sym.name,
                                kind,
                                &sym_details,
                                kind.label(),
                                pos,
                            );
                            graph.add_edge(center_idx, sym_idx, "declares", 1.1);
                        }
                    } else {
                        // -----------------------------------------------------------------
                        // LEVEL 1: CRATE / DEPENDENCY ARCHITECTURE
                        // Shows center crate, immediate top-level modules, and root symbols!
                        // (Avoids dumping thousands of nested items at once)
                        // -----------------------------------------------------------------
                        let is_dep = target_focus.starts_with("dep:");
                        let main_kind = if is_dep { NodeKind::Dependency } else { NodeKind::Crate };
                        let main_label = index.info.name.clone();
                        let main_details = DocGenerator::generate_llm_doc(&index, true, 3);

                        let center_idx = graph.add_or_get_node(
                            target_focus,
                            &main_label,
                            main_kind,
                            &main_details,
                            "focused_crate",
                            Pos2::new(450.0, 300.0),
                        );

                        // Find top-level modules (e.g. `rmcp::transport`, `rmcp::server`, etc.)
                        let crate_prefix = format!("{}::", index.info.name);
                        let mut top_modules: Vec<String> = Vec::new();

                        for sym in &index.symbols {
                            let mpath = if sym.kind == SymbolKind::Module {
                                if sym.module_path.is_empty() {
                                    format!("{}::{}", index.info.name, sym.name)
                                } else {
                                    format!("{}::{}", sym.module_path, sym.name)
                                }
                            } else {
                                sym.module_path.clone()
                            };

                            let relative = if let Some(stripped) = mpath.strip_prefix(&crate_prefix) {
                                stripped
                            } else if mpath == index.info.name {
                                ""
                            } else {
                                &mpath
                            };

                            if !relative.is_empty() {
                                let top_mod_name = relative.split("::").next().unwrap_or(relative);
                                let full_mod = format!("{}{}", crate_prefix, top_mod_name);
                                if !top_modules.contains(&full_mod) {
                                    top_modules.push(full_mod);
                                }
                            }
                        }

                        // Orbit 1: Top-level modules
                        let mod_count = top_modules.len().max(1);
                        let mod_angle_step = std::f32::consts::TAU / (mod_count as f32);
                        for (i, mod_path) in top_modules.iter().enumerate() {
                            let angle = (i as f32) * mod_angle_step;
                            let pos = Pos2::new(450.0 + 190.0 * angle.cos(), 300.0 + 190.0 * angle.sin());
                            let short_name = mod_path.rsplit("::").next().unwrap_or(mod_path);
                            let mod_id = format!("mod:{}", mod_path);
                            let m_details = format!(
                                "# Module: `{}`\n\nTop-level architectural module of `{}`.\n\nDouble-click or click **Explore** to drill into structs, traits, and submodules.",
                                mod_path, index.info.name
                            );
                            let m_idx = graph.add_or_get_node(
                                &mod_id,
                                short_name,
                                NodeKind::Module,
                                &m_details,
                                "module",
                                pos,
                            );
                            graph.add_edge(center_idx, m_idx, "module", 1.4);
                        }

                        // Orbit 2: Top-level / root exported public symbols
                        let root_symbols: Vec<&Symbol> = index
                            .symbols
                            .iter()
                            .filter(|s| s.visibility.is_public() && s.kind != SymbolKind::Module)
                            .filter(|s| s.module_path.is_empty() || s.module_path == index.info.name)
                            .collect();

                        let root_sym_count = root_symbols.len().max(1);
                        let root_angle_step = std::f32::consts::TAU / (root_sym_count as f32);
                        for (j, sym) in root_symbols.iter().enumerate() {
                            let angle = (j as f32) * root_angle_step + 0.25;
                            let radius = 310.0 + ((j % 2) as f32) * 40.0;
                            let pos = Pos2::new(450.0 + radius * angle.cos(), 300.0 + radius * angle.sin());

                            let kind = match sym.kind {
                                SymbolKind::Struct => NodeKind::Struct,
                                SymbolKind::Enum => NodeKind::Enum,
                                SymbolKind::Trait => NodeKind::Trait,
                                SymbolKind::Function => NodeKind::Function,
                                _ => NodeKind::Symbol,
                            };

                            let sym_details = DocGenerator::render_symbol_detail_with_source(
                                sym,
                                Some(&crate_root_path),
                                true,
                            );

                            let sym_id = format!("symbol:{}::{}", index.info.name, sym.name);
                            let sym_idx = graph.add_or_get_node(
                                &sym_id,
                                &sym.name,
                                kind,
                                &sym_details,
                                kind.label(),
                                pos,
                            );
                            graph.add_edge(center_idx, sym_idx, "exports", 1.0);
                        }
                    }
                } else {
                    // Fallback to SQLite memories if index couldn't be loaded from disk
                    for mem in memories {
                        let is_match = mem.key == target_focus
                            || mem.key.starts_with(&format!("symbol:{}::", crate_lookup_name));
                        if is_match {
                            let short = mem.key.rsplit("::").next().unwrap_or(&mem.key);
                            let idx = graph.add_or_get_node(
                                &mem.key,
                                short,
                                NodeKind::Struct,
                                &mem.content,
                                &mem.category,
                                Pos2::new(450.0, 300.0),
                            );
                            graph.add_edge(root_idx, idx, "contained_in", 1.0);
                        }
                    }
                }
            }
        }

        Ok(())
    }
}
