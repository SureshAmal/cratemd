---
name: cratemd
description: >-
  Use this skill whenever you need to explore, inspect, search, or understand any Rust crate (e.g. tokio, serde, axum, clap, regex, or any local dependency) or multi-crate Cargo workspace.
  Allows LLMs to understand crate architectures, public APIs, structs, traits, method signatures, and project dependencies instantly and offline without reading individual source files.
---

# cratemd: Offline Rust Crate & Workspace Intelligence

`cratemd` provides instant, 100% offline intelligence for any Rust crate cached in Cargo (`~/.cargo/registry/src`), git checkouts, local crates, or multi-crate workspaces.

Instead of burning context tokens reading massive `.rs` files or guessing API signatures, use `cratemd` to fetch clean, token-efficient outlines, search symbols, inspect definitions, and explore multi-crate workspaces in milliseconds.

---

## Invocation Modes

`cratemd` supports two primary execution modes:

1. **MCP Tools (Primary / Recommended)**: When running in an environment with the `cratemd` MCP server active (such as Antigravity / Gemini), invoke the direct MCP tools (`cratemd_*`). MCP tools execute directly in-process with structured arguments and zero subshell overhead.
2. **CLI Binary (Fallback / Shell)**: In standard terminal shells, subagents without MCP access, or standalone scripts, run the `cratemd` binary directly via shell commands (`cratemd <subcommand>`).

---

## Token Conservation Playbook for AI Agents

To maximize context efficiency and prevent context window exhaustion, follow these rules:

1. **Never read a raw `.rs` file with `read_file` or `cat` without first checking `cratemd_file`**:
   - `cratemd_file` extracts structs, enums, traits, functions, methods, doc summaries, and exact line ranges (`L14-L26`).
   - Query specific symbols (`cratemd_file({ "path": "...", "symbol": "foo" })`) and extract source definitions directly with `"include_body": true`.
   - Saves 70% to 90% in tokens compared to reading the full file.
2. **Start with `cratemd_cheat` instead of `cratemd_doc`**:
   - `cratemd_cheat` yields an ultra-condensed ~500-token summary of key types and functions.
   - Only call `cratemd_doc` when you genuinely require full API documentation for the entire crate.
3. **Use `cratemd_view` for surgical symbol lookups**:
   - When you need a specific method signature, field list, or doc example, query `cratemd_view` with the symbol name instead of grepping source files.
   - Pass `"include_body": true` (or CLI `--body`) to extract the complete function or struct source implementation directly.
4. **Use `cratemd_impls` to see trait implementations**:
   - Quickly find what traits a struct implements (e.g. `Serialize`, `Stream`, `Service`) or what types implement a given trait.
5. **Use `cratemd_tokens` before querying large crates**:
   - Inspect the token footprint beforehand to verify how much context space the crate docs require.

---

## Tool Reference & Workflows

### 1. Single File Read & Analysis
Outline an individual `.rs` file to see all types, functions, methods, docstrings, and line ranges. Optionally query a specific symbol and extract its full source body directly.

- **MCP Tool**:
  ```json
  cratemd_file({ "path": "src/analyzer.rs" })
  cratemd_file({ "path": "src/analyzer.rs", "symbol": "parse_item" })
  cratemd_file({ "path": "src/analyzer.rs", "symbol": "parse_item", "include_body": true })
  ```
- **CLI Equivalent**:
  ```bash
  cratemd file src/analyzer.rs
  cratemd file src/analyzer.rs parse_item
  cratemd file src/analyzer.rs parse_item --body
  # Or shorthand
  cratemd src/analyzer.rs
  ```

---

### 2. Multi-Crate Workspace Architecture
Inspect member crates, intra-workspace dependencies, and crate hierarchy graphs.

- **MCP Tool**:
  ```json
  cratemd_workspace({})
  cratemd_workspace({ "path": "/path/to/project" })
  ```
- **CLI Equivalent**:
  ```bash
  cratemd workspace
  cratemd workspace /path/to/project
  ```

---

### 3. Project & Workspace Dependencies
List resolved dependencies from `Cargo.lock` with versions, usage counts, and offline cache status.

- **MCP Tool**:
  ```json
  cratemd_deps({})
  cratemd_deps({ "target": "/path/to/project" })
  ```
- **CLI Equivalent**:
  ```bash
  cratemd deps
  cratemd deps /path/to/project
  ```

---

### 4. Cross-Project Search
Search symbols simultaneously across local workspace crates and all external dependencies with kind and signature filters.

- **MCP Tool**:
  ```json
  cratemd_find({ "query": "process_frame", "limit": 20 })
  cratemd_find({ "query": "parse", "kind": "fn", "workspace_only": true })
  cratemd_find({ "returns": "Result", "takes": "TcpStream" })
  cratemd_find({ "query": "Config", "specific_crate": "tokio" })
  ```
- **CLI Equivalent**:
  ```bash
  cratemd find process_frame
  cratemd find process_frame -w        # workspace only
  cratemd find process_frame -d        # dependencies only
  cratemd find --returns Result        # filter by return type
  cratemd find --takes TcpStream       # filter by parameter type
  cratemd find --kind fn               # filter by symbol kind
  ```

