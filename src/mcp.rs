use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use anyhow::Result;
use serde_json::{json, Value};

use crate::analyzer::CrateAnalyzer;
use crate::audit_cmd::DependencyAuditor;
use crate::cache::CacheManager;
use crate::cross_search::{CrossSearcher, FindArgs};
use crate::deps_cmd::DepsInspector;
use crate::docgen::DocGenerator;
use crate::features_cmd::FeaturesInspector;
use crate::file_cmd::FileAnalyzer;
use crate::impls_cmd::ImplsQuery;
use crate::locator::CrateLocator;
use crate::model::CrateIndex;
use crate::refs_cmd::WorkspaceRefsFinder;
use crate::search::{CrateSearcher, SearchQuery};
use crate::tokens;
use crate::workspace::WorkspaceInfo;

pub struct McpServer;

impl McpServer {
    pub fn run() -> Result<()> {
        let stdin = io::stdin();
        let mut stdout = io::stdout();
        let mut lines = stdin.lock().lines();

        while let Some(line_res) = lines.next() {
            let line = match line_res {
                Ok(l) => l,
                Err(_) => break,
            };

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Handle optional Content-Length header prefix
            let json_str = if trimmed.starts_with("Content-Length:") {
                // Read until empty line
                let mut content_len: usize = 0;
                if let Some(len_str) = trimmed.split(':').nth(1) {
                    content_len = len_str.trim().parse().unwrap_or(0);
                }
                while let Some(Ok(header_line)) = lines.next() {
                    if header_line.trim().is_empty() {
                        break;
                    }
                }
                let mut buf = vec![0u8; content_len];
                use std::io::Read;
                let _ = io::stdin().read_exact(&mut buf);
                String::from_utf8_lossy(&buf).to_string()
            } else {
                trimmed.to_string()
            };

            let req_val: Value = match serde_json::from_str(&json_str) {
                Ok(v) => v,
                Err(err) => {
                    eprintln!("cratemd mcp: parse error: {}", err);
                    continue;
                }
            };

            if let Some(resp) = handle_rpc_message(&req_val) {
                let resp_str = serde_json::to_string(&resp)?;
                writeln!(stdout, "{}", resp_str)?;
                stdout.flush()?;
            }
        }

        Ok(())
    }
}

fn handle_rpc_message(msg: &Value) -> Option<Value> {
    let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or_default();
    let id = msg.get("id").cloned();

    // Notifications (no id)
    id.as_ref()?;
    let id_val = id.unwrap();

    match method {
        "initialize" => Some(json!({
            "jsonrpc": "2.0",
            "id": id_val,
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {}
                },
                "serverInfo": {
                    "name": "cratemd",
                    "version": "0.1.0"
                }
            }
        })),

        "ping" => Some(json!({
            "jsonrpc": "2.0",
            "id": id_val,
            "result": {}
        })),

        "tools/list" => Some(json!({
            "jsonrpc": "2.0",
            "id": id_val,
            "result": {
                "tools": get_tool_definitions()
            }
        })),

        "tools/call" => {
            let params = msg.get("params").unwrap_or(&Value::Null);
            let tool_name = params.get("name").and_then(|n| n.as_str()).unwrap_or_default();
            let args = params.get("arguments").cloned().unwrap_or(json!({}));

            let (result_text, is_error) = execute_tool(tool_name, &args);

            Some(json!({
                "jsonrpc": "2.0",
                "id": id_val,
                "result": {
                    "content": [
                        {
                            "type": "text",
                            "text": result_text
                        }
                    ],
                    "isError": is_error
                }
            }))
        }

        _ => Some(json!({
            "jsonrpc": "2.0",
            "id": id_val,
            "error": {
                "code": -32601,
                "message": format!("Method not found: {}", method)
            }
        })),
    }
}

