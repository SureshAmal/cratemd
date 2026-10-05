use std::fmt::Write;
use crate::analyzer::clean_rust_syntax;
use crate::model::{CrateIndex, ModuleNode, Symbol, SymbolKind};

pub struct DocGenerator;

impl DocGenerator {
    /// Generates an LLM-optimized single markdown document for the entire crate.
    /// Balances high information density with token efficiency.
    pub fn generate_llm_doc(index: &CrateIndex, full: bool, max_depth: usize) -> String {
        let mut out = String::new();

        // 1. Header & Crate Overview
        let _ = writeln!(out, "# Crate: {} v{}", index.info.name, index.info.version);
        if let Some(ref desc) = index.info.description {
            let _ = writeln!(out, "> {}\n", desc);
        } else {
            let _ = writeln!(out);
        }

        let _ = writeln!(out, "**Edition:** {} | **Public Symbols:** {} / {}",
            index.info.edition, index.stats.public_symbols, index.stats.total_symbols);

        if !index.info.features.is_empty() {
            let _ = writeln!(out, "**Features:** `{}`", index.info.features.join("`, `"));
        }
        let _ = writeln!(out);

        // 2. Architectural Blueprint / Module Tree
        let _ = writeln!(out, "## Module Architecture");
        let _ = writeln!(out, "```text");
        render_module_tree_text(&index.root_module, 0, max_depth, &mut out);
        let _ = writeln!(out, "```\n");

        // 3. Key Traits (Core Interfaces)
        let public_traits: Vec<&Symbol> = index
            .symbols
            .iter()
            .filter(|s| s.visibility.is_public() && s.kind == SymbolKind::Trait)
            .collect();

        if !public_traits.is_empty() {
            let _ = writeln!(out, "## Core Traits\n");
            let limit = if full { public_traits.len() } else { 15 };
            for t in public_traits.iter().take(limit) {
                render_symbol_markdown(t, full, &mut out);
            }
            if !full && public_traits.len() > limit {
                let _ = writeln!(out, "*... ({} more traits omitted for context efficiency)*\n", public_traits.len() - limit);
            }
        }

        // 4. Key Structs & Types
        let public_structs: Vec<&Symbol> = index
            .symbols
            .iter()
            .filter(|s| s.visibility.is_public() && (s.kind == SymbolKind::Struct || s.kind == SymbolKind::TypeAlias))
            .collect();

        if !public_structs.is_empty() {
            let _ = writeln!(out, "## Structs & Types\n");
            let limit = if full { public_structs.len() } else { 20 };
            for s in public_structs.iter().take(limit) {
                render_symbol_markdown(s, full, &mut out);
            }
            if !full && public_structs.len() > limit {
                let _ = writeln!(out, "*... ({} more structs/types omitted for context efficiency. Use 'cratemd view {} <symbol>' or 'cratemd doc {} --full')*\n",
                    public_structs.len() - limit, index.info.name, index.info.name);
            }
        }

        // 5. Enums
        let public_enums: Vec<&Symbol> = index
            .symbols
            .iter()
            .filter(|s| s.visibility.is_public() && s.kind == SymbolKind::Enum)
            .collect();

        if !public_enums.is_empty() {
            let _ = writeln!(out, "## Enums\n");
            let limit = if full { public_enums.len() } else { 12 };
            for e in public_enums.iter().take(limit) {
                render_symbol_markdown(e, full, &mut out);
            }
            if !full && public_enums.len() > limit {
                let _ = writeln!(out, "*... ({} more enums omitted for context efficiency)*\n", public_enums.len() - limit);
            }
        }

        // 6. Free Functions
        let public_fns: Vec<&Symbol> = index
            .symbols
            .iter()
            .filter(|s| s.visibility.is_public() && s.kind == SymbolKind::Function)
            .collect();

        if !public_fns.is_empty() {
            let _ = writeln!(out, "## Functions\n");
            let limit = if full { public_fns.len() } else { 15 };
            for f in public_fns.iter().take(limit) {
                render_symbol_markdown(f, full, &mut out);
            }
            if !full && public_fns.len() > limit {
                let _ = writeln!(out, "*... ({} more functions omitted for context efficiency)*\n", public_fns.len() - limit);
            }
        }

        // 7. Macros
        let public_macros: Vec<&Symbol> = index
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Macro)
            .collect();

