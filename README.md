# cratemd

> **Instant 100% offline Rust crate intelligence for LLMs and developers.**
> Understand any Rust crate or multi-crate Cargo workspace in seconds without reading files one-by-one or burning context window tokens.

`cratemd` allows an LLM (or human developer) to pass the name of a crate or point to a local multi-crate workspace. It automatically locates the crate in your local Cargo cache (`~/.cargo/registry/src`), parses its AST (via `syn` and `tree-sitter`), extracts all structs, enums, traits, functions, methods, re-exports, trait implementations, and docstrings, and generates:
- **Multi-Crate Workspace Architecture Blueprints** (`cratemd workspace`)
- **Unified Cross-Crate & Dependency Search** (`cratemd find <query>`)
- **Dependency Inventory & Offline Status** (`cratemd deps`)
- **Ultra-Condensed ~500-Token Cheat Sheets** (`cratemd cheat <crate>`)
- **Extracted Runnable Code Examples** (`cratemd examples <crate>`)
- **LLM-optimized single-document markdown API summaries** (`llms.txt` style)
- **Universal Ctags** (`tags` file format)
- **Tree-sitter AST outlines**
- **Sub-2ms Persistent Disk Caching** (`~/.cache/cratemd`)

**Zero network requests. 100% local and offline.**

---

## Installation

```bash
cargo build --release
```

Binary is built at `./target/release/cratemd`.

---

## Quick Start for LLMs & Developers

### 1. Multi-Crate Workspace Architecture
Get the blueprint of an entire workspace and inter-crate dependency relationships:
```bash
cratemd workspace
cratemd workspace /path/to/project
```

### 2. Unified Search Across Workspace & Dependencies
Search for functions, methods, or structs across local workspace crates and external dependencies at once:
```bash
# Search across workspace members and external dependencies
cratemd find <query>

# Search only workspace crates
cratemd find <query> -w

# Filter functions by return type
cratemd find --returns Result

# Filter functions by argument type
cratemd find --takes TcpStream
```

### 3. Dependency Inventory
Check all external dependencies, resolved versions from Cargo.lock, and offline availability:
```bash
cratemd deps
cratemd deps /path/to/project
```

### 4. Ultra-Condensed Cheat Sheet (~500 tokens)
Get an immediate high-density summary of key structs, enums, traits, and functions:
```bash
cratemd cheat serde
cratemd cheat tokio
```

### 5. Extract Runnable Code Examples
Extract code snippets from doc comments and `examples/` directory:
```bash
cratemd examples clap
cratemd examples tokio spawn
```

### 6. Single-Crate Overview & Full Docs
```bash
cratemd serde
cratemd doc serde --out serde.md
```

### 7. Fast Symbol & Method Search
```bash
cratemd search serde Serialize
cratemd search clap_builder Command --kind struct
cratemd search tokio "spawn" --doc
cratemd search serde Serializer --json
```

### 8. Inspect a Specific Symbol in Detail
```bash
cratemd view serde Serializer
cratemd view clap_builder arg
```

### 9. Hierarchical Module Outline
```bash
cratemd outline serde
```

### 10. Universal Ctags & Tree-sitter AST Outline
```bash
cratemd ctags serde --out tags
cratemd treesitter serde
cratemd treesitter serde --sexp
```

### 11. Locate & Explore Local Crates
```bash
cratemd locate serde
cratemd list [filter]
```

### 12. Token Footprint Analysis & Budget Control
Protect your LLM context window from being flooded by large documentation dumps:
```bash
# Analyze context footprint across views (% of 128k context)
cratemd tokens tokio
cratemd tokens /path/to/workspace

# Enforce strict token limit (truncates gracefully at line boundaries)
cratemd tokio --max-tokens 1500
cratemd search tokio TcpStream --max-tokens 400

# Append estimated token count to output
cratemd cheat tokio --tokens
```
