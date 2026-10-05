---
name: cratemd
description: >-
  Use this skill whenever you need to explore, inspect, search, or understand any Rust crate (e.g. tokio, serde, axum, clap, regex, or any local dependency) or multi-crate Cargo workspace.
  Allows LLMs to understand crate architectures, public APIs, structs, traits, method signatures, and project dependencies instantly and offline without reading individual source files.
---

# cratemd: Offline Rust Crate & Workspace Intelligence

`cratemd` provides instant, 100% offline intelligence for any Rust crate cached in Cargo (`~/.cargo/registry/src`), git checkouts, or local multi-crate workspaces.

Instead of burning tokens reading multiple `.rs` files or guessing API signatures, use `cratemd` to fetch clean, token-efficient summaries, search symbols, inspect definitions, and explore multi-crate workspaces in milliseconds (~2ms via persistent cache).

---

## When to Use This Skill

Activate and use this skill when:
- You need to understand a multi-crate Cargo workspace, its member crates, and their dependencies (`cratemd workspace`).
- You need to know which dependencies are used in a project and whether they are ready offline (`cratemd deps`).
- You need to find a symbol or function across both local workspace crates and external dependencies (`cratemd find <query>`).
- You need an ultra-condensed ~500-token cheat sheet for a crate (`cratemd cheat <crate>`).
- You need runnable code examples from docs and example directories (`cratemd examples <crate>`).
- You need to search by return type or parameter type (e.g. `--returns Result`, `--takes Stream`).
- You need the exact method signatures, types, or trait definitions from a crate without reading dozens of raw files (`cratemd view <crate> <symbol>`).

---

## Core Workflows

### 1. Multi-Crate Workspace Architecture

To understand the architecture and inter-crate dependency relationships of any workspace:

```bash
# In current workspace root
cratemd workspace

# Or targeting a specific path
cratemd workspace /path/to/project
```

Outputs:
- Member crates overview and relative paths
- Internal workspace dependencies and external dependency counts
- ASCII dependency hierarchy graph (e.g. `api -> core`, `cli -> core`)

---

### 2. Inspect Project & Workspace Dependencies

To inspect all dependencies, exact resolved versions from `Cargo.lock`, and offline readiness:

```bash
# In current project or workspace root
cratemd deps

# Or targeting a specific path
cratemd deps /path/to/project
```

Outputs a clean Markdown table with:
- Dependency name and resolved version
- Offline cache status (`ready` / `missing`)
- Member crates using each dependency
- One-line description

---

### 3. Unified Cross-Project Search (`cratemd find`)

Search across ALL workspace member crates and external dependencies simultaneously:

```bash
# Search across workspace code and external dependencies
cratemd find <query>

# Search only within local workspace members
cratemd find <query> -w

# Search only within external dependencies
cratemd find <query> -d

# Filter functions by return type
cratemd find --returns Result

# Filter functions by parameter type
cratemd find --takes TcpStream

# Restrict search to a specific crate
cratemd find <query> -c <crate_name>
```

Results rank local workspace code first, tagged with `[workspace: <crate>]` or `[dep: <crate> v<ver>]`, along with exact file path and line numbers.

---

### 4. Ultra-Condensed Crate Cheat Sheet (~500 tokens)

When you need an immediate, high-density summary of key structs, enums, traits, and functions without reading a full documentation manual:

```bash
cratemd cheat <crate_name>
```

---

### 5. Extract Code Examples

Extract runnable code snippets from documentation comments and the `examples/` directory:

```bash
# All examples in a crate
cratemd examples <crate_name>

# Filter examples by keyword
cratemd examples <crate_name> <keyword>
```

---

### 6. Single-Crate Overview & Full Docs

To generate full documentation for a single crate:

```bash
cratemd doc <crate_name>
cratemd doc <crate_name> --full --out <output_path.md>
```

---

### 7. Fast Symbol & Method Search in a Single Crate

```bash
# Search by symbol name or keyword
cratemd search <crate_name> <query>

# Filter by kind (fn, struct, enum, trait, method, type, macro)
cratemd search <crate_name> <query> --kind <kind>

# Search by return or argument type
cratemd search <crate_name> --returns Result
cratemd search <crate_name> --takes Context

# Include doc comments
cratemd search <crate_name> <query> --doc
```

---

### 8. Inspect a Specific Type, Trait, or Function in Detail

```bash
cratemd view <crate_name> <symbol_or_path>
```

Shows:
- Full declaration and visibility
- Implemented traits (both derived and explicit `impl Trait for Type`)
- All methods with signatures and documentation
- Doc examples

---

### 9. Single File Read & Analysis (`cratemd file`)

When you only need to inspect a single `.rs` file without indexing an entire crate, `cratemd` provides a token-efficient symbol outline with line numbers:

```bash
# Analyze a single Rust source file
cratemd file src/analyzer.rs

# Or shorthand directly
cratemd src/analyzer.rs
```

