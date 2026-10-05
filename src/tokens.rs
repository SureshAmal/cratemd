use crate::docgen::DocGenerator;
use crate::model::CrateIndex;
use crate::workspace::WorkspaceInfo;

/// Accurate token estimation for Rust code, Markdown, and text.
pub fn estimate_tokens(text: &str) -> usize {
    let chars = text.chars().count();
    let words = text.split_whitespace().count();
    let from_chars = chars / 4;
    let from_words = (words * 13) / 10;
    std::cmp::max(from_chars, from_words).max(1)
}

/// Applies a maximum token budget to text, truncating gracefully at line boundaries if needed.
pub fn apply_budget(text: String, max_tokens: Option<usize>) -> String {
    let Some(max) = max_tokens else {
        return text;
    };

    let est = estimate_tokens(&text);
    if est <= max {
        return text;
    }

    let mut current = String::new();
    let mut current_tokens = 0;
    let target_tokens = max.saturating_sub(35);

    for line in text.lines() {
        let line_tokens = estimate_tokens(line) + 1;
        if current_tokens + line_tokens > target_tokens {
            break;
        }
        current.push_str(line);
        current.push('\n');
        current_tokens += line_tokens;
    }

    use std::fmt::Write;
    let _ = writeln!(
        current,
        "\n[Output capped at ~{} tokens to fit environment budget (original: ~{} tokens). Use more specific queries or increase --max-tokens]",
        current_tokens, est
    );

    current
}

/// Renders a comprehensive context environment impact report for a crate.
pub fn render_crate_context_impact(index: &CrateIndex) -> String {
    use std::fmt::Write;
    let mut out = String::new();

    let cheat_text = DocGenerator::generate_cheat_sheet(index);
    let cheat_tokens = estimate_tokens(&cheat_text);

    let outline_text = DocGenerator::generate_outline(index, 3);
    let outline_tokens = estimate_tokens(&outline_text);

    let overview_text = DocGenerator::generate_llm_doc(index, false, 3);
    let overview_tokens = estimate_tokens(&overview_text);

    let full_text = DocGenerator::generate_llm_doc(index, true, 3);
    let full_tokens = estimate_tokens(&full_text);

    let _ = writeln!(out, "# Environment Context Impact: {} v{}\n", index.info.name, index.info.version);

    let _ = writeln!(out, "| View Mode | Command | Tokens | % of 128k Context | Environment Footprint |");
    let _ = writeln!(out, "|---|---|:---:|:---:|---|");

    let pct = |t: usize| -> String {
        format!("{:.1}%", (t as f64 / 131_072.0) * 100.0)
    };

    let _ = writeln!(out, "| Cheat Sheet | `cratemd cheat {}` | ~{} | {} | Minimal (key types & methods) |",
        index.info.name, cheat_tokens, pct(cheat_tokens));
    let _ = writeln!(out, "| Module Outline | `cratemd outline {}` | ~{} | {} | Low (hierarchy & module tree) |",
        index.info.name, outline_tokens, pct(outline_tokens));
    let _ = writeln!(out, "| Overview Doc | `cratemd {}` | ~{} | {} | Medium (core APIs & traits) |",
        index.info.name, overview_tokens, pct(overview_tokens));
    let _ = writeln!(out, "| Full Reference | `cratemd doc {} --full` | ~{} | {} | Heavy (complete crate dump) |",
        index.info.name, full_tokens, pct(full_tokens));

    let _ = writeln!(out, "\n## Symbol Breakdown");
    let _ = writeln!(out, "- Total Symbols: {} (Public: {})", index.stats.total_symbols, index.stats.public_symbols);
    let _ = writeln!(out, "- Structs: {} | Traits: {} | Enums: {} | Functions: {}",
        index.stats.structs_count, index.stats.traits_count, index.stats.enums_count, index.stats.functions_count);
    let _ = writeln!(out, "- Direct Dependencies: {}", index.info.dependencies.len());

    let _ = writeln!(out, "\n## Recommendations for LLM Agents");
    let _ = writeln!(out, "1. Prefer `cratemd cheat {}` (~{} tokens) to grasp key types without polluting context.", index.info.name, cheat_tokens);
    let _ = writeln!(out, "2. Use `cratemd search {} <query>` (~150 tokens) to find specific functions or methods.", index.info.name);
    let _ = writeln!(out, "3. Use `cratemd view {} <symbol>` (~80 tokens) for exact type definitions and signatures.", index.info.name);
    let _ = writeln!(out, "4. Use `--max-tokens <N>` on any command to enforce strict context boundaries.\n");

    out
}

/// Renders a comprehensive context environment impact report for a workspace.
pub fn render_workspace_context_impact(ws: &WorkspaceInfo) -> String {
    use std::fmt::Write;
    let mut out = String::new();

    let blueprint = ws.render_blueprint();
    let bp_tokens = estimate_tokens(&blueprint);

    let _ = writeln!(out, "# Environment Context Impact: Workspace `{}`\n", ws.root_dir.display());
    let _ = writeln!(out, "Member Crates: {} | Cargo.lock: {}\n",
        ws.members.len(),
        if ws.lockfile_present { "Present" } else { "Not found" }
    );

    let _ = writeln!(out, "| Workspace Member | Path | Internal Deps | External Deps |");
    let _ = writeln!(out, "|---|---|---|:---:|");
    for m in &ws.members {
        let int_str = if m.internal_deps.is_empty() { "-".to_string() } else { m.internal_deps.join(", ") };
        let _ = writeln!(out, "| `{}` | `{}` | {} | {} |",
            m.name, m.rel_path, int_str, m.external_deps.len());
    }

    let _ = writeln!(out, "\n**Workspace Blueprint Size:** ~{} tokens", bp_tokens);

    let _ = writeln!(out, "\n## Recommendations for LLM Agents in this Workspace");
    let _ = writeln!(out, "- Run `cratemd workspace` (~{} tokens) to inspect the architecture.", bp_tokens);
    let _ = writeln!(out, "- Run `cratemd find <query> -w` (~300 tokens) to search only across member crates.");
    let _ = writeln!(out, "- Run `cratemd deps` (~450 tokens) to inspect shared dependencies.");
    let _ = writeln!(out, "- Run `cratemd cheat <member>` for any individual member crate.\n");

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens() {
        let text = "fn main() {\n    println!(\"hello world\");\n}";
        let tokens = estimate_tokens(text);
        assert!(tokens > 0 && tokens < 20);
    }

    #[test]
    fn test_apply_budget_no_truncation() {
        let text = "short text".to_string();
        let budgeted = apply_budget(text.clone(), Some(100));
        assert_eq!(budgeted, text);
    }

    #[test]
    fn test_apply_budget_truncation() {
        let lines: Vec<String> = (0..200).map(|i| format!("line number {}", i)).collect();
        let text = lines.join("\n");
        let budgeted = apply_budget(text, Some(50));
        assert!(budgeted.contains("[Output capped at ~"));
    }
}
