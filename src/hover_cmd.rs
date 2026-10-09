use std::fmt::Write;
use std::path::Path;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use crate::analyzer::clean_rust_syntax;
use crate::def_cmd::DefFinder;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoverInfo {
    pub symbol: String,
    pub title: String,
    pub kind: String,
    pub visibility: String,
    pub crate_name: String,
    pub file_path: String,
    pub line_start: usize,
    pub line_end: usize,
    pub signature: String,
    pub doc: Option<String>,
}

pub struct HoverInspector;

impl HoverInspector {
    pub fn hover(
        symbol_or_target: &str,
        target_dir: Option<&Path>,
    ) -> Result<Option<HoverInfo>> {
        let sym = symbol_or_target.trim();
        if sym.is_empty() {
            bail!("Hover target cannot be empty");
        }

        // Use DefFinder to locate the best matching definition
        let rep = DefFinder::find(sym, target_dir, true, false, 5)?;
        let best = if !rep.matches.is_empty() {
            rep.matches.into_iter().next()
        } else {
            // Try non-exact if exact didn't match
            let rep_fuzzy = DefFinder::find(sym, target_dir, false, false, 5)?;
            rep_fuzzy.matches.into_iter().next()
        };

        if let Some(m) = best {
            Ok(Some(HoverInfo {
                symbol: sym.to_string(),
                title: m.full_id,
                kind: m.kind,
                visibility: m.visibility,
                crate_name: m.crate_name,
                file_path: m.file_path,
                line_start: m.line_start,
                line_end: m.line_end,
                signature: clean_rust_syntax(&m.signature),
                doc: m.doc,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn render_markdown(info: &HoverInfo) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "### `{}` [{}] ({})\n", info.title, info.kind, info.crate_name);
        let _ = writeln!(out, "```rust\n{}\n```", info.signature);
        let _ = writeln!(out, "- **Location:** `{}:{}-{}`", info.file_path, info.line_start, info.line_end);
        let _ = writeln!(out, "- **Visibility:** `{}`", info.visibility);

        if let Some(ref doc) = info.doc {
            let _ = writeln!(out, "\n#### Documentation\n{}", doc.trim());
        }

        out
    }
}