        if !public_macros.is_empty() {
            let _ = writeln!(out, "## Macros\n");
            let limit = if full { public_macros.len() } else { 10 };
            for m in public_macros.iter().take(limit) {
                render_symbol_markdown(m, full, &mut out);
            }
            if !full && public_macros.len() > limit {
                let _ = writeln!(out, "*... ({} more macros omitted for context efficiency)*\n", public_macros.len() - limit);
            }
        }

        // 8. Standalone Examples summary if any
        if !index.standalone_examples.is_empty() {
            let _ = writeln!(out, "## Crate Examples");
            for ex in &index.standalone_examples {
                let _ = writeln!(out, "- `{}`", ex.title);
            }
            let _ = writeln!(out, "\n*(Use `cratemd examples {}` to view full example source)*\n", index.info.name);
        }

        out
    }

    /// Renders an ultra-compact outline of the crate (modules and public symbols)
    pub fn generate_outline(index: &CrateIndex, max_depth: usize) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "# Outline: {} v{}\n", index.info.name, index.info.version);
        render_outline_recursive(&index.root_module, 0, max_depth, &mut out);
        out
    }

    /// Generates an ultra-condensed cheat sheet (~500 tokens) for minimal LLM context usage
    pub fn generate_cheat_sheet(index: &CrateIndex) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "# Cheat Sheet: {} v{}", index.info.name, index.info.version);
        if let Some(ref desc) = index.info.description {
            let _ = writeln!(out, "> {}\n", desc);
        }

        // Top Structs
        let structs: Vec<&Symbol> = index.symbols.iter()
            .filter(|s| s.visibility.is_public() && s.kind == SymbolKind::Struct)
            .take(15)
            .collect();

        if !structs.is_empty() {
            let _ = writeln!(out, "### Types & Structs");
            let _ = writeln!(out, "```rust");
            for s in structs {
                let methods_summary: Vec<&str> = s.methods.iter()
                    .filter(|m| m.visibility.is_public())
                    .map(|m| m.name.as_str())
                    .take(6)
                    .collect();

                let impls_hint = if !s.trait_impls.is_empty() {
                    format!(" [impl: {}]", s.trait_impls.iter().take(4).cloned().collect::<Vec<_>>().join(", "))
                } else {
                    String::new()
                };

                let method_hint = if !methods_summary.is_empty() {
                    format!(" => .{{{}}}", methods_summary.join(", "))
                } else {
                    String::new()
                };

                let _ = writeln!(out, "{}{}{}", clean_rust_syntax(&s.signature), impls_hint, method_hint);
            }
            let _ = writeln!(out, "```\n");
        }

        // Core Traits
        let traits: Vec<&Symbol> = index.symbols.iter()
            .filter(|s| s.visibility.is_public() && s.kind == SymbolKind::Trait)
            .take(10)
            .collect();

        if !traits.is_empty() {
            let _ = writeln!(out, "### Key Traits");
            let _ = writeln!(out, "```rust");
            for t in traits {
                let _ = writeln!(out, "{}", clean_rust_syntax(&t.signature));
            }
            let _ = writeln!(out, "```\n");
        }

        // Key Functions
        let fns: Vec<&Symbol> = index.symbols.iter()
            .filter(|s| s.visibility.is_public() && s.kind == SymbolKind::Function)
            .take(12)
            .collect();

        if !fns.is_empty() {
            let _ = writeln!(out, "### Functions");
            let _ = writeln!(out, "```rust");
            for f in fns {
                let _ = writeln!(out, "{}", clean_rust_syntax(&f.signature));
            }
            let _ = writeln!(out, "```\n");
        }

        out
    }

    /// Renders detailed view of a single symbol
    pub fn render_symbol_detail(sym: &Symbol) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "# {} ({})\n", sym.id, sym.kind.as_str());
        let _ = writeln!(out, "**File:** `{}:{}`", sym.file_path, sym.line_start);
        let _ = writeln!(out, "**Visibility:** `{}`", sym.visibility.as_str());
        if let Some(ref p) = sym.parent {
            let _ = writeln!(out, "**Parent:** `{}`", p);
        }

        if !sym.trait_impls.is_empty() {
            let _ = writeln!(out, "**Implements:** `{}`", sym.trait_impls.join("`, `"));
        }
        if let Some(ref feat) = sym.feature {
            let _ = writeln!(out, "**Required Feature:** `{}`", feat);
        }
        let _ = writeln!(out);

        let clean_sig = clean_rust_syntax(&sym.signature);
        let _ = writeln!(out, "```rust");
        if let Some(ref detail) = sym.detail {
            if detail.starts_with("Re-export of") {
                let _ = writeln!(out, "// {}\n{}", detail, clean_sig);
            } else {
                let clean_detail = clean_rust_syntax(detail);
                let _ = writeln!(out, "{} {}", clean_sig.trim_end_matches(';'), clean_detail);
            }
        } else {
            let _ = writeln!(out, "{}", clean_sig);
        }
        let _ = writeln!(out, "```\n");

        if !sym.doc.is_empty() {
            let _ = writeln!(out, "## Documentation\n");
            let _ = writeln!(out, "{}\n", sym.doc);
        }

        if !sym.methods.is_empty() {
            let _ = writeln!(out, "## Methods ({})\n", sym.methods.len());
            let _ = writeln!(out, "```rust");
            let _ = writeln!(out, "impl {} {{", sym.name);
            for m in &sym.methods {
                let m_sig = clean_rust_syntax(&m.signature);
                if !m.doc.is_empty() {
                    let first_line = m.doc.lines().next().unwrap_or("").trim();
                    if !first_line.is_empty() {
                        let _ = writeln!(out, "    /// {}", first_line);
                    }
                }
                let _ = writeln!(out, "    {}", m_sig);
            }
            let _ = writeln!(out, "}}\n```\n");
        }

        if !sym.examples.is_empty() {
            let _ = writeln!(out, "## Examples\n");
            for ex in &sym.examples {
                let _ = writeln!(out, "### {}\n```rust\n{}\n```\n", ex.title, ex.code);
            }
        }

        out
    }

    /// Renders examples matching an optional query
    pub fn render_examples(index: &CrateIndex, query: Option<&str>) -> String {
        let mut out = String::new();
        let q = query.map(|s| s.to_lowercase());

        let _ = writeln!(out, "# Code Examples: {} v{}\n", index.info.name, index.info.version);

        let mut count = 0;

        // 1. Standalone examples in examples/
        for ex in &index.standalone_examples {
            let matches = match &q {
                Some(q_str) => ex.title.to_lowercase().contains(q_str) || ex.code.to_lowercase().contains(q_str),
                None => true,
            };

            if matches {
                count += 1;
                let _ = writeln!(out, "## File: `{}`\n", ex.title);
                let _ = writeln!(out, "```rust\n{}\n```\n", ex.code);
            }
        }

        // 2. Doc examples attached to symbols
        for sym in &index.symbols {
            for ex in &sym.examples {
                let matches = match &q {
                    Some(q_str) => {
                        sym.name.to_lowercase().contains(q_str)
                            || ex.title.to_lowercase().contains(q_str)
                            || ex.code.to_lowercase().contains(q_str)
                    }
                    None => true,
                };

                if matches {
                    count += 1;
                    let _ = writeln!(out, "## `{}`: {}\n", sym.id, ex.title);
                    let _ = writeln!(out, "```rust\n{}\n```\n", ex.code);
                }
            }
        }

        if count == 0 {
            if let Some(q_str) = query {
                let _ = writeln!(out, "No code examples found matching '{}'.", q_str);
            } else {
                let _ = writeln!(out, "No code examples found in docstrings or examples/ directory.");
            }
        }

        out
    }
}

