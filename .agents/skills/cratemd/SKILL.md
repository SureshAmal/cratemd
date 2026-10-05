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

### 9. Module Outline, Ctags & Tree-sitter

```bash
# Hierarchical module tree
cratemd outline <crate_name> [--max-depth N]

# Generate Universal Ctags
cratemd ctags <crate_name> [--out tags]

# Tree-sitter AST syntax outline or S-expressions
cratemd treesitter <crate_name> [relative/path/to/file.rs] [--sexp]
```

---

### 10. Token Footprint & Context Protection

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
2. **Check context footprint with `cratemd tokens <crate>`** before dumping large documentation sets into your context.
3. **Prefer `cratemd cheat <crate>` (~500 tokens)** over full documentation dumps to preserve context space.
4. **Use `--max-tokens <N>`** whenever you need to ensure output stays within a strict budget.
5. **Use `cratemd find <query>`** to search for functionality across the entire project and dependencies simultaneously before writing duplicate code.
6. **Use `cratemd view <crate> <symbol>`** to ensure accurate method signatures and trait implementations before calling them in Rust.
7. **Add `--json`** whenever automated parsing is needed.