fn get_tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "cratemd_doc",
            "description": "Generate LLM-optimized single-document documentation for a Rust crate (e.g. 'axum', 'tokio', 'serde', or local path)",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "crate_name": {
                        "type": "string",
                        "description": "Crate name (from Cargo cache) or local directory path"
                    },
                    "full": {
                        "type": "boolean",
                        "description": "Generate full documentation with all internal fields and private methods (default false)"
                    }
                },
                "required": ["crate_name"]
            }
        }),
        json!({
            "name": "cratemd_cheat",
            "description": "Generate an ultra-condensed ~500-token cheat sheet of key types & functions for a Rust crate",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "crate_name": {
                        "type": "string",
                        "description": "Crate name or path"
                    }
                },
                "required": ["crate_name"]
            }
        }),
        json!({
            "name": "cratemd_search",
            "description": "Search for symbols, signatures, methods, or docstrings in a Rust crate",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "crate_name": {
                        "type": "string",
                        "description": "Crate name or path"
                    },
                    "query": {
                        "type": "string",
                        "description": "Search query (symbol name, keyword, or path)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max results to return (default 25)"
                    }
                },
                "required": ["crate_name", "query"]
            }
        }),
        json!({
            "name": "cratemd_view",
            "description": "Inspect a specific symbol (struct, enum, trait, function, method) in detail with signatures, docs, and examples",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "crate_name": {
                        "type": "string",
                        "description": "Crate name or path"
                    },
                    "symbol": {
                        "type": "string",
                        "description": "Exact or partial symbol name (e.g. 'Router', 'AudioProcessing::process_capture_i16')"
                    },
                    "include_body": {
                        "type": "boolean",
                        "description": "If true, extracts and includes the full source code implementation block (Lstart-Lend) of the symbol or method"
                    }
                },
                "required": ["crate_name", "symbol"]
            }
        }),
        json!({
            "name": "cratemd_file",
            "description": "Read and analyze a Rust source file (.rs) or directory of Rust files, extracting outlines, structs, enums, traits, functions, methods, and line numbers with token savings",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Relative or absolute path to the .rs file or directory"
                    },
                    "symbol": {
                        "type": "string",
                        "description": "Optional symbol or function name to inspect specifically within this file or directory"
                    },
                    "include_body": {
                        "type": "boolean",
                        "description": "Whether to include source code lines of the item (defaults to true when symbol is specified)"
                    }
                },
                "required": ["path"]
            }
        }),
        json!({
            "name": "cratemd_find",
            "description": "Cross-search symbols across workspace member crates and external dependencies",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query across workspace"
                    },
                    "path": {
                        "type": "string",
                        "description": "Workspace root directory (defaults to current directory)"
                    },
                    "kind": {
                        "type": "string",
                        "description": "Filter by symbol kind (fn, struct, enum, trait, method, type, macro, const)"
                    },
                    "returns": {
                        "type": "string",
                        "description": "Filter functions by return type (e.g. 'Result', 'Option', 'Stream')"
                    },
                    "takes": {
                        "type": "string",
                        "description": "Filter functions by parameter type (e.g. 'Context', 'Request', 'TcpStream')"
                    },
                    "workspace_only": {
                        "type": "boolean",
                        "description": "Search only within workspace member crates (exclude external dependencies)"
                    },
                    "deps_only": {
                        "type": "boolean",
                        "description": "Search only within external dependencies (exclude workspace members)"
                    },
                    "specific_crate": {
                        "type": "string",
                        "description": "Restrict search to a specific crate name"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max results to return (default 25)"
                    }
                },
                "required": ["query"]
            }
        }),
        json!({
            "name": "cratemd_features",
            "description": "Inspect Cargo feature flags, default features, dependencies, and feature-gated symbols in a crate",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "crate_name": {
                        "type": "string",
                        "description": "Crate name or path"
                    },
                    "feature": {
                        "type": "string",
                        "description": "Optional specific feature flag to inspect enabled symbols for"
                    }
                },
                "required": ["crate_name"]
            }
        }),
        json!({
            "name": "cratemd_impls",
            "description": "Query trait implementations and find implementors of traits or types in a crate",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "crate_name": {
                        "type": "string",
                        "description": "Crate name or path"
                    },
                    "query": {
                        "type": "string",
                        "description": "Type name or Trait name to query"
                    }
                },
                "required": ["crate_name"]
            }
        }),
        json!({
            "name": "cratemd_refs",
            "description": "Find all references and usages of a symbol across workspace member crates",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "symbol": {
                        "type": "string",
                        "description": "Symbol name or identifier to search for"
                    },
                    "path": {
                        "type": "string",
                        "description": "Workspace root directory (defaults to current directory)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max occurrences to return (default 50)"
                    }
                },
                "required": ["symbol"]
            }
        }),
        json!({
            "name": "cratemd_audit",
            "description": "Audit project dependencies for duplicate version splits and offline cache readiness",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "target": {
                        "type": "string",
                        "description": "Path to workspace/crate directory (defaults to current directory)"
                    }
                }
            }
        }),
        json!({
            "name": "cratemd_deps",
            "description": "List and inspect dependencies of the current project or workspace",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "target": {
                        "type": "string",
                        "description": "Path to crate or workspace root (defaults to current directory)"
                    }
                }
            }
        }),
        json!({
            "name": "cratemd_workspace",
            "description": "Inspect workspace architecture, member crates, and dependency relationships",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to workspace directory (defaults to current directory)"
                    }
                }
            }
        }),
        json!({
            "name": "cratemd_tokens",
            "description": "Analyze token footprint and context window impact of a crate or workspace",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "target": {
                        "type": "string",
                        "description": "Crate name, directory path, or workspace root (defaults to current directory)"
                    }
                }
            }
        }),
        json!({
            "name": "cratemd_outline",
            "description": "Hierarchical module outline and symbol tree for a crate",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "crate_name": {
                        "type": "string",
                        "description": "Crate name or path"
                    },
                    "max_depth": {
                        "type": "integer",
                        "description": "Maximum module depth (default 4)"
                    }
                },
                "required": ["crate_name"]
            }
        }),
        json!({
            "name": "cratemd_examples",
            "description": "Extract runnable code examples from documentation and examples/ directory for a crate",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "crate_name": {
                        "type": "string",
                        "description": "Crate name or path"
                    },
                    "query": {
                        "type": "string",
                        "description": "Optional keyword or title filter for examples"
                    }
                },
                "required": ["crate_name"]
            }
        }),
        json!({
            "name": "cratemd_list",
            "description": "List all Rust crates cached locally in the Cargo registry with versions and paths",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "filter": {
                        "type": "string",
                        "description": "Optional name filter for listing cached crates"
                    }
                }
            }
        }),
        json!({
            "name": "cratemd_locate",
            "description": "Locate a crate on the local system and inspect its root directory, manifest path, and metadata",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "crate_name": {
                        "type": "string",
                        "description": "Crate name or spec (e.g. 'tokio', 'serde@1.0.229', or path)"
                    }
                },
                "required": ["crate_name"]
            }
        }),
    ]
}

