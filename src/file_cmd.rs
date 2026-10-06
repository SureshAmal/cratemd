use std::fmt::Write;
use std::fs;
use std::path::{Path, PathBuf};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use quote::ToTokens;
use syn::spanned::Spanned;

use crate::analyzer::{clean_rust_syntax, format_signature};
use crate::tokens;
use crate::treesitter_gen::TreeSitterGen;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SingleFileReport {
    pub file_path: String,
    pub absolute_path: PathBuf,
    pub lines_of_code: usize,
    pub file_bytes: usize,
    pub full_file_tokens: usize,
    pub outline_tokens: usize,
    pub token_savings_pct: f64,
    pub file_doc: Option<String>,
    pub items: Vec<FileItem>,
    pub parsed_with_syn: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileItem {
    pub kind: String, // "fn", "struct", "enum", "trait", "impl", "type", "const", "macro"
    pub name: String,
    pub signature: String,
    pub visibility: String,
    pub line_start: usize,
    pub line_end: usize,
    pub doc: Option<String>,
    pub details: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryReport {
    pub dir_path: String,
    pub absolute_path: PathBuf,
    pub total_files: usize,
    pub total_lines_of_code: usize,
    pub total_file_bytes: usize,
    pub total_full_tokens: usize,
    pub total_outline_tokens: usize,
    pub token_savings_pct: f64,
    pub files: Vec<SingleFileReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FileReport {
    Single(SingleFileReport),
    Directory(DirectoryReport),
}

impl FileReport {
    pub fn render_ascii(&self) -> String {
        match self {
            Self::Single(r) => r.render_ascii(),
            Self::Directory(d) => d.render_ascii(),
        }
    }

    pub fn render_symbol(&self, symbol_query: &str, include_body: bool) -> String {
        match self {
            Self::Single(r) => r.render_symbol(symbol_query, include_body),
            Self::Directory(d) => d.render_symbol(symbol_query, include_body),
        }
    }

    pub fn items(&self) -> Vec<&FileItem> {
        match self {
            Self::Single(r) => r.items.iter().collect(),
            Self::Directory(d) => d.files.iter().flat_map(|f| &f.items).collect(),
        }
    }
}

pub struct FileAnalyzer;

impl FileAnalyzer {
    pub fn analyze(path: &Path) -> Result<FileReport> {
        if !path.exists() {
            bail!("Path not found: {}", path.display());
        }

        if path.is_file() {
            let single = Self::analyze_file(path)?;
            Ok(FileReport::Single(single))
        } else if path.is_dir() {
            let dir = Self::analyze_dir(path)?;
            Ok(FileReport::Directory(dir))
        } else {
            bail!("Unsupported path type: {}", path.display());
        }
    }

    pub fn analyze_file(path: &Path) -> Result<SingleFileReport> {
        if !path.exists() {
            bail!("File not found: {}", path.display());
        }
        if !path.is_file() {
            bail!("Path is a directory, not a file: {}. Use `cratemd file` on directory or crate root.", path.display());
        }

        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read file: {}", path.display()))?;

        let lines_of_code = content.lines().count();
        let file_bytes = content.len();
        let full_file_tokens = tokens::estimate_tokens(&content);
        let abs_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

        // Try parsing with syn
        match syn::parse_file(&content) {
            Ok(syntax_tree) => {
                let (file_doc, items) = extract_syn_items(&syntax_tree, &content);
                let outline_md = render_items_markdown(path, lines_of_code, full_file_tokens, &file_doc, &items);
                let outline_tokens = tokens::estimate_tokens(&outline_md);
                let savings = if full_file_tokens > 0 {
                    (1.0 - (outline_tokens as f64 / full_file_tokens as f64)) * 100.0
                } else {
                    0.0
                };

                Ok(SingleFileReport {
                    file_path: path.to_string_lossy().to_string(),
                    absolute_path: abs_path,
                    lines_of_code,
                    file_bytes,
                    full_file_tokens,
                    outline_tokens,
                    token_savings_pct: savings.max(0.0),
                    file_doc,
                    items,
                    parsed_with_syn: true,
                })
            }
            Err(_) => {
                // Fallback to Tree-sitter outline
                let outline_content = TreeSitterGen::outline_file(path)
                    .unwrap_or_else(|_| content.clone());
                let outline_tokens = tokens::estimate_tokens(&outline_content);
                let savings = if full_file_tokens > 0 {
                    (1.0 - (outline_tokens as f64 / full_file_tokens as f64)) * 100.0
                } else {
                    0.0
                };

                Ok(SingleFileReport {
                    file_path: path.to_string_lossy().to_string(),
                    absolute_path: abs_path,
                    lines_of_code,
                    file_bytes,
                    full_file_tokens,
                    outline_tokens,
                    token_savings_pct: savings.max(0.0),
                    file_doc: None,
                    items: vec![FileItem {
                        kind: "outline".to_string(),
                        name: path.file_name().unwrap_or_default().to_string_lossy().to_string(),
                        signature: outline_content,
                        visibility: "pub".to_string(),
                        line_start: 1,
                        line_end: lines_of_code,
                        doc: None,
                        details: Vec::new(),
                    }],
                    parsed_with_syn: false,
                })
            }
        }
    }

    pub fn analyze_dir(path: &Path) -> Result<DirectoryReport> {
        if !path.exists() {
            bail!("Directory not found: {}", path.display());
        }
        if !path.is_dir() {
            bail!("Path is not a directory: {}", path.display());
        }

        let rs_paths = collect_rs_files(path, 6);
        if rs_paths.is_empty() {
            bail!("No Rust (.rs) files found in directory: {}", path.display());
        }

        let mut files = Vec::new();
        for p in rs_paths {
            if let Ok(report) = Self::analyze_file(&p) {
                files.push(report);
            }
        }

        if files.is_empty() {
            bail!("Failed to parse any Rust files in directory: {}", path.display());
        }

        let total_files = files.len();
        let total_lines_of_code = files.iter().map(|f| f.lines_of_code).sum();
        let total_file_bytes = files.iter().map(|f| f.file_bytes).sum();
        let total_full_tokens = files.iter().map(|f| f.full_file_tokens).sum();
        let total_outline_tokens = files.iter().map(|f| f.outline_tokens).sum();
        let token_savings_pct = if total_full_tokens > 0 {
            (1.0 - (total_outline_tokens as f64 / total_full_tokens as f64)) * 100.0
        } else {
            0.0
        };

        let abs_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

        Ok(DirectoryReport {
            dir_path: path.to_string_lossy().to_string(),
            absolute_path: abs_path,
            total_files,
            total_lines_of_code,
            total_file_bytes,
            total_full_tokens,
            total_outline_tokens,
            token_savings_pct: token_savings_pct.max(0.0),
            files,
        })
    }
}

impl SingleFileReport {
    pub fn render_ascii(&self) -> String {
        if !self.parsed_with_syn {
            let mut out = String::new();
            let _ = writeln!(out, "# File Outline: {} (Tree-sitter fallback)", self.file_path);
            let _ = writeln!(
                out,
                "Lines: {} | Size: {} bytes | Tokens: ~{} (full file ~{}, saved {:.1}%)\n",
                self.lines_of_code, self.file_bytes, self.outline_tokens, self.full_file_tokens, self.token_savings_pct
            );
            if let Some(item) = self.items.first() {
                out.push_str(&item.signature);
            }
            return out;
        }

        render_items_markdown(
            Path::new(&self.file_path),
            self.lines_of_code,
            self.full_file_tokens,
            &self.file_doc,
            &self.items,
        )
    }

    pub fn render_symbol(&self, symbol_query: &str, include_body: bool) -> String {
        let q = symbol_query.trim();
        let q_lower = q.to_lowercase();

        let matched: Vec<&FileItem> = self.items.iter().filter(|it| {
            it.name == q
                || it.name.eq_ignore_ascii_case(q)
                || it.name.to_lowercase().contains(&q_lower)
                || it.details.iter().any(|d| d.to_lowercase().contains(&q_lower))
        }).collect();

        if matched.is_empty() {
            let available: Vec<&str> = self.items.iter().map(|it| it.name.as_str()).take(15).collect();
            let more = if self.items.len() > 15 {
                format!(" and {} more", self.items.len() - 15)
            } else {
                String::new()
            };
            return format!(
                "Symbol '{}' not found in '{}'. Available symbols: {}{}.\nTip: Run `cratemd file {}` to see full outline.",
                q, self.file_path, available.join(", "), more, self.file_path
            );
        }

        let mut out = String::new();
        let raw_content = fs::read_to_string(&self.absolute_path).or_else(|_| fs::read_to_string(&self.file_path)).ok();
        let raw_lines: Vec<&str> = raw_content.as_deref().map(|c| c.lines().collect()).unwrap_or_default();

        for it in matched {
            let _ = writeln!(out, "# {} `{}` in `{}:{}-{}`\n", it.kind, it.name, self.file_path, it.line_start, it.line_end);
            let _ = writeln!(out, "**Visibility:** `{}`", it.visibility);
            let _ = writeln!(out, "**Lines:** L{}-L{} ({} lines)\n", it.line_start, it.line_end, it.line_end.saturating_sub(it.line_start) + 1);

            if let Some(ref doc) = it.doc {
                let _ = writeln!(out, "## Documentation\n{}\n", doc);
            }

            let _ = writeln!(out, "## Signature\n```rust\n{}\n```\n", it.signature);

            if !it.details.is_empty() {
                let _ = writeln!(out, "## Members / Details ({})\n", it.details.len());
                let _ = writeln!(out, "```rust");
                for d in &it.details {
                    let _ = writeln!(out, "    {}", d);
                }
                let _ = writeln!(out, "```\n");
            }

            if include_body && !raw_lines.is_empty() && it.line_start > 0 && it.line_start <= raw_lines.len() {
                let end = it.line_end.min(raw_lines.len());
                let body_slice = raw_lines[it.line_start - 1..end].join("\n");
                let _ = writeln!(out, "## Implementation (L{}-L{})\n```rust\n{}\n```\n", it.line_start, end, body_slice);
            }
        }

        out
    }
}

impl DirectoryReport {
    pub fn render_ascii(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(
            out,
            "# Directory Outline: `{}` ({} Rust files, {} lines, ~{} full tokens -> ~{} outline tokens, {:.1}% token savings)\n",
            self.dir_path,
            self.total_files,
            self.total_lines_of_code,
            self.total_full_tokens,
            self.total_outline_tokens,
            self.token_savings_pct,
        );

        let _ = writeln!(out, "| File | Lines | Tokens | Symbols | Key Exports |");
        let _ = writeln!(out, "|------|-------|--------|---------|-------------|");

        for f in &self.files {
            let key_exports: Vec<&str> = f
                .items
                .iter()
                .filter(|it| it.visibility == "pub")
                .map(|it| it.name.as_str())
                .take(4)
                .collect();
            let exports_str = if key_exports.is_empty() {
                "-".to_string()
            } else {
                key_exports.join(", ")
            };
            let _ = writeln!(
                out,
                "| `{}` | {} | ~{} | {} | {} |",
                f.file_path,
                f.lines_of_code,
                f.outline_tokens,
                f.items.len(),
                exports_str
            );
        }

        let _ = writeln!(
            out,
            "\nTip: Run `cratemd file <path/to/file.rs>` to see complete items and docstrings for a file."
        );
        out
    }

    pub fn render_symbol(&self, symbol_query: &str, include_body: bool) -> String {
        let q_lower = symbol_query.to_lowercase();
        let mut matching_files = 0;
        let mut out = String::new();

        for f in &self.files {
            let has_match = f.items.iter().any(|it| {
                it.name == symbol_query
                    || it.name.eq_ignore_ascii_case(symbol_query)
                    || it.name.to_lowercase().contains(&q_lower)
                    || it.details.iter().any(|d| d.to_lowercase().contains(&q_lower))
            });

            if has_match {
                matching_files += 1;
                out.push_str(&f.render_symbol(symbol_query, include_body));
                out.push('\n');
            }
        }

        if matching_files == 0 {
            format!(
                "Symbol '{}' not found across {} files in directory '{}'.",
                symbol_query, self.total_files, self.dir_path
            )
        } else {
            out
        }
    }
}

fn collect_rs_files(dir: &Path, max_depth: usize) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rs_files_recursive(dir, 0, max_depth, &mut files);
    files.sort();
    files
}

fn collect_rs_files_recursive(dir: &Path, depth: usize, max_depth: usize, acc: &mut Vec<PathBuf>) {
    if depth > max_depth {
        return;
    }
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if file_name.starts_with('.') || file_name == "target" || file_name == "node_modules" {
            continue;
        }

        if path.is_file() && path.extension().is_some_and(|e| e == "rs") {
            acc.push(path);
        } else if path.is_dir() {
            collect_rs_files_recursive(&path, depth + 1, max_depth, acc);
        }
    }
}

fn extract_syn_items(file: &syn::File, _raw_content: &str) -> (Option<String>, Vec<FileItem>) {
    let mut file_doc = None;
    let mut items = Vec::new();

    // Module doc comments (//! or /*! ... */)
    let inner_docs: Vec<String> = file.attrs.iter()
        .filter(|attr| attr.style == syn::AttrStyle::Inner(Default::default()))
        .map(|attr| crate::analyzer::extract_docs(std::slice::from_ref(attr)))
        .filter(|s| !s.is_empty())
        .collect();
    if !inner_docs.is_empty() {
        file_doc = Some(inner_docs.join("\n"));
    }

    for item in &file.items {
        let (start, end) = (item.span().start().line, item.span().end().line.max(item.span().start().line));
        match item {
            syn::Item::Fn(f) => {
                let vis = format_visibility(&f.vis);
                let sig = format_fn_signature(&f.sig, &vis);
                let doc = extract_item_doc(&f.attrs);
                items.push(FileItem {
                    kind: "fn".to_string(),
                    name: f.sig.ident.to_string(),
                    signature: sig,
                    visibility: vis,
                    line_start: start,
                    line_end: end,
                    doc,
                    details: Vec::new(),
                });
            }
            syn::Item::Struct(s) => {
                let vis = format_visibility(&s.vis);
                let doc = extract_item_doc(&s.attrs);
                let mut details = Vec::new();

                for field in &s.fields {
                    let field_vis = format_visibility(&field.vis);
                    let field_name = field.ident.as_ref().map(|i| i.to_string()).unwrap_or_else(|| "_".to_string());
                    let field_ty = clean_rust_syntax(&field.ty.to_token_stream().to_string());
                    let prefix = if field_vis.is_empty() { String::new() } else { format!("{} ", field_vis) };
                    details.push(format!("{}{}: {}", prefix, field_name, field_ty));
                }

                let sig = format!("{}struct {}", if vis.is_empty() { String::new() } else { format!("{} ", vis) }, s.ident);
                items.push(FileItem {
                    kind: "struct".to_string(),
                    name: s.ident.to_string(),
                    signature: sig,
                    visibility: vis,
                    line_start: start,
                    line_end: end,
                    doc,
                    details,
                });
            }
            syn::Item::Enum(e) => {
                let vis = format_visibility(&e.vis);
                let doc = extract_item_doc(&e.attrs);
                let mut details = Vec::new();

                for var in &e.variants {
                    let var_name = var.ident.to_string();
                    let field_str = match &var.fields {
                        syn::Fields::Unit => String::new(),
                        syn::Fields::Unnamed(fields) => {
                            let types: Vec<String> = fields.unnamed.iter().map(|f| clean_rust_syntax(&f.ty.to_token_stream().to_string())).collect();
                            format!("({})", types.join(", "))
                        }
                        syn::Fields::Named(fields) => {
                            let names: Vec<String> = fields.named.iter().map(|f| {
                                let id = f.ident.as_ref().map(|i| i.to_string()).unwrap_or_default();
                                let ty = clean_rust_syntax(&f.ty.to_token_stream().to_string());
                                format!("{}: {}", id, ty)
                            }).collect();
                            format!(" {{ {} }}", names.join(", "))
                        }
                    };
                    details.push(format!("{}{}", var_name, field_str));
                }

                let sig = format!("{}enum {}", if vis.is_empty() { String::new() } else { format!("{} ", vis) }, e.ident);
                items.push(FileItem {
                    kind: "enum".to_string(),
                    name: e.ident.to_string(),
                    signature: sig,
                    visibility: vis,
                    line_start: start,
                    line_end: end,
                    doc,
                    details,
                });
            }
            syn::Item::Trait(t) => {
                let vis = format_visibility(&t.vis);
                let doc = extract_item_doc(&t.attrs);
                let mut details = Vec::new();

                for trait_item in &t.items {
                    if let syn::TraitItem::Fn(tf) = trait_item {
                        let sig_str = format_fn_signature(&tf.sig, "");
                        details.push(sig_str);
                    }
                }

                let sig = format!("{}trait {}", if vis.is_empty() { String::new() } else { format!("{} ", vis) }, t.ident);
                items.push(FileItem {
                    kind: "trait".to_string(),
                    name: t.ident.to_string(),
                    signature: sig,
                    visibility: vis,
                    line_start: start,
                    line_end: end,
                    doc,
                    details,
                });
            }
            syn::Item::Impl(im) => {
                let self_ty = clean_rust_syntax(&im.self_ty.to_token_stream().to_string());
                let trait_name = im.trait_.as_ref().map(|(path, _)| {
                    clean_rust_syntax(&path.to_token_stream().to_string())
                });

                let mut details = Vec::new();
                for impl_item in &im.items {
                    if let syn::ImplItem::Fn(im_fn) = impl_item {
                        let fn_vis = format_visibility(&im_fn.vis);
                        let fn_sig = format_fn_signature(&im_fn.sig, &fn_vis);
                        details.push(fn_sig);
                    }
                }

                let (name, sig) = if let Some(ref tr) = trait_name {
                    (format!("impl {} for {}", tr, self_ty), format!("impl {} for {}", tr, self_ty))
                } else {
                    (format!("impl {}", self_ty), format!("impl {}", self_ty))
                };

                items.push(FileItem {
                    kind: "impl".to_string(),
                    name,
                    signature: sig,
                    visibility: "pub".to_string(),
                    line_start: start,
                    line_end: end,
                    doc: None,
                    details,
                });
            }
            syn::Item::Type(ty) => {
                let vis = format_visibility(&ty.vis);
                let target = clean_rust_syntax(&ty.ty.to_token_stream().to_string());
                let sig = format!("{}type {} = {}", if vis.is_empty() { String::new() } else { format!("{} ", vis) }, ty.ident, target);
                items.push(FileItem {
                    kind: "type".to_string(),
                    name: ty.ident.to_string(),
                    signature: sig,
                    visibility: vis,
                    line_start: start,
                    line_end: end,
                    doc: extract_item_doc(&ty.attrs),
                    details: Vec::new(),
                });
            }
            syn::Item::Const(c) => {
                let vis = format_visibility(&c.vis);
                let ty = clean_rust_syntax(&c.ty.to_token_stream().to_string());
                let sig = format!("{}const {}: {}", if vis.is_empty() { String::new() } else { format!("{} ", vis) }, c.ident, ty);
                items.push(FileItem {
                    kind: "const".to_string(),
                    name: c.ident.to_string(),
                    signature: sig,
                    visibility: vis,
                    line_start: start,
                    line_end: end,
                    doc: extract_item_doc(&c.attrs),
                    details: Vec::new(),
                });
            }
            syn::Item::Macro(m) => {
                if let Some(ref id) = m.ident {
                    items.push(FileItem {
                        kind: "macro".to_string(),
                        name: id.to_string(),
                        signature: format!("macro_rules! {}", id),
                        visibility: "pub".to_string(),
                        line_start: start,
                        line_end: end,
                        doc: extract_item_doc(&m.attrs),
                        details: Vec::new(),
                    });
                }
            }
            _ => {}
        }
    }

    (file_doc, items)
}

fn render_items_markdown(
    path: &Path,
    loc: usize,
    full_tokens: usize,
    file_doc: &Option<String>,
    items: &[FileItem],
) -> String {
    let mut out = String::new();
    let filename = path.file_name().unwrap_or_default().to_string_lossy();

    let _ = writeln!(out, "# File: `{}` ({} lines, ~{} tokens)\n", filename, loc, full_tokens);

    if let Some(doc) = file_doc {
        let first_p = doc.lines().take(4).collect::<Vec<_>>().join("\n");
        let _ = writeln!(out, "{}", first_p);
        let _ = writeln!(out);
    }

    // Categorize
    let structs: Vec<&FileItem> = items.iter().filter(|i| i.kind == "struct").collect();
    let enums: Vec<&FileItem> = items.iter().filter(|i| i.kind == "enum").collect();
    let traits: Vec<&FileItem> = items.iter().filter(|i| i.kind == "trait").collect();
    let functions: Vec<&FileItem> = items.iter().filter(|i| i.kind == "fn").collect();
    let impls: Vec<&FileItem> = items.iter().filter(|i| i.kind == "impl").collect();
    let types: Vec<&FileItem> = items.iter().filter(|i| i.kind == "type" || i.kind == "const").collect();

    let _ = writeln!(
        out,
        "Outline: {} structs | {} enums | {} traits | {} fns | {} impls\n",
        structs.len(), enums.len(), traits.len(), functions.len(), impls.len()
    );

    if !structs.is_empty() {
        let _ = writeln!(out, "### Structs");
        for s in structs {
            let doc_str = s.doc.as_ref().map(|d| format!(" - {}", d.lines().next().unwrap_or_default())).unwrap_or_default();
            let _ = writeln!(out, "- `{}` (L{}-L{}){}", s.signature, s.line_start, s.line_end, doc_str);
            if !s.details.is_empty() && s.details.len() <= 6 {
                for d in &s.details {
                    let _ = writeln!(out, "    {}", d);
                }
            } else if s.details.len() > 6 {
                let _ = writeln!(out, "    ({} fields)", s.details.len());
            }
        }
        let _ = writeln!(out);
    }

    if !enums.is_empty() {
        let _ = writeln!(out, "### Enums");
        for e in enums {
            let doc_str = e.doc.as_ref().map(|d| format!(" - {}", d.lines().next().unwrap_or_default())).unwrap_or_default();
            let _ = writeln!(out, "- `{}` (L{}-L{}){}", e.signature, e.line_start, e.line_end, doc_str);
            if !e.details.is_empty() {
                let preview = e.details.iter().take(8).cloned().collect::<Vec<_>>().join(", ");
                let more = if e.details.len() > 8 { format!(", ... ({} total)", e.details.len()) } else { String::new() };
                let _ = writeln!(out, "    Variants: {}{}", preview, more);
            }
        }
        let _ = writeln!(out);
    }

    if !traits.is_empty() {
        let _ = writeln!(out, "### Traits");
        for t in traits {
            let _ = writeln!(out, "- `{}` (L{}-L{})", t.signature, t.line_start, t.line_end);
            for m in &t.details {
                let _ = writeln!(out, "    {}", m);
            }
        }
        let _ = writeln!(out);
    }

    if !functions.is_empty() {
        let _ = writeln!(out, "### Functions");
        for f in functions {
            let doc_str = f.doc.as_ref().map(|d| format!(" - {}", d.lines().next().unwrap_or_default())).unwrap_or_default();
            let _ = writeln!(out, "- `{}` (L{}-L{}){}", f.signature, f.line_start, f.line_end, doc_str);
        }
        let _ = writeln!(out);
    }

    if !impls.is_empty() {
        let _ = writeln!(out, "### Implementations");
        for im in impls {
            let _ = writeln!(out, "- `{}` (L{}-L{})", im.signature, im.line_start, im.line_end);
            for m in im.details.iter().take(6) {
                let _ = writeln!(out, "    {}", m);
            }
            if im.details.len() > 6 {
                let _ = writeln!(out, "    ... ({} methods total)", im.details.len());
            }
        }
        let _ = writeln!(out);
    }

    if !types.is_empty() {
        let _ = writeln!(out, "### Types & Constants");
        for ty in types {
            let _ = writeln!(out, "- `{}` (L{})", ty.signature, ty.line_start);
        }
        let _ = writeln!(out);
    }

    out
}

fn format_visibility(vis: &syn::Visibility) -> String {
    match vis {
        syn::Visibility::Public(_) => "pub".to_string(),
        syn::Visibility::Restricted(r) => clean_rust_syntax(&quote::quote!(#r).to_string()),
        syn::Visibility::Inherited => String::new(),
    }
}

fn format_fn_signature(sig: &syn::Signature, vis: &str) -> String {
    let base = format_signature(sig);
    if vis.is_empty() {
        base
    } else {
        format!("{} {}", vis, base)
    }
}

fn extract_item_doc(attrs: &[syn::Attribute]) -> Option<String> {
    let s = crate::analyzer::extract_docs(attrs);
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyze_source_code() {
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("cratemd_test_single.rs");
        let code = r#"
//! A sample module doc comment.

pub struct TestConfig {
    pub name: String,
    port: u16,
}

pub enum Status {
    Active,
    Inactive(String),
}

pub fn run_server(config: &TestConfig) -> bool {
    true
}

impl TestConfig {
    pub fn new(name: &str) -> Self {
        Self { name: name.to_string(), port: 8080 }
    }
}
"#;
        fs::write(&test_file, code).unwrap();
        let report = FileAnalyzer::analyze_file(&test_file).unwrap();

        assert_eq!(report.lines_of_code, code.lines().count());
        assert!(report.parsed_with_syn);
        assert!(report.file_doc.is_some());
        assert_eq!(report.items.iter().filter(|i| i.kind == "struct").count(), 1);
        assert_eq!(report.items.iter().filter(|i| i.kind == "enum").count(), 1);
        assert_eq!(report.items.iter().filter(|i| i.kind == "fn").count(), 1);
        assert_eq!(report.items.iter().filter(|i| i.kind == "impl").count(), 1);

        let sym_output = report.render_symbol("run_server", true);
        assert!(sym_output.contains("run_server"));
        assert!(sym_output.contains("pub fn run_server"));

        let not_found = report.render_symbol("nonexistent", false);
        assert!(not_found.contains("not found"));

        let _ = fs::remove_file(&test_file);
    }

    #[test]
    fn test_analyze_directory() {
        let temp_dir = std::env::temp_dir().join("cratemd_dir_test");
        let _ = fs::create_dir_all(&temp_dir);

        let file1 = temp_dir.join("mod_a.rs");
        let file2 = temp_dir.join("mod_b.rs");

        fs::write(&file1, "pub struct Alpha { pub id: usize }\npub fn alpha_fn() {}").unwrap();
        fs::write(&file2, "pub struct Beta { pub count: u32 }\npub fn beta_fn() {}").unwrap();

        let report = FileAnalyzer::analyze_dir(&temp_dir).unwrap();
        assert_eq!(report.total_files, 2);
        assert!(report.total_lines_of_code >= 4);

        let ascii = report.render_ascii();
        assert!(ascii.contains("Directory Outline"));
        assert!(ascii.contains("mod_a.rs"));
        assert!(ascii.contains("mod_b.rs"));

        let sym_output = report.render_symbol("alpha_fn", false);
        assert!(sym_output.contains("alpha_fn"));

        let _ = fs::remove_dir_all(temp_dir);
    }
}
