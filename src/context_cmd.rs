use std::fmt::Write;
use std::path::{Path, PathBuf};
use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::calls_cmd::{CallsFinder, CallsReport};
use crate::def_cmd::{DefFinder, SymbolDefMatch};
use crate::refs_cmd::{WorkspaceRefsFinder, WorkspaceRefsReport};

/// Pipelined context report consolidating definition, callers/callees, and references
/// into a single token-budgeted output for LLMs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelinedContextReport {
    pub symbol_query: String,
    pub target_path: PathBuf,
    pub definition: Option<SymbolDefMatch>,
    pub additional_definitions: Vec<SymbolDefMatch>,
    pub calls: Option<CallsReport>,
    pub references: Option<WorkspaceRefsReport>,
}

pub struct PipelinedContextFinder;

impl PipelinedContextFinder {
    /// Collects definition, call hierarchy, and references in a single pipelined execution
    pub fn inspect(symbol: &str, target_dir: Option<&Path>) -> Result<PipelinedContextReport> {
        let sym = symbol.trim();
        let target_path = match target_dir {
            Some(p) => p.to_path_buf(),
            None => std::env::current_dir()?,
        };

        // 1. Definition (limit 3, with code snippet)
        let def_report = DefFinder::find(sym, Some(&target_path), false, true, 3).ok();

        let mut primary_def: Option<SymbolDefMatch> = None;
        let mut additional_defs: Vec<SymbolDefMatch> = Vec::new();

        if let Some(ref report) = def_report {
            let mut matches = report.matches.clone();
            if !matches.is_empty() {
                primary_def = Some(matches.remove(0));
                additional_defs = matches;
            }
        }

        // Determine symbol simple name for calls and references (e.g., "WorkspaceInfo::load" -> "load")
        let clean_name = if let Some(pos) = sym.rfind("::") {
            &sym[pos + 2..]
        } else {
            sym
        };

        // 2. Call hierarchy (incoming callers and outgoing calls)
        let calls_report = CallsFinder::find(clean_name, Some(&target_path), false, false).ok();

        // 3. Workspace references (top 5 references)
        let refs_report = WorkspaceRefsFinder::find(clean_name, Some(&target_path), 5).ok();

        Ok(PipelinedContextReport {
            symbol_query: sym.to_string(),
            target_path,
            definition: primary_def,
            additional_definitions: additional_defs,
            calls: calls_report,
            references: refs_report,
        })
    }

    /// Renders a concise, token-budgeted markdown representation of the pipelined context
    pub fn render_markdown(report: &PipelinedContextReport) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "# Context: `{}`\n", report.symbol_query);

        // --- 1. DEFINITION SECTION ---
        if let Some(ref def) = report.definition {
            let _ = writeln!(out, "## Definition");
            let vis_str = if def.visibility.is_empty() { "pub(crate)" } else { &def.visibility };
            let _ = writeln!(
                out,
                "- **Symbol**: `{}` (`{}`) [{}]",
                def.name, def.kind, vis_str
            );
            let _ = writeln!(
                out,
                "- **Location**: `{}:{}-{}` (crate `{}`)",
                def.file_path, def.line_start, def.line_end, def.crate_name
            );

            if let Some(ref doc) = def.doc {
                let first_line = doc.lines().next().unwrap_or("").trim();
                if !first_line.is_empty() {
                    let _ = writeln!(out, "- **Doc**: {}", first_line);
                }
            }

            // Compact code snippet (max 30 lines)
            if let Some(ref snip) = def.snippet {
                let lines: Vec<&str> = snip.lines().collect();
                let _ = writeln!(out, "\n```rust");
                let max_lines = 30;
                let take_count = lines.len().min(max_lines);
                for line in &lines[..take_count] {
                    let _ = writeln!(out, "{}", line);
                }
                if lines.len() > max_lines {
                    let _ = writeln!(out, "// ... ({} lines hidden)", lines.len() - max_lines);
                }
                let _ = writeln!(out, "```\n");
            } else {
                let _ = writeln!(out, "\n```rust\n{}\n```\n", def.signature);
            }

            if !report.additional_definitions.is_empty() {
                let _ = writeln!(out, "### Other Matching Definitions");
                for extra in &report.additional_definitions {
                    let _ = writeln!(
                        out,
                        "- `{}` in `{}:{}`",
                        extra.full_id, extra.file_path, extra.line_start
                    );
                }
                let _ = writeln!(out);
            }
        } else {
            let _ = writeln!(out, "## Definition\n*No direct definition found in workspace or dependencies.*\n");
        }

        // --- 2. CALL HIERARCHY SECTION ---
        if let Some(ref calls) = report.calls {
            let has_incoming = !calls.incoming.is_empty();
            let has_outgoing = !calls.outgoing.is_empty();

            if has_incoming || has_outgoing {
                let _ = writeln!(out, "## Call Hierarchy");

                if has_incoming {
                    let _ = writeln!(out, "### Incoming Callers (who calls `{}`):", calls.function_name);
                    for caller in calls.incoming.iter().take(5) {
                        let _ = writeln!(
                            out,
                            "- `{}` at `{}:{}`",
                            caller.caller_name, caller.file_path, caller.line_number
                        );
                    }
                    if calls.incoming.len() > 5 {
                        let _ = writeln!(out, "  *... and {} more callers*", calls.incoming.len() - 5);
                    }
                }

                if has_outgoing {
                    let _ = writeln!(out, "\n### Outgoing Calls (called by `{}`):", calls.function_name);
                    let mut out_sorted = calls.outgoing.clone();
                    out_sorted.sort();
                    let out_display: Vec<String> = out_sorted.into_iter().take(8).map(|c| format!("`{}`", c)).collect();
                    let _ = writeln!(out, "{}", out_display.join(", "));
                    if calls.outgoing.len() > 8 {
                        let _ = writeln!(out, "  *... and {} more callees*", calls.outgoing.len() - 8);
                    }
                }
                let _ = writeln!(out);
            }
        }

        // --- 3. REFERENCES SECTION ---
        if let Some(ref refs) = report.references {
            if refs.total_matches > 0 {
                let _ = writeln!(out, "## Key References");
                let _ = writeln!(
                    out,
                    "Found {} reference(s) across {} file(s):\n",
                    refs.total_matches, refs.files_matched
                );

                let mut count = 0;
                'outer: for file_match in &refs.results {
                    for occ in &file_match.occurrences {
                        let _ = writeln!(
                            out,
                            "- `{}:{}`: `{}`",
                            file_match.file_path,
                            occ.line_number,
                            occ.line_text.trim()
                        );
                        count += 1;
                        if count >= 5 {
                            break 'outer;
                        }
                    }
                }

                if refs.total_matches > count {
                    let _ = writeln!(out, "  *... and {} more references*", refs.total_matches - count);
                }
                let _ = writeln!(out);
            }
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipelined_context_aggregation() {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let report = PipelinedContextFinder::inspect("CrateLocator", Some(manifest_dir))
            .expect("should find context for CrateLocator");

        assert!(report.definition.is_some(), "Definition should be resolved");
        let def = report.definition.as_ref().unwrap();
        assert!(def.name.contains("CrateLocator"));

        let md = PipelinedContextFinder::render_markdown(&report);
        assert!(md.contains("# Context: `CrateLocator`"));
        assert!(md.contains("## Definition"));
    }
}