Outputs:
- Total lines of code and estimated tokens
- Exact line ranges (`L14-L26`) for every struct, enum, trait, function, and impl block
- Method outlines, fields, and docstrings
- Token savings typically exceeding 70-90% compared to reading the raw file

---

### 10. Cargo Features & Feature-Gates (`cratemd features`)

Inspect feature flags, default enabled features, and feature-gated symbols:

```bash
# Overview of all features in a crate
cratemd features <crate_name>

# Inspect symbols enabled by a specific feature
cratemd features <crate_name> --feature <feature_name>
```

---

### 11. Trait Implementations Query (`cratemd impls`)

Find all implementors of a trait or all traits implemented for a type:

```bash
# Query all implementations in a crate
cratemd impls <crate_name>

# Find implementors of a specific trait or type
cratemd impls <crate_name> <TypeOrTrait>
```

---

### 12. Workspace Cross-References (`cratemd refs`)

Find all usages and references of a symbol across all member crates in a workspace:

```bash
# Find references to a function, struct, or type across the workspace
cratemd refs <symbol_name>

# Target a specific workspace root
cratemd refs <symbol_name> --path /path/to/workspace
```

---

### 13. Dependency Health & Split Audit (`cratemd audit`)

Audit project dependencies for duplicate version splits and offline cache readiness:

```bash
# Audit current project dependencies
cratemd audit
```

---

### 14. Pre-Warm Local Cache (`cratemd warm`)

Pre-parse and index dependencies in the background for zero-latency queries:

```bash
# Pre-warm all dependencies in the workspace
cratemd warm --all
```

---

### 15. Built-in Model Context Protocol (MCP) Server (`cratemd mcp`)

`cratemd` has a native, zero-overhead MCP server communicating over standard IO (`stdio`) using JSON-RPC 2.0. Any AI agent or IDE (Antigravity, Claude Desktop, Cursor, Zed) can configure `cratemd` directly as an MCP tool provider:

```bash
cratemd mcp
```

#### Supported MCP Tools:
- `cratemd_doc`: Generate LLM-optimized single-document documentation.
- `cratemd_cheat`: Ultra-condensed ~500-token cheat sheet.
- `cratemd_search`: Search symbols, signatures, and docstrings.
- `cratemd_view`: Detailed inspection of a specific symbol with exact signatures and methods.
- `cratemd_file`: Read and analyze a single `.rs` file with line ranges and token savings.
- `cratemd_find`: Cross-search symbols across workspace members and external dependencies.
- `cratemd_features`: Inspect Cargo features and feature-gated code.
- `cratemd_impls`: Query trait implementations and reverse lookups.
- `cratemd_refs`: Find symbol references across workspace members.
- `cratemd_audit`: Audit dependencies for duplicate version splits and cache readiness.
- `cratemd_deps`: Inspect dependencies and offline readiness.
- `cratemd_workspace`: Inspect workspace architecture and hierarchy.
- `cratemd_tokens`: Measure token footprint and context impact.

#### Sample MCP Client Configuration:
```json
{
  "mcpServers": {
    "cratemd": {
      "command": "cratemd",
      "args": ["mcp"]
    }
  }
}
```

---

### 16. Module Outline, Ctags & Tree-sitter

```bash
# Hierarchical module tree
cratemd outline <crate_name> [--max-depth N]

# Generate Universal Ctags
cratemd ctags <crate_name> [--out tags]

# Tree-sitter AST syntax outline or S-expressions
cratemd treesitter <crate_name> [relative/path/to/file.rs] [--sexp]
```

---

### 17. Token Footprint & Context Protection

Measure context footprint and enforce strict token budgets to prevent LLM context exhaustion:

```bash
# Analyze context footprint and view token size across cheat sheet, outline, and docs
cratemd tokens <crate_name>

# Workspace context impact report
cratemd tokens /path/to/workspace

# Enforce a maximum token budget on any command (truncates safely at line boundaries)
cratemd <crate_name> --max-tokens 1500
cratemd search <crate_name> <query> --max-tokens 500

# Display estimated tokens appended to output
cratemd cheat <crate_name> --tokens
```

---

## Guidelines for LLM Agents

1. **Start with `cratemd workspace` or `cratemd deps`** when exploring a new repository or workspace to grasp project boundaries and library dependencies in seconds.
2. **For individual files, use `cratemd src/foo.rs`** to get an outline with exact line numbers before reading raw lines, saving up to 90% in tokens.
3. **Check context footprint with `cratemd tokens <crate>`** before dumping large documentation sets into your context.
4. **Prefer `cratemd cheat <crate>` (~500 tokens)** over full documentation dumps to preserve context space.
5. **Use `--max-tokens <N>`** whenever you need to ensure output stays within a strict budget.
6. **Use `cratemd find <query>`** to search for functionality across the entire project and dependencies simultaneously before writing duplicate code.
7. **Use `cratemd view <crate> <symbol>`** to ensure accurate method signatures and trait implementations before calling them in Rust.
8. **Add `--json`** whenever automated parsing is needed.
9. **Use `cratemd mcp`** when configuring agent tool environments for direct tool calling.