---

### 5. Ultra-Condensed Crate Cheat Sheet (~500 Tokens)
Get a compact summary of core structs, enums, traits, and functions for any crate.

- **MCP Tool**:
  ```json
  cratemd_cheat({ "crate_name": "tokio" })
  ```
- **CLI Equivalent**:
  ```bash
  cratemd cheat tokio
  ```

---

### 6. Symbol & Method Search in a Single Crate
Search for symbols, methods, or docstrings within a specific crate.

- **MCP Tool**:
  ```json
  cratemd_search({ "crate_name": "axum", "query": "Router" })
  ```
- **CLI Equivalent**:
  ```bash
  cratemd search axum Router
  cratemd search axum Router --kind struct
  cratemd search axum --returns Result
  cratemd search axum --takes Request
  cratemd search axum Router --doc
  ```

---

### 7. Inspect Specific Symbol in Detail
View full declarations, trait implementations, methods, and documentation for a single item. Optionally extract the entire source body implementation.

- **MCP Tool**:
  ```json
  cratemd_view({ "crate_name": "sonora", "symbol": "AudioProcessing::process_capture_i16" })
  cratemd_view({ "crate_name": "sonora", "symbol": "AudioProcessing::process_capture_i16", "include_body": true })
  ```
- **CLI Equivalent**:
  ```bash
  cratemd view sonora "AudioProcessing::process_capture_i16"
  cratemd view sonora "AudioProcessing::process_capture_i16" --body
  ```

---

### 8. Trait Implementations Query
Inspect all trait implementations in a crate or find implementors of a specific trait or type.

- **MCP Tool**:
  ```json
  cratemd_impls({ "crate_name": "serde", "query": "Serialize" })
  ```
- **CLI Equivalent**:
  ```bash
  cratemd impls serde
  cratemd impls serde Serialize
  ```

---

### 9. Workspace Cross-References
Locate all call sites, imports, and usages of a symbol across all member crates.

- **MCP Tool**:
  ```json
  cratemd_refs({ "symbol": "parse_config", "limit": 50 })
  ```
- **CLI Equivalent**:
  ```bash
  cratemd refs parse_config
  cratemd refs parse_config --path /path/to/workspace
  ```

---

### 10. Cargo Features & Feature Gates
Inspect crate feature flags, default feature sets, and feature-gated symbols.

- **MCP Tool**:
  ```json
  cratemd_features({ "crate_name": "tokio" })
  cratemd_features({ "crate_name": "tokio", "feature": "full" })
  ```
- **CLI Equivalent**:
  ```bash
  cratemd features tokio
  cratemd features tokio --feature full
  ```

---

### 11. Dependency Health & Version Split Audit
Audit project dependencies for duplicate version splits and verify local cache readiness.

- **MCP Tool**:
  ```json
  cratemd_audit({})
  cratemd_audit({ "target": "/path/to/project" })
  ```
- **CLI Equivalent**:
  ```bash
  cratemd audit
  ```

---

### 12. Full Crate Documentation
Generate single-document documentation for a crate (summarized or full).

- **MCP Tool**:
  ```json
  cratemd_doc({ "crate_name": "clap", "full": false })
  ```
- **CLI Equivalent**:
  ```bash
  cratemd doc clap
  cratemd doc clap --full --out /path/to/clap_docs.md
  ```

---

### 13. Token Footprint & Context Impact
Measure token counts across cheat sheets, outlines, and documentation before querying.

- **MCP Tool**:
  ```json
  cratemd_tokens({ "target": "tokio" })
  ```
- **CLI Equivalent**:
  ```bash
  cratemd tokens tokio
  cratemd tokens /path/to/workspace
  cratemd cheat tokio --max-tokens 500
  ```

---

### 14. Hierarchical Module Outline Tree
Inspect a crate's module tree hierarchy with optional depth limiting.

- **MCP Tool**:
  ```json
  cratemd_outline({ "crate_name": "tokio" })
  cratemd_outline({ "crate_name": "tokio", "max_depth": 2 })
  ```
- **CLI Equivalent**:
  ```bash
  cratemd outline tokio
  cratemd outline tokio --max-depth 2
  ```

---

### 15. Extract Runnable Code Examples
Extract runnable code snippets from crate docstrings and the `examples/` directory.

- **MCP Tool**:
  ```json
  cratemd_examples({ "crate_name": "tokio" })
  cratemd_examples({ "crate_name": "tokio", "filter": "tcp" })
  ```
- **CLI Equivalent**:
  ```bash
  cratemd examples tokio
  cratemd examples tokio tcp
  ```

---

### 16. Additional CLI Utilities
The CLI supports specialized indexing utilities:

```bash
# Universal Ctags generation
cratemd ctags <crate_name> [--out tags]

# Tree-sitter AST syntax outline or S-expressions
cratemd treesitter <crate_name> [relative/path/to/file.rs] [--sexp]

# Pre-warm local cache for zero-latency queries
cratemd warm --all
```

---

## MCP Server Configuration

When configuring `cratemd` in other tools or IDEs, add the following to `mcp_config.json`:

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
