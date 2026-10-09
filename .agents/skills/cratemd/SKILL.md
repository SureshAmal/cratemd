---
name: cratemd
description: >-
  Use this skill whenever you need to explore, inspect, search, or understand any Rust crate (e.g. tokio, serde, axum, clap, regex, or any local dependency) or multi-crate Cargo workspace.
  Allows LLMs to understand crate architectures, public APIs, structs, traits, method signatures, project dependencies, call hierarchies, and persistent project memory instantly and offline without reading individual source files.
---

# cratemd: Offline Rust Crate & Workspace Intelligence

`cratemd` provides instant, 100% offline intelligence for any Rust crate cached in Cargo (`~/.cargo/registry/src`), git checkouts, local crates, or multi-crate workspaces.

Instead of burning context tokens reading massive `.rs` files or guessing API signatures, use `cratemd` to fetch clean, token-efficient outlines, search symbols, inspect definitions, and explore multi-crate workspaces in milliseconds.

---

## Invocation Modes

`cratemd` supports two primary execution modes:

1. **MCP Tools (Primary / Recommended)**: When running in an environment with the `cratemd` MCP server active (such as Antigravity / Gemini / Codex), invoke direct MCP tools (`cratemd_*`). MCP tools execute directly in-process with structured arguments and zero subshell overhead.
2. **CLI Binary (Fallback / Shell)**: In standard terminal shells, subagents without MCP access, or standalone scripts, run the `cratemd` binary directly via shell commands (`cratemd <subcommand>`).

---

## Token Conservation Playbook for AI Agents

To maximize context efficiency and prevent context window exhaustion, follow these rules:

1. **Use `cratemd_context` as the primary investigative tool**:
   - Instead of making separate calls to definition, hover, callers, and references, invoke `cratemd_context({ "symbol": "..." })`.
   - Returns a single consolidated, token-budgeted markdown card containing symbol definition, compact source snippet, callers/callees, and key usage references.
2. **Never read a raw `.rs` file with `read_file` or `cat` without first checking `cratemd_file`**:
   - `cratemd_file` extracts structs, enums, traits, functions, methods, doc summaries, and exact line ranges (`L14-L26`).
   - Query specific symbols (`cratemd_file({ "path": "...", "symbol": "foo" })`) and extract source definitions directly with `"include_body": true`.
   - Saves 70% to 90% in tokens compared to reading the full file.
3. **Persist and recall project knowledge with `.cratemd.db` memory tools**:
   - Call `cratemd_memory_get` and `cratemd_memory_search` before re-analyzing complex architectural flows or schemas.
   - When reaching a major design decision, saving API findings, or noting gotchas, call `cratemd_memory_set` so other agent threads can immediately leverage that knowledge.
4. **Start with `cratemd_cheat` instead of `cratemd_doc`**:
   - `cratemd_cheat` yields an ultra-condensed ~500-token summary of key types and functions.
   - Only call `cratemd_doc` when you genuinely require full API documentation for the entire crate.
5. **Use `cratemd_view` for surgical symbol lookups**:
   - When you need a specific method signature, field list, or doc example, query `cratemd_view` with the symbol name instead of grepping source files.
   - Pass `"include_body": true` (or CLI `--body`) to extract the complete function or struct source implementation directly.
6. **Use `cratemd_tokens` before querying large crates**:
   - Inspect the token footprint beforehand to verify how much context space the crate docs require.

---

## Tool Reference & Workflows

### 1. Consolidated Pipelined Context (Zero-Roundtrip Symbol Deep Dive)
Get complete symbol context (definition, code snippet, call hierarchy, and references) in a single tool call without multiple roundtrips.

- **MCP Tool**:
  ```json
  cratemd_context({ "symbol": "CrateLocator::locate" })
  cratemd_context({ "symbol": "WorkspaceInfo", "path": "/path/to/project" })
  ```
- **CLI Equivalent**:
  ```bash
  cratemd context CrateLocator::locate
  cratemd context WorkspaceInfo
  ```

---

### 2. Persistent Project Context & Memory (.cratemd.db)
Initialize an offline SQLite database with full-text search (FTS5) to store workspace architecture blueprints, crate cheat sheets, and persistent agent memory notes.