fn execute_tool(name: &str, args: &Value) -> (String, bool) {
    match name {
        "cratemd_doc" => {
            let crate_name = match args.get("crate_name").and_then(|c| c.as_str()) {
                Some(c) => c,
                None => return ("Missing 'crate_name' argument".to_string(), true),
            };
            let full = args.get("full").and_then(|f| f.as_bool()).unwrap_or(false);
            match load_index(crate_name) {
                Ok(index) => {
                    let doc = DocGenerator::generate_llm_doc(&index, full, 3);
                    (doc, false)
                }
                Err(err) => (format!("Error loading crate '{}': {:#}", crate_name, err), true),
            }
        }

        "cratemd_cheat" => {
            let crate_name = match args.get("crate_name").and_then(|c| c.as_str()) {
                Some(c) => c,
                None => return ("Missing 'crate_name' argument".to_string(), true),
            };
            match load_index(crate_name) {
                Ok(index) => (DocGenerator::generate_cheat_sheet(&index), false),
                Err(err) => (format!("Error loading crate '{}': {:#}", crate_name, err), true),
            }
        }

        "cratemd_search" => {
            let crate_name = match args.get("crate_name").and_then(|c| c.as_str()) {
                Some(c) => c,
                None => return ("Missing 'crate_name' argument".to_string(), true),
            };
            let query = args.get("query").and_then(|q| q.as_str()).unwrap_or_default();
            let limit = args.get("limit").and_then(|l| l.as_u64()).unwrap_or(25) as usize;

            match load_index(crate_name) {
                Ok(index) => {
                    let sq = SearchQuery {
                        text: query.to_string(),
                        kind_filter: None,
                        pub_only: true,
                        search_docs: false,
                        search_signatures: true,
                        returns_filter: None,
                        takes_filter: None,
                        limit,
                    };
                    let results = CrateSearcher::search(&index, &sq);
                    let mut out = format!("Search results for '{}' in {} ({} matches):\n\n", query, crate_name, results.len());
                    for (i, r) in results.iter().enumerate() {
                        out.push_str(&format!("{}. [{}] `{}`\n   Signature: {}\n   File: {}:{}\n\n",
                            i + 1, r.symbol.kind.as_str(), r.symbol.id, r.symbol.signature, r.symbol.file_path, r.symbol.line_start
                        ));
                    }
                    (out, false)
                }
                Err(err) => (format!("Error loading crate '{}': {:#}", crate_name, err), true),
            }
        }

        "cratemd_view" => {
            let crate_name = match args.get("crate_name").and_then(|c| c.as_str()) {
                Some(c) => c,
                None => return ("Missing 'crate_name' argument".to_string(), true),
            };
            let symbol_query = match args.get("symbol").and_then(|s| s.as_str()) {
                Some(s) => s,
                None => return ("Missing 'symbol' argument".to_string(), true),
            };
            let include_body = args.get("include_body").and_then(|b| b.as_bool()).unwrap_or(false);

            match load_index(crate_name) {
                Ok(index) => {
                    let target_suffix = format!("::{}", symbol_query);
                    let sym_opt = index.symbols.iter().find(|s| s.id == symbol_query || s.name == symbol_query)
                        .or_else(|| index.symbols.iter().find(|s| s.id.ends_with(&target_suffix)))
                        .or_else(|| {
                            index.symbols.iter().flat_map(|s| &s.methods).find(|m| {
                                m.id == symbol_query || m.name == symbol_query || m.id.ends_with(&target_suffix)
                            })
                        })
                        .or_else(|| {
                            index.symbols.iter().find(|s| s.name.eq_ignore_ascii_case(symbol_query) || s.id.ends_with(&format!("::{}", symbol_query)))
                        });

                    if let Some(sym) = sym_opt {
                        let text = DocGenerator::render_symbol_detail_with_source(sym, Some(&index.info.root_dir), include_body);
                        (text, false)
                    } else {
                        (format!("Symbol '{}' not found in crate '{}'.", symbol_query, crate_name), true)
                    }
                }
                Err(err) => (format!("Error loading crate '{}': {:#}", crate_name, err), true),
            }
        }

        "cratemd_file" => {
            let path_str = match args.get("path").and_then(|p| p.as_str()) {
                Some(p) => p,
                None => return ("Missing 'path' argument".to_string(), true),
            };
            let symbol_opt = args.get("symbol").and_then(|s| s.as_str());
            let include_body = args.get("include_body").and_then(|b| b.as_bool()).unwrap_or(true);

            let p = Path::new(path_str);
            match FileAnalyzer::analyze(p) {
                Ok(report) => {
                    if let Some(sym) = symbol_opt {
                        (report.render_symbol(sym, include_body), false)
                    } else {
                        (report.render_ascii(), false)
                    }
                }
                Err(err) => (format!("Error analyzing file '{}': {:#}", path_str, err), true),
            }
        }

        "cratemd_find" => {
            let query = match args.get("query").and_then(|q| q.as_str()) {
                Some(q) => q,
                None => return ("Missing 'query' argument".to_string(), true),
            };
            let path_opt = args.get("path").and_then(|p| p.as_str()).map(PathBuf::from);
            let limit = args.get("limit").and_then(|l| l.as_u64()).unwrap_or(25) as usize;
            let kind = args.get("kind").and_then(|k| k.as_str()).and_then(|s| match s {
                "fn" => Some(crate::cli::CliSymbolKind::Fn),
                "struct" => Some(crate::cli::CliSymbolKind::Struct),
                "enum" => Some(crate::cli::CliSymbolKind::Enum),
                "trait" => Some(crate::cli::CliSymbolKind::Trait),
                "method" => Some(crate::cli::CliSymbolKind::Method),
                "type" => Some(crate::cli::CliSymbolKind::Type),
                "const" => Some(crate::cli::CliSymbolKind::Const),
                "static" => Some(crate::cli::CliSymbolKind::Static),
                "macro" => Some(crate::cli::CliSymbolKind::Macro),
                "module" => Some(crate::cli::CliSymbolKind::Module),
                "impl" => Some(crate::cli::CliSymbolKind::Impl),
                _ => None,
            });
            let returns = args.get("returns").and_then(|r| r.as_str()).map(|s| s.to_string());
            let takes = args.get("takes").and_then(|t| t.as_str()).map(|s| s.to_string());
            let workspace_only = args.get("workspace_only").and_then(|w| w.as_bool()).unwrap_or(false);
            let deps_only = args.get("deps_only").and_then(|d| d.as_bool()).unwrap_or(false);
            let specific_crate = args.get("specific_crate").and_then(|c| c.as_str()).map(|s| s.to_string());

            let cache = CacheManager::new(true, false);

            let find_args = FindArgs {
                query: query.to_string(),
                kind,
                returns,
                takes,
                all: false,
                doc: false,
                limit,
                workspace_only,
                deps_only,
                path: path_opt,
                specific_crate,
            };

            match CrossSearcher::find(&find_args, &cache) {
                Ok(results) => {
                    let mut out = format!("Found {} results for '{}' across workspace and dependencies:\n\n", results.len(), query);
                    for (i, hit) in results.iter().enumerate() {
                        let sym = &hit.symbol;
                        let badge = if hit.is_workspace {
                            format!("[workspace: {}]", hit.origin_crate)
                        } else {
                            format!("[dep: {} v{}]", hit.origin_crate, hit.origin_version)
                        };
                        out.push_str(&format!("{}. {} [{}] `{}` ({}:{})\n   Signature: {}\n\n",
                            i + 1, badge, sym.kind.as_str(), sym.id, sym.file_path, sym.line_start, sym.signature
                        ));
                    }
                    (out, false)
                }
                Err(err) => (format!("Error executing find: {:#}", err), true),
            }
        }

        "cratemd_features" => {
            let crate_name = match args.get("crate_name").and_then(|c| c.as_str()) {
                Some(c) => c,
                None => return ("Missing 'crate_name' argument".to_string(), true),
            };
            let feat_opt = args.get("feature").and_then(|f| f.as_str());

            match load_index(crate_name) {
                Ok(index) => {
                    let report = FeaturesInspector::inspect(&index, feat_opt);
                    (report.render_ascii(), false)
                }
                Err(err) => (format!("Error loading crate '{}': {:#}", crate_name, err), true),
            }
        }

        "cratemd_impls" => {
            let crate_name = match args.get("crate_name").and_then(|c| c.as_str()) {
                Some(c) => c,
                None => return ("Missing 'crate_name' argument".to_string(), true),
            };
            let q_opt = args.get("query").and_then(|q| q.as_str());

            match load_index(crate_name) {
                Ok(index) => {
                    let report = ImplsQuery::query(&index, q_opt);
                    (report.render_ascii(), false)
                }
                Err(err) => (format!("Error loading crate '{}': {:#}", crate_name, err), true),
            }
        }

        "cratemd_refs" => {
            let symbol = match args.get("symbol").and_then(|s| s.as_str()) {
                Some(s) => s,
                None => return ("Missing 'symbol' argument".to_string(), true),
            };
            let path_opt = args.get("path").and_then(|p| p.as_str()).map(Path::new);
            let limit = args.get("limit").and_then(|l| l.as_u64()).unwrap_or(50) as usize;

            match WorkspaceRefsFinder::find(symbol, path_opt, limit) {
                Ok(report) => (report.render_ascii(), false),
                Err(err) => (format!("Error finding refs for '{}': {:#}", symbol, err), true),
            }
        }

        "cratemd_audit" => {
            let target_opt = args.get("target").and_then(|t| t.as_str()).map(Path::new);
            match DependencyAuditor::audit(target_opt) {
                Ok(report) => (report.render_ascii(), false),
                Err(err) => (format!("Error auditing dependencies: {:#}", err), true),
            }
        }

        "cratemd_deps" => {
            let target_opt = args.get("target").and_then(|t| t.as_str());
            match DepsInspector::inspect(target_opt) {
                Ok(report) => (DepsInspector::render_markdown(&report), false),
                Err(err) => (format!("Error inspecting dependencies: {:#}", err), true),
            }
        }

        "cratemd_workspace" => {
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let ws_path = args.get("path").and_then(|p| p.as_str()).map(PathBuf::from).unwrap_or(cwd);
            let ws_root = WorkspaceInfo::find_root(&ws_path).unwrap_or(ws_path);
            match WorkspaceInfo::load(&ws_root) {
                Ok(ws) => (ws.render_blueprint(), false),
                Err(err) => (format!("Error loading workspace: {:#}", err), true),
            }
        }

        "cratemd_tokens" => {
            let target = args.get("target").and_then(|t| t.as_str());
            if let Some(t) = target {
                match load_index(t) {
                    Ok(index) => (tokens::render_crate_context_impact(&index), false),
                    Err(err) => (format!("Error loading crate for token impact: {:#}", err), true),
                }
            } else {
                let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                if let Some(ws_root) = WorkspaceInfo::find_root(&cwd)
                    && let Ok(ws) = WorkspaceInfo::load(&ws_root) {
                        return (tokens::render_workspace_context_impact(&ws), false);
                    }
                match load_index(".") {
                    Ok(index) => (tokens::render_crate_context_impact(&index), false),
                    Err(err) => (format!("Error analyzing tokens: {:#}", err), true),
                }
            }
        }

        "cratemd_outline" => {
            let crate_name = match args.get("crate_name").and_then(|c| c.as_str()) {
                Some(c) => c,
                None => return ("Missing 'crate_name' argument".to_string(), true),
            };
            let max_depth = args.get("max_depth").and_then(|d| d.as_u64()).unwrap_or(4) as usize;

            match load_index(crate_name) {
                Ok(index) => {
                    let outline = DocGenerator::generate_outline(&index, max_depth);
                    (outline, false)
                }
                Err(err) => (format!("Error loading crate '{}': {:#}", crate_name, err), true),
            }
        }

        "cratemd_examples" => {
            let crate_name = match args.get("crate_name").and_then(|c| c.as_str()) {
                Some(c) => c,
                None => return ("Missing 'crate_name' argument".to_string(), true),
            };
            let query = args.get("query").and_then(|q| q.as_str());

            match load_index(crate_name) {
                Ok(index) => {
                    let examples = DocGenerator::render_examples(&index, query);
                    (examples, false)
                }
                Err(err) => (format!("Error loading crate '{}': {:#}", crate_name, err), true),
            }
        }

        "cratemd_list" => {
            let filter = args.get("filter").and_then(|f| f.as_str());
            match CrateLocator::list_cached(filter) {
                Ok(crates) => {
                    if crates.is_empty() {
                        ("No locally cached crates found.".to_string(), false)
                    } else {
                        use std::fmt::Write;
                        let mut out = format!("# Locally Cached Crates ({} found)\n\n| Crate | Version | Path |\n|---|---|---|\n", crates.len());
                        for (name, ver, path) in crates {
                            let _ = writeln!(out, "| `{}` | `{}` | `{}` |", name, ver, path.display());
                        }
                        (out, false)
                    }
                }
                Err(err) => (format!("Error listing cached crates: {:#}", err), true),
            }
        }

        "cratemd_locate" => {
            let crate_name = match args.get("crate_name").and_then(|c| c.as_str()) {
                Some(c) => c,
                None => return ("Missing 'crate_name' argument".to_string(), true),
            };
            match CrateLocator::locate(crate_name) {
                Ok(info) => {
                    use std::fmt::Write;
                    let mut out = format!("# Crate Location: `{}` v{}\n\n", info.name, info.version);
                    let _ = writeln!(out, "- **Root Directory:** `{}`", info.root_dir.display());
                    let _ = writeln!(out, "- **Manifest Path:** `{}`", info.manifest_path.display());
                    let _ = writeln!(out, "- **Edition:** {}", info.edition);
                    if let Some(ref desc) = info.description {
                        let _ = writeln!(out, "- **Description:** {}", desc);
                    }
                    if let Some(ref lib) = info.lib_path {
                        let _ = writeln!(out, "- **Library Entry:** `{}`", lib.display());
                    }
                    if !info.bin_paths.is_empty() {
                        let bins: Vec<String> = info.bin_paths.iter().map(|p| format!("`{}`", p.display())).collect();
                        let _ = writeln!(out, "- **Binary Entries:** {}", bins.join(", "));
                    }
                    let _ = writeln!(out, "- **Dependencies ({}):** `{}`", info.dependencies.len(), info.dependencies.join("`, `"));
                    (out, false)
                }
                Err(err) => (format!("Error locating crate '{}': {:#}", crate_name, err), true),
            }
        }

        _ => (format!("Unknown tool: {}", name), true),
    }
}

fn load_index(crate_spec: &str) -> Result<CrateIndex> {
    let crate_info = CrateLocator::locate(crate_spec)?;
    let cache = CacheManager::new(true, false);

    if let Some(cached) = cache.load(&crate_info.name, &crate_info.version, &crate_info.root_dir) {
        return Ok(cached);
    }

    let analyzer = CrateAnalyzer::new(crate_info);
    let index = analyzer.analyze()?;
    let _ = cache.store(&index);
    Ok(index)
}
