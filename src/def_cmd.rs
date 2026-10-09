use std::fmt::Write;
use std::path::{Path, PathBuf};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::analyzer::clean_rust_syntax;
use crate::locator::CrateLocator;
use crate::model::CrateIndex;
use crate::workspace::WorkspaceInfo;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolDefReport {
    pub query: String,
    pub total_matches: usize,
    pub matches: Vec<SymbolDefMatch>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolDefMatch {
    pub crate_name: String,
    pub is_workspace: bool,
    pub name: String,
    pub full_id: String,
    pub kind: String,
    pub visibility: String,
    pub file_path: String,
    pub abs_path: PathBuf,
    pub line_start: usize,
    pub line_end: usize,
    pub signature: String,
    pub doc: Option<String>,
    pub snippet: Option<String>,
}

pub struct DefFinder;

impl DefFinder {
    pub fn find(
        symbol_query: &str,
        target_dir: Option<&Path>,
        exact: bool,
        include_snippet: bool,
        limit: usize,
    ) -> Result<SymbolDefReport> {
        let query = symbol_query.trim();
        if query.is_empty() {
            bail!("Symbol query cannot be empty");
        }

        let start_dir = match target_dir {
            Some(p) => p.to_path_buf(),
            None => std::env::current_dir()?,
        };

        let ws_root = WorkspaceInfo::find_root(&start_dir).unwrap_or_else(|| start_dir.clone());
        let ws = WorkspaceInfo::load(&ws_root).ok();

        let mut matches = Vec::new();
        let query_lower = query.to_lowercase();

        // 1. If we are in a workspace, search each member crate's indexed symbols
        if let Some(ref w) = ws {
            for member in &w.members {
                if let Ok(info) = CrateLocator::locate(&member.abs_path.to_string_lossy()) {
                    let analyzer = crate::analyzer::CrateAnalyzer::new(info);
                    if let Ok(index) = analyzer.analyze() {
                        collect_matches_from_index(
                            &index,
                            &member.name,
                            true,
                            query,
                            &query_lower,
                            exact,
                            include_snippet,
                            &mut matches,
                        );
                    }
                }
            }
        } else if let Ok(info) = CrateLocator::locate(&ws_root.to_string_lossy()) {
            let crate_name = info.name.clone();
            let analyzer = crate::analyzer::CrateAnalyzer::new(info);
            if let Ok(index) = analyzer.analyze() {
                collect_matches_from_index(
                    &index,
                    &crate_name,
                    true,
                    query,
                    &query_lower,
                    exact,
                    include_snippet,
                    &mut matches,
                );
            }
        }

        // 2. If nothing found in workspace, fallback to scanning single files in the directory
        if matches.is_empty() {
            for entry in WalkDir::new(&ws_root).into_iter().filter_map(Result::ok) {
                let path = entry.path();
                if path.is_file() && path.extension().is_some_and(|e| e == "rs") {
                    if path.components().any(|c| c.as_os_str() == "target") {
                        continue;
                    }
                    if let Ok(file_rep) = crate::file_cmd::FileAnalyzer::analyze(path) {
                        for item in file_rep.items() {
                            let item_name_lower = item.name.to_lowercase();
                            let is_hit = if exact {
                                item.name == query
                            } else {
                                item.name == query || item_name_lower.contains(&query_lower)
                            };

                            if is_hit {
                                let snippet = if include_snippet {
                                    extract_snippet(path, item.line_start, item.line_end)
                                } else {
                                    None
                                };

                                matches.push(SymbolDefMatch {
                                    crate_name: "local".to_string(),
                                    is_workspace: true,
                                    name: item.name.clone(),
                                    full_id: item.name.clone(),
                                    kind: item.kind.clone(),
                                    visibility: item.visibility.clone(),
                                    file_path: path.to_string_lossy().to_string(),
                                    abs_path: path.to_path_buf(),
                                    line_start: item.line_start,
                                    line_end: item.line_end,
                                    signature: item.signature.clone(),
                                    doc: item.doc.clone(),
                                    snippet,
                                });
                            }
                        }
                    }
                }
            }
        }

        // Deduplicate matches by (file_path, line_start)
        let mut seen = std::collections::HashSet::new();
        matches.retain(|m| seen.insert((m.file_path.clone(), m.line_start)));

        // Sort exact name matches first, then shorter names
        matches.sort_by(|a, b| {
            let a_exact = a.name == query || a.full_id == query;
            let b_exact = b.name == query || b.full_id == query;
            b_exact.cmp(&a_exact).then_with(|| a.name.len().cmp(&b.name.len()))
        });

        let total_matches = matches.len();
        if matches.len() > limit {
            matches.truncate(limit);
        }

        Ok(SymbolDefReport {
            query: query.to_string(),
            total_matches,
            matches,
        })
    }

    pub fn render_markdown(report: &SymbolDefReport) -> String {
        let mut out = String::new();
        if report.matches.is_empty() {
            let _ = writeln!(out, "No definitions found matching `{}` in workspace.", report.query);
            return out;
        }

        let _ = writeln!(
            out,
            "# Definitions matching `{}` ({} found):\n",
            report.query, report.total_matches
        );

        for (i, m) in report.matches.iter().enumerate() {
            let clean_sig = clean_rust_syntax(&m.signature);
            let _ = writeln!(
                out,
                "### {}. `{}` [{}] ({})",
                i + 1,
                m.full_id,
                m.kind,
                m.crate_name
            );
            let _ = writeln!(out, "- **File:** `{}:{}-{}`", m.file_path, m.line_start, m.line_end);
            let _ = writeln!(out, "- **Visibility:** `{}`", m.visibility);
            let _ = writeln!(out, "- **Signature:** `{}`", clean_sig);

            if let Some(ref doc) = m.doc {
                let first_line = doc.lines().next().unwrap_or("").trim();
                if !first_line.is_empty() {
                    let _ = writeln!(out, "- **Doc:** {}", first_line);
                }
            }

            if let Some(ref snippet) = m.snippet {
                let _ = writeln!(out, "\n```rust\n{}\n```", snippet);
            }
            let _ = writeln!(out);
        }

        out
    }
}

fn collect_matches_from_index(
    index: &CrateIndex,
    crate_name: &str,
    is_workspace: bool,
    query: &str,
    query_lower: &str,
    exact: bool,
    include_snippet: bool,
    matches: &mut Vec<SymbolDefMatch>,
) {
    let mut check_symbol = |sym: &crate::model::Symbol| {
        let sym_name_lower = sym.name.to_lowercase();
        let sym_id_lower = sym.id.to_lowercase();

        let is_hit = if exact {
            sym.name == query || sym.id == query || sym.id.ends_with(&format!("::{}", query))
        } else {
            sym.name == query
                || sym.id == query
                || sym_name_lower.contains(query_lower)
                || sym_id_lower.contains(query_lower)
        };

        if is_hit {
            let abs_file = index.info.root_dir.join(&sym.file_path);
            let snippet = if include_snippet {
                extract_snippet(&abs_file, sym.line_start, sym.line_end)
            } else {
                None
            };

            matches.push(SymbolDefMatch {
                crate_name: crate_name.to_string(),
                is_workspace,
                name: sym.name.clone(),
                full_id: sym.id.clone(),
                kind: sym.kind.as_str().to_string(),
                visibility: sym.visibility.as_str().to_string(),
                file_path: sym.file_path.clone(),
                abs_path: abs_file,
                line_start: sym.line_start,
                line_end: sym.line_end,
                signature: sym.signature.clone(),
                doc: if sym.doc.is_empty() { None } else { Some(sym.doc.clone()) },
                snippet,
            });
        }
    };

    for sym in &index.symbols {
        check_symbol(sym);
        for method in &sym.methods {
            check_symbol(method);
        }
    }
}

fn extract_snippet(file_path: &Path, start_line: usize, end_line: usize) -> Option<String> {
    let content = std::fs::read_to_string(file_path).ok()?;
    let lines: Vec<&str> = content.lines().collect();
    if start_line == 0 || start_line > lines.len() {
        return None;
    }

    let s = start_line - 1;
    let e = end_line.min(lines.len()).max(start_line);
    // Limit snippet to at most 40 lines to preserve token efficiency
    let take_count = (e - s).min(40);
    let slice = &lines[s..s + take_count];
    let mut res = slice.join("\n");
    if e - s > 40 {
        res.push_str("\n    // ... (truncated for token brevity)");
    }
    Some(res)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_snippet() {
        let temp = std::env::temp_dir().join(format!("cratemd-def-test-{}", std::process::id()));
        std::fs::write(&temp, "line 1\nline 2\nline 3\nline 4\nline 5\n").unwrap();
        let snip = extract_snippet(&temp, 2, 4).unwrap();
        assert_eq!(snip, "line 2\nline 3\nline 4");
        std::fs::remove_file(&temp).unwrap();
    }
}
