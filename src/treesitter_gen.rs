use anyhow::{Context, Result};
use std::path::Path;
use tree_sitter::{Node, Parser};

pub struct TreeSitterGen;

impl TreeSitterGen {
    /// Generates a tree-sitter AST outline for a file or all entry files
    pub fn outline_file(file_path: &Path) -> Result<String> {
        let content = std::fs::read_to_string(file_path)
            .with_context(|| format!("Failed to read {}", file_path.display()))?;

        let mut parser = Parser::new();
        let language = tree_sitter_rust::LANGUAGE.into();
        parser.set_language(&language)?;

        let tree = parser
            .parse(&content, None)
            .context("Failed to parse Rust source code with tree-sitter")?;

        let root_node = tree.root_node();
        let mut output = String::new();
        format_node(root_node, &content, 0, &mut output);

        Ok(output)
    }

    /// Generates S-expression AST for a file
    pub fn sexp_file(file_path: &Path) -> Result<String> {
        let content = std::fs::read_to_string(file_path)
            .with_context(|| format!("Failed to read {}", file_path.display()))?;

        let mut parser = Parser::new();
        let language = tree_sitter_rust::LANGUAGE.into();
        parser.set_language(&language)?;

        let tree = parser
            .parse(&content, None)
            .context("Failed to parse Rust source code with tree-sitter")?;

        Ok(tree.root_node().to_sexp())
    }
}

fn format_node(node: Node, source: &str, depth: usize, out: &mut String) {
    let kind = node.kind();

    // Filter to interesting syntax nodes
    let is_interesting = matches!(
        kind,
        "struct_item"
            | "enum_item"
            | "function_item"
            | "trait_item"
            | "impl_item"
            | "type_item"
            | "mod_item"
            | "macro_definition"
            | "const_item"
            | "static_item"
    );

    if is_interesting {
        let indent = "  ".repeat(depth);
        let start_pos = node.start_position();
        let end_pos = node.end_position();

        let header = get_node_header(node, source);
        out.push_str(&format!(
            "{}[L{}-L{}] {}: {}\n",
            indent,
            start_pos.row + 1,
            end_pos.row + 1,
            kind,
            header
        ));

        // Recurse into children for items like impl_item, trait_item, mod_item
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.is_named() {
                format_node(child, source, depth + 1, out);
            }
        }
    } else {
        // Traverse children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.is_named() {
                format_node(child, source, depth, out);
            }
        }
    }
}

fn get_node_header<'a>(node: Node<'a>, source: &'a str) -> &'a str {
    // Return first line of the node
    let text = &source[node.start_byte()..node.end_byte()];
    let first_line = text.lines().next().unwrap_or("").trim();
    // Strip trailing opening braces
    first_line.trim_end_matches('{').trim()
}