- **MCP Tools**:
  ```json
  cratemd_memory_get({ "key": "workspace:blueprint" })
  cratemd_memory_set({ "key": "auth:flow", "category": "architecture", "content": "OAuth2 PKCE flow implemented in router" })
  cratemd_memory_search({ "query": "PKCE flow", "limit": 5 })
  ```
- **CLI Equivalent**:
  ```bash
  # Initialize workspace blueprint & member crate summaries into .cratemd.db
  cratemd init
  cratemd init /path/to/project

  # Memory management
  cratemd memory list
  cratemd memory get workspace:blueprint
  cratemd memory set "auth:flow" --category "architecture" "OAuth2 PKCE flow implemented in router"
  cratemd memory search "PKCE flow"
  ```

---

### 3. File & Directory Source Analysis
Outline an individual `.rs` file or an entire directory of Rust files to see all types, functions, methods, docstrings, line ranges, and aggregate token savings (85-95%).

- **MCP Tool**:
  ```json
  cratemd_file({ "path": "src/analyzer.rs" })
  cratemd_file({ "path": "src" })
  cratemd_file({ "path": "src", "symbol": "FileAnalyzer" })
  cratemd_file({ "path": "src/analyzer.rs", "symbol": "parse_item", "include_body": true })
  ```
- **CLI Equivalent**:
  ```bash
  cratemd file src/analyzer.rs
  cratemd file src/
  cratemd file src FileAnalyzer
  cratemd file src/analyzer.rs parse_item --body
  # Or shorthand
  cratemd src/analyzer.rs
  ```

---

### 4. LSP-Grade Definition & Call Hierarchy
Jump directly to definitions or inspect call graphs across the codebase.

- **MCP Tools**:
  ```json
  cratemd_def({ "symbol": "CrateLocator", "snippet": true })
  cratemd_calls({ "function": "locate", "incoming": true })
  cratemd_hover({ "symbol": "CrateLocator" })
  ```
- **CLI Equivalent**:
  ```bash
  cratemd def CrateLocator -s
  cratemd calls locate -i
  cratemd hover CrateLocator
  ```

---

### 5. Multi-Crate Workspace Architecture & Visual Graphs
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
  cratemd workspace --mermaid
  ```

---

### 6. Project & Workspace Dependencies
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

### 7. Cross-Project Search
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

### 8. Ultra-Condensed Crate Cheat Sheet (~500 Tokens)
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

### 9. Symbol & Method Search in a Single Crate
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

### 10. Inspect Specific Symbol in Detail
View full declarations, trait implementations, methods, and documentation for a single item.

- **MCP Tool**:
  ```json
  cratemd_view({ "crate_name": "cratemd", "symbol": "CrateLocator::locate", "include_body": true })
  ```
- **CLI Equivalent**:
  ```bash
  cratemd view cratemd "CrateLocator::locate" --body
  ```

---

### 11. Trait Implementations Query
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

### 12. Workspace Cross-References
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

### 13. Cargo Features & Feature Gates
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

### 14. Dependency Health & Version Split Audit
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

### 15. Full Crate Documentation
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

### 16. Token Footprint & Context Impact
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

### 17. Hierarchical Module Outline Tree
Inspect a crate's module tree hierarchy with optional depth limiting.

- **MCP Tool**:
  ```json
  cratemd_outline({ "crate_name": "tokio", "max_depth": 2 })
  ```
- **CLI Equivalent**:
  ```bash
  cratemd outline tokio --max-depth 2
  ```

---

### 18. Extract Runnable Code Examples
Extract runnable code snippets from crate docstrings and the `examples/` directory.

- **MCP Tool**:
  ```json
  cratemd_examples({ "crate_name": "tokio", "query": "tcp" })
  ```
- **CLI Equivalent**:
  ```bash
  cratemd examples tokio tcp
  ```

---

### 19. List Locally Cached Crates & Locate Roots
Discover cached crates offline in Cargo's registry and locate filesystem paths.

- **MCP Tools**:
  ```json
  cratemd_list({ "filter": "tokio" })
  cratemd_locate({ "crate_name": "axum" })
  ```
- **CLI Equivalent**:
  ```bash
  cratemd list tokio
  cratemd locate axum
  ```

---

### 20. Additional CLI Utilities
Specialized indexing utilities:

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

When configuring `cratemd` in IDEs or agent clients:

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