fn render_symbol_markdown(sym: &Symbol, full: bool, out: &mut String) {
    let _ = writeln!(out, "### `{}` (`{}`)", sym.name, sym.module_path);
    if !sym.trait_impls.is_empty() {
        let _ = writeln!(out, "**Implements:** `{}`\n", sym.trait_impls.join("`, `"));
    }

    let clean_sig = clean_rust_syntax(&sym.signature);
    let _ = writeln!(out, "```rust");
    if let Some(ref detail) = sym.detail {
        let clean_det = clean_rust_syntax(detail);
        if full || detail.lines().count() <= 10 {
            let _ = writeln!(out, "{} {}", clean_sig.trim_end_matches(';'), clean_det);
        } else {
            // Truncate long bodies in compact mode
            let lines: Vec<&str> = clean_det.lines().take(8).collect();
            let _ = writeln!(out, "{} {}\n  // ... ({} more items)\n}}", clean_sig.trim_end_matches(';'), lines.join("\n"), detail.lines().count() - 8);
        }
    } else {
        let _ = writeln!(out, "{}", clean_sig);
    }
    let _ = writeln!(out, "```");

    if !sym.doc.is_empty() {
        if full {
            let _ = writeln!(out, "{}\n", sym.doc);
        } else {
            // First paragraph or line for token efficiency
            let summary = sym.doc.lines().take(3).collect::<Vec<&str>>().join("\n");
            let _ = writeln!(out, "{}\n", summary);
        }
    } else {
        let _ = writeln!(out);
    }

    if !sym.methods.is_empty() {
        let _ = writeln!(out, "<details><summary>Methods ({})</summary>\n", sym.methods.len());
        let _ = writeln!(out, "```rust");
        if full || sym.methods.len() <= 10 {
            for m in &sym.methods {
                let _ = writeln!(out, "{}", clean_rust_syntax(&m.signature));
            }
        } else {
            for m in sym.methods.iter().take(10) {
                let _ = writeln!(out, "{}", clean_rust_syntax(&m.signature));
            }
            let _ = writeln!(out, "  // ... ({} more methods. Use 'cratemd view <crate> {}' for full detail)", sym.methods.len() - 10, sym.name);
        }
        let _ = writeln!(out, "```\n</details>\n");
    }
}

