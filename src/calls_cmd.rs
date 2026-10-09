use std::collections::HashSet;
use std::fmt::Write;
use std::path::{Path, PathBuf};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Parser};
use walkdir::WalkDir;

use crate::workspace::WorkspaceInfo;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallsReport {
    pub function_name: String,
    pub incoming: Vec<CallSite>,
    pub outgoing: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallSite {
    pub caller_name: String,
    pub file_path: String,
    pub line_number: usize,
    pub code_line: String,
}

pub struct CallsFinder;

impl CallsFinder {
    /// Analyzes the workspace for incoming calls (callers) and outgoing calls (callees) for `fn_name`
    pub fn find(
        fn_name: &str,
        target_dir: Option<&Path>,
        incoming_only: bool,
        outgoing_only: bool,
    ) -> Result<CallsReport> {
        let fn_clean = fn_name.trim();
        if fn_clean.is_empty() {
            bail!("Function name cannot be empty");
        }

        let start_dir = match target_dir {
            Some(p) => p.to_path_buf(),
            None => std::env::current_dir()?,
        };

        let ws_root = WorkspaceInfo::find_root(&start_dir).unwrap_or_else(|| start_dir.clone());
        let ws = WorkspaceInfo::load(&ws_root).ok();

        let mut parser = Parser::new();
        let language = tree_sitter_rust::LANGUAGE.into();
        parser.set_language(&language)?;

        let mut search_files = Vec::new();
        let search_dirs: Vec<PathBuf> = if let Some(ref w) = ws {
            w.members.iter().map(|m| m.abs_path.clone()).collect()
        } else {
            vec![ws_root.clone()]
        };

        for dir in search_dirs {
            for entry in WalkDir::new(dir).into_iter().filter_map(Result::ok) {
                let path = entry.path();
                if path.is_file() && path.extension().is_some_and(|e| e == "rs") {
                    if path.components().any(|c| c.as_os_str() == "target") {
                        continue;
                    }
                    search_files.push(path.to_path_buf());
                }
            }
        }

        let mut incoming = Vec::new();
        let mut outgoing_set = HashSet::new();

        for file_path in &search_files {
            let Ok(content) = std::fs::read_to_string(file_path) else {
                continue;
            };

            // Fast path: if the file does not contain the function name at all, skip tree-sitter parse
            if !content.contains(fn_clean) {
                continue;
            }

            let Some(tree) = parser.parse(&content, None) else {
                continue;
            };

            let root = tree.root_node();
            let rel_path = file_path
                .strip_prefix(&ws_root)
                .unwrap_or(file_path)
                .to_string_lossy()
                .to_string();

            analyze_calls_in_node(
                root,
                &content,
                &rel_path,
                fn_clean,
                None,
                !outgoing_only, // scan incoming?
                !incoming_only, // scan outgoing?
                &mut incoming,
                &mut outgoing_set,
            );
        }

        let mut outgoing: Vec<String> = outgoing_set.into_iter().collect();
        outgoing.sort();

        Ok(CallsReport {
            function_name: fn_clean.to_string(),
            incoming,
            outgoing,
        })
    }

    pub fn render_markdown(report: &CallsReport) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "# Call Hierarchy for `{}`\n", report.function_name);

        let _ = writeln!(out, "## Incoming Calls (Callers - {} sites)", report.incoming.len());
        if report.incoming.is_empty() {
            let _ = writeln!(out, "_No direct workspace callers found._\n");
        } else {
            for site in &report.incoming {
                let _ = writeln!(
                    out,
                    "- in `{}` (`{}:{}`)\n  ```rust\n  {}\n  ```",
                    site.caller_name, site.file_path, site.line_number, site.code_line
                );
            }
            let _ = writeln!(out);
        }

        let _ = writeln!(out, "## Outgoing Calls (Callees inside `{}` - {} calls)", report.function_name, report.outgoing.len());
        if report.outgoing.is_empty() {
            let _ = writeln!(out, "_No outgoing calls found or function definition not located in workspace._\n");
        } else {
            for callee in &report.outgoing {
                let _ = writeln!(out, "- `{}`", callee);
            }
            let _ = writeln!(out);
        }

        out
    }
}

fn analyze_calls_in_node(
    node: Node,
    source: &str,
    file_path: &str,
    target_fn: &str,
    current_fn: Option<&str>,
    scan_incoming: bool,
    scan_outgoing: bool,
    incoming: &mut Vec<CallSite>,
    outgoing: &mut HashSet<String>,
) {
    let kind = node.kind();

    // Check if entering a function or method item
    let mut updated_fn = current_fn;
    if kind == "function_item" {
        if let Some(name_node) = node.child_by_field_name("name") {
            let fn_name = &source[name_node.start_byte()..name_node.end_byte()];
            updated_fn = Some(fn_name);
        }
    }

    // Is this a call_expression? (e.g. foo(arg) or obj.foo(arg) or Type::foo(arg))
    if kind == "call_expression" {
        if let Some(function_node) = node.child_by_field_name("function") {
            let call_target = extract_called_name(function_node, source);
            let caller_name = updated_fn.unwrap_or("<top-level>");

            // 1. Incoming call check: someone calls target_fn
            if scan_incoming && (call_target == target_fn || call_target.ends_with(&format!("::{}", target_fn))) {
                let line_num = node.start_position().row + 1;
                let line_text = source
                    .lines()
                    .nth(node.start_position().row)
                    .unwrap_or("")
                    .trim()
                    .to_string();

                incoming.push(CallSite {
                    caller_name: caller_name.to_string(),
                    file_path: file_path.to_string(),
                    line_number: line_num,
                    code_line: line_text,
                });
            }

            // 2. Outgoing call check: we are inside target_fn, and calling something else
            if scan_outgoing && updated_fn == Some(target_fn) && !call_target.is_empty() && call_target != target_fn {
                outgoing.insert(call_target);
            }
        }
    }

    // Recurse over children
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.is_named() {
            analyze_calls_in_node(
                child,
                source,
                file_path,
                target_fn,
                updated_fn,
                scan_incoming,
                scan_outgoing,
                incoming,
                outgoing,
            );
        }
    }
}

fn extract_called_name(node: Node, source: &str) -> String {
    let raw = &source[node.start_byte()..node.end_byte()];
    // If it's a field_expression like `self.do_something`, get the field name
    if node.kind() == "field_expression" {
        if let Some(field) = node.child_by_field_name("field") {
            return source[field.start_byte()..field.end_byte()].to_string();
        }
    }
    // If it's scoped_identifier like `CrateLocator::locate`, get the whole identifier
    raw.trim().to_string()
}
