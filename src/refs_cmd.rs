use anyhow::{bail, Result};
use std::fmt::Write;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;
use regex::Regex;
use crate::workspace::WorkspaceInfo;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceRefsReport {
    pub query_symbol: String,
    pub workspace_root: PathBuf,
    pub total_matches: usize,
    pub files_matched: usize,
    pub results: Vec<RefFileMatch>,
}

impl WorkspaceRefsReport {
    pub fn render_ascii(&self) -> String {
        WorkspaceRefsFinder::render_markdown(self, usize::MAX)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefFileMatch {
    pub crate_name: String,
    pub file_path: String,
    pub occurrences: Vec<RefOccurrence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefOccurrence {
    pub line_number: usize,
    pub enclosing_item: Option<String>,
    pub line_text: String,
}

pub struct WorkspaceRefsFinder;

impl WorkspaceRefsFinder {
    pub fn find(symbol: &str, target_dir: Option<&Path>, limit: usize) -> Result<WorkspaceRefsReport> {
        let sym = symbol.trim();
        if sym.is_empty() {
            bail!("Search symbol cannot be empty");
        }

        let start_dir = match target_dir {
            Some(p) => p.to_path_buf(),
            None => std::env::current_dir()?,
        };

        let ws_root = WorkspaceInfo::find_root(&start_dir).unwrap_or_else(|| start_dir.clone());
        let ws = WorkspaceInfo::load(&ws_root).ok();

        // Pattern for matching symbol as an identifier or token
        let escaped = regex::escape(sym);
        let re_pattern = format!(r"\b{}\b", escaped);
        let re = Regex::new(&re_pattern)?;

        let mut ref_files: Vec<RefFileMatch> = Vec::new();
        let mut total_matches = 0;

        // Collect search directories (either workspace members or ws_root)
        let search_dirs: Vec<(String, PathBuf)> = if let Some(ref w) = ws {
            w.members.iter().map(|m| (m.name.clone(), m.abs_path.clone())).collect()
        } else {
            vec![("root".to_string(), ws_root.clone())]
        };

        for (crate_name, dir) in search_dirs {
            for entry in WalkDir::new(&dir).into_iter().filter_map(Result::ok) {
                let path = entry.path();
                if path.is_file() && path.extension().map_or(false, |ext| ext == "rs") {
                    // Skip target/ directory
                    if path.components().any(|c| c.as_os_str() == "target") {
                        continue;
                    }

                    if let Ok(content) = std::fs::read_to_string(path) {
                        if !re.is_match(&content) {
                            continue;
                        }

                        let rel_path = path.strip_prefix(&ws_root)
                            .unwrap_or(path)
                            .to_string_lossy()
                            .to_string();

                        let mut occurrences = Vec::new();
                        let lines: Vec<&str> = content.lines().collect();

                        for (idx, line) in lines.iter().enumerate() {
                            if re.is_match(line) {
                                let enclosing = find_enclosing_item(&lines, idx);
                                occurrences.push(RefOccurrence {
                                    line_number: idx + 1,
                                    enclosing_item: enclosing,
                                    line_text: line.trim().to_string(),
                                });
                                total_matches += 1;
                                if total_matches >= limit {
                                    break;
                                }
                            }
                        }

                        if !occurrences.is_empty() {
                            ref_files.push(RefFileMatch {
                                crate_name: crate_name.clone(),
                                file_path: rel_path,
                                occurrences,
                            });
                        }

                        if total_matches >= limit {
                            break;
                        }
                    }
                }
            }

            if total_matches >= limit {
                break;
            }
        }

        let files_matched = ref_files.len();

        Ok(WorkspaceRefsReport {
            query_symbol: sym.to_string(),
            workspace_root: ws_root,
            total_matches,
            files_matched,
            results: ref_files,
        })
    }

    pub fn render_markdown(report: &WorkspaceRefsReport, limit: usize) -> String {
        let mut out = String::new();
        let limit_note = if report.total_matches >= limit {
            format!(" (capped at limit {})", limit)
        } else {
            String::new()
        };

        let _ = writeln!(
            out,
            "Workspace references to '{}' ({} found across {} files){}:\n",
            report.query_symbol, report.total_matches, report.files_matched, limit_note
        );

        if report.results.is_empty() {
            let _ = writeln!(out, "No references found in workspace.");
            return out;
        }

        for file_match in &report.results {
            let _ = writeln!(out, "[{}] `{}`", file_match.crate_name, file_match.file_path);
            for occ in &file_match.occurrences {
                let ctx_str = occ.enclosing_item.as_deref().map(|c| format!(" (in {})", c)).unwrap_or_default();
                let _ = writeln!(out, "  Line {}{}:", occ.line_number, ctx_str);
                let _ = writeln!(out, "    {}", occ.line_text);
            }
            let _ = writeln!(out);
        }

        out
    }
}

fn find_enclosing_item(lines: &[&str], curr_idx: usize) -> Option<String> {
    // Scan backwards from curr_idx to find nearest fn or impl
    let mut i = curr_idx;
    while i > 0 {
        i -= 1;
        let trimmed = lines[i].trim();
        if trimmed.starts_with("pub fn ") || trimmed.starts_with("fn ") || trimmed.starts_with("pub async fn ") || trimmed.starts_with("async fn ") {
            let name_part = trimmed.split('(').next().unwrap_or(trimmed).trim();
            return Some(name_part.to_string());
        }
        if trimmed.starts_with("impl ") {
            let impl_part = trimmed.split('{').next().unwrap_or(trimmed).trim();
            return Some(impl_part.to_string());
        }
        if curr_idx - i > 60 {
            break;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_enclosing_item() {
        let code = [
            "pub fn process_data(input: &str) -> Result<()> {",
            "    let x = 42;",
            "    do_something(x);",
            "    Ok(())",
            "}",
        ];
        let enclosing = find_enclosing_item(&code, 2);
        assert_eq!(enclosing, Some("pub fn process_data".to_string()));
    }
}