fn render_module_tree_text(node: &ModuleNode, depth: usize, max_depth: usize, out: &mut String) {
    if depth > max_depth {
        return;
    }
    let indent = "  ".repeat(depth);
    let sym_count = node.symbols.iter().filter(|s| s.visibility.is_public()).count();
    let doc_hint = if !node.doc.is_empty() {
        let first = node.doc.lines().next().unwrap_or("").trim();
        if !first.is_empty() {
            format!(" - {}", first)
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    let _ = writeln!(out, "{}{} ({} items){}", indent, node.name, sym_count, doc_hint);

    for sub in &node.submodules {
        render_module_tree_text(sub, depth + 1, max_depth, out);
    }
}

fn render_outline_recursive(node: &ModuleNode, depth: usize, max_depth: usize, out: &mut String) {
    if depth > max_depth {
        return;
    }
    let indent = "  ".repeat(depth);
    let _ = writeln!(out, "{}* **{}** (`{}`)", indent, node.name, node.full_path);

    for sym in &node.symbols {
        if sym.visibility.is_public() {
            let _ = writeln!(out, "{}  - `{}`: `{}`", indent, sym.kind.as_str(), sym.name);
        }
    }

    for sub in &node.submodules {
        render_outline_recursive(sub, depth + 1, max_depth, out);
    }
}
