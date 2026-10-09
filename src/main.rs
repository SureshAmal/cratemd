use cratemd::{
    analyzer, audit_cmd, cache, calls_cmd, cli, context_cmd, cross_search, ctags_gen,
    db, def_cmd, deps_cmd, docgen, features_cmd, file_cmd, hover_cmd, impls_cmd,
    locator, mcp, model, refs_cmd, search, tokens, treesitter_gen, warm_cmd, workspace,
};

use std::fs;
use std::path::{Path, PathBuf};
use anyhow::{bail, Context, Result};
use clap::Parser;

use analyzer::{clean_rust_syntax, CrateAnalyzer};
use audit_cmd::DependencyAuditor;
use cache::CacheManager;
use calls_cmd::CallsFinder;
use cli::{
    AuditArgs, CallsArgs, CheatArgs, Cli, Commands, ContextArgs, CtagsArgs, DefArgs, DepsCliArgs, DocArgs,
    ExamplesArgs, FeaturesArgs, FileArgs, FindCliArgs, HoverArgs, ImplsArgs, InitArgs, ListArgs, LocateArgs,
    McpArgs, MemoryAction, MemoryArgs, OutlineArgs, RefsArgs, SearchArgs, TokensCliArgs, TreesitterArgs,
    ViewArgs, WarmArgs, WorkspaceCliArgs,
};
use context_cmd::PipelinedContextFinder;
use db::ProjectDb;
use cross_search::{CrossSearcher, FindArgs};
use ctags_gen::CtagsGenerator;
use def_cmd::DefFinder;
use deps_cmd::DepsInspector;
use docgen::DocGenerator;
use features_cmd::FeaturesInspector;
use file_cmd::FileAnalyzer;
use hover_cmd::HoverInspector;
use impls_cmd::ImplsQuery;
use locator::CrateLocator;
use mcp::McpServer;
use model::CrateIndex;
use refs_cmd::WorkspaceRefsFinder;
use search::{CrateSearcher, SearchQuery};
use treesitter_gen::TreeSitterGen;
use warm_cmd::CacheWarmer;
use workspace::WorkspaceInfo;

fn main() {
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }

    if let Err(err) = run() {
        if let Some(io_err) = err.downcast_ref::<std::io::Error>()
            && io_err.kind() == std::io::ErrorKind::BrokenPipe {
                return;
            }
        eprintln!("cratemd: {:#}", err);
        std::process::exit(1);
    }
}

fn print_output(text: String, cli: &Cli) {
    let mut final_text = tokens::apply_budget(text, cli.max_tokens);
    if cli.tokens {
        let est = tokens::estimate_tokens(&final_text);
        if !final_text.ends_with('\n') {
            final_text.push('\n');
        }
        final_text.push_str(&format!("[Tokens: ~{}]\n", est));
    }
    print!("{}", final_text);
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let refresh = cli.refresh;
    let no_cache = cli.no_cache;

    let command = cli.command.clone();
    match command {
        Some(Commands::Doc(args)) => handle_doc(args, &cli, refresh, no_cache),
        Some(Commands::Search(args)) => handle_search(args, &cli, refresh, no_cache),
        Some(Commands::Find(args)) => handle_find(args, &cli, refresh, no_cache),
        Some(Commands::Deps(args)) => handle_deps(args, &cli),
        Some(Commands::Workspace(args)) => handle_workspace(args, &cli),
        Some(Commands::Tokens(args)) => handle_tokens(args, &cli, refresh, no_cache),
        Some(Commands::Cheat(args)) => handle_cheat(args, &cli, refresh, no_cache),
        Some(Commands::Examples(args)) => handle_examples(args, &cli, refresh, no_cache),
        Some(Commands::Outline(args)) => handle_outline(args, &cli, refresh, no_cache),
        Some(Commands::View(args)) => handle_view(args, &cli, refresh, no_cache),
        Some(Commands::Ctags(args)) => handle_ctags(args, &cli, refresh, no_cache),
        Some(Commands::Treesitter(args)) => handle_treesitter(args, &cli),
        Some(Commands::Locate(args)) => handle_locate(args, &cli),
        Some(Commands::List(args)) => handle_list(args, &cli),
        Some(Commands::Features(args)) => handle_features(args, &cli, refresh, no_cache),
        Some(Commands::Impls(args)) => handle_impls(args, &cli, refresh, no_cache),
        Some(Commands::Refs(args)) => handle_refs(args, &cli),
        Some(Commands::Def(args)) => handle_def(args, &cli),
        Some(Commands::Calls(args)) => handle_calls(args, &cli),
        Some(Commands::Hover(args)) => handle_hover(args, &cli),
        Some(Commands::Context(args)) => handle_context(args, &cli),
        Some(Commands::Audit(args)) => handle_audit(args, &cli),
        Some(Commands::Warm(args)) => handle_warm(args, &cli, refresh),
        Some(Commands::File(args)) => handle_file(args, &cli),
        Some(Commands::Init(args)) => handle_init(args, &cli),
        Some(Commands::Memory(args)) => handle_memory(args, &cli),
        Some(Commands::Mcp(args)) => handle_mcp(args),
        None => {
            if let Some(crate_name) = cli.crate_name.clone() {
                // If target is a single .rs file, analyze that file
                let file_path = Path::new(&crate_name);
                if (file_path.is_file() && file_path.extension().is_some_and(|e| e == "rs")) || crate_name.ends_with(".rs") {
                    return handle_file(FileArgs { path: file_path.to_path_buf(), symbol: None, body: false }, &cli);
                }

                // If the target is a workspace root, show workspace blueprint
                let target_path = Path::new(&crate_name);
                if target_path.exists()
                    && let Some(ws_root) = WorkspaceInfo::find_root(target_path)
                        && let Ok(ws) = WorkspaceInfo::load(&ws_root)
                            && ws.members.len() > 1 {
                                return handle_workspace(WorkspaceCliArgs { path: Some(ws_root), mermaid: false }, &cli);
                            }

                // Default action when just crate name is provided: generate LLM doc
                handle_doc(
                    DocArgs {
                        crate_name,
                        full: false,
                        max_depth: 3,
                        out: None,
                    },
                    &cli,
                    refresh,
                    no_cache,
                )
            } else {
                // Check if current directory is a workspace or crate
                let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                if let Some(ws_root) = WorkspaceInfo::find_root(&cwd) {
                    if let Ok(ws) = WorkspaceInfo::load(&ws_root) {
                        if ws.members.len() > 1 {
                            return handle_workspace(WorkspaceCliArgs { path: Some(ws_root), mermaid: false }, &cli);
                        } else if let Some(single) = ws.members.first() {
                            return handle_cheat(CheatArgs { crate_name: single.abs_path.to_string_lossy().to_string() }, &cli, refresh, no_cache);
                        }
                    }
                } else if cwd.join("Cargo.toml").exists() {
                    return handle_cheat(CheatArgs { crate_name: ".".to_string() }, &cli, refresh, no_cache);
                }

                // Otherwise print help
                use clap::CommandFactory;
                let _ = Cli::command().print_help();
                println!();
                Ok(())
            }
        }
    }
}

fn load_and_index(crate_spec: &str, refresh: bool, no_cache: bool) -> Result<CrateIndex> {
    let crate_info = CrateLocator::locate(crate_spec)?;
    let cache = CacheManager::new(!no_cache, refresh);

    if let Some(cached_index) = cache.load(&crate_info.name, &crate_info.version, &crate_info.root_dir) {
        return Ok(cached_index);
    }

    let analyzer = CrateAnalyzer::new(crate_info);
    let index = analyzer.analyze()?;
    let _ = cache.store(&index);
    Ok(index)
}

fn handle_doc(args: DocArgs, cli: &Cli, refresh: bool, no_cache: bool) -> Result<()> {
    let index = load_and_index(&args.crate_name, refresh, no_cache)?;

    if cli.json {
        print_output(serde_json::to_string_pretty(&index)?, cli);
        return Ok(());
    }

    let markdown = DocGenerator::generate_llm_doc(&index, args.full, args.max_depth);

    if let Some(out_path) = args.out {
        fs::write(&out_path, &markdown)
            .with_context(|| format!("Failed to write doc to {}", out_path.display()))?;
        eprintln!("Generated documentation written to {}", out_path.display());
    } else {
        print_output(markdown, cli);
    }

    Ok(())
}

fn handle_cheat(args: CheatArgs, cli: &Cli, refresh: bool, no_cache: bool) -> Result<()> {
    let index = load_and_index(&args.crate_name, refresh, no_cache)?;

    if cli.json {
        print_output(serde_json::to_string_pretty(&index)?, cli);
        return Ok(());
    }

    let cheat = DocGenerator::generate_cheat_sheet(&index);
    print_output(cheat, cli);
    Ok(())
}

fn handle_examples(args: ExamplesArgs, cli: &Cli, refresh: bool, no_cache: bool) -> Result<()> {
    let index = load_and_index(&args.crate_name, refresh, no_cache)?;

    if cli.json {
        let examples: Vec<_> = index.standalone_examples.iter()
            .chain(index.symbols.iter().flat_map(|s| &s.examples))
            .collect();
        print_output(serde_json::to_string_pretty(&examples)?, cli);
        return Ok(());
    }

    let examples = DocGenerator::render_examples(&index, args.query.as_deref());
    print_output(examples, cli);
    Ok(())
}

fn handle_search(args: SearchArgs, cli: &Cli, refresh: bool, no_cache: bool) -> Result<()> {
    use std::fmt::Write;
    let index = load_and_index(&args.crate_name, refresh, no_cache)?;

    let query = SearchQuery {
        text: args.query.clone(),
        kind_filter: args.kind.map(|k| k.to_model_kind()),
        pub_only: !args.all,
        search_docs: args.doc,
        search_signatures: true,
        returns_filter: args.returns,
        takes_filter: args.takes,
        limit: args.limit,
    };

    let hits = CrateSearcher::search(&index, &query);

    if cli.json {
        print_output(serde_json::to_string_pretty(&hits)?, cli);
        return Ok(());
    }

    if hits.is_empty() {
        print_output(format!("No symbols found matching query in crate '{}'.\n", args.crate_name), cli);
        return Ok(());
    }

    let mut out = String::new();
    let query_desc = if args.query.is_empty() {
        "filters".to_string()
    } else {
        format!("'{}'", args.query)
    };
    let _ = writeln!(out, "Found {} results for {} in {} v{}:\n", hits.len(), query_desc, index.info.name, index.info.version);

    for (i, hit) in hits.iter().enumerate() {
        let sym = &hit.symbol;
        let kind_badge = format!("[{}]", sym.kind.as_str());
        let vis_str = if sym.visibility.is_public() { "" } else { " (internal)" };
        let feat_badge = sym.feature.as_deref().map(|f| format!(" [feature: {}]", f)).unwrap_or_default();
        let clean_sig = clean_rust_syntax(&sym.signature);

        let _ = writeln!(out, "{}. {} `{}`{}{} ({}:{})", i + 1, kind_badge, sym.id, feat_badge, vis_str, sym.file_path, sym.line_start);
        let _ = writeln!(out, "   {}", clean_sig);

        if !sym.trait_impls.is_empty() {
            let _ = writeln!(out, "   Implements: {}", sym.trait_impls.join(", "));
        }
        if !sym.methods.is_empty() {
            let total = sym.methods.len();
            let preview: Vec<&str> = sym.methods.iter().take(8).map(|m| m.name.as_str()).collect();
            let more = if total > 8 {
                format!(", ... ({} total)", total)
            } else {
                String::new()
            };
            let _ = writeln!(out, "   Methods: {}{}", preview.join(", "), more);
        }
        if !sym.doc.is_empty() {
            let first_line = sym.doc.lines().next().unwrap_or("").trim();
            if !first_line.is_empty() {
                let _ = writeln!(out, "   {}", first_line);
            }
        }
        let _ = writeln!(out);
    }

    print_output(out, cli);
    Ok(())
}

fn handle_outline(args: OutlineArgs, cli: &Cli, refresh: bool, no_cache: bool) -> Result<()> {
    let index = load_and_index(&args.crate_name, refresh, no_cache)?;

    if cli.json {
        print_output(serde_json::to_string_pretty(&index.root_module)?, cli);
        return Ok(());
    }

    let outline = DocGenerator::generate_outline(&index, args.max_depth);
    print_output(outline, cli);
    Ok(())
}

fn handle_view(args: ViewArgs, cli: &Cli, refresh: bool, no_cache: bool) -> Result<()> {
    let index = load_and_index(&args.crate_name, refresh, no_cache)?;

    let raw_target = args.symbol.trim();
    let target = raw_target.trim_end_matches("()").replace('.', "::");
    let target_suffix = format!("::{}", target);
    let target_lower = target.to_lowercase();
    let suffix_lower = format!("::{}", target_lower);

    // Find symbol with smart resolution:
    // 1. Exact id match on any symbol (including methods in index.symbols)
    let sym = index.symbols.iter().find(|s| s.id == target)
        // 2. Exact name match
        .or_else(|| index.symbols.iter().find(|s| s.name == target))
        // 3. Suffix match on id (e.g. "AudioProcessing::process_capture_i16" or "audio_processing::AudioProcessing")
        .or_else(|| index.symbols.iter().find(|s| s.id.ends_with(&target_suffix)))
        // 4. Case-insensitive name match
        .or_else(|| index.symbols.iter().find(|s| s.name.eq_ignore_ascii_case(&target)))
        // 5. Case-insensitive suffix match on id
        .or_else(|| index.symbols.iter().find(|s| s.id.to_lowercase().ends_with(&suffix_lower)))
        // 6. Check methods attached to parent structs/enums
        .or_else(|| {
            index.symbols.iter().flat_map(|s| &s.methods).find(|m| {
                m.id == target
                    || m.name == target
                    || m.id.ends_with(&target_suffix)
                    || m.name.eq_ignore_ascii_case(&target)
                    || m.id.to_lowercase().ends_with(&suffix_lower)
            })
        })
        // 7. Substring / contains fallback
        .or_else(|| {
            index.symbols.iter().find(|s| s.id.to_lowercase().contains(&target_lower))
        });

    let Some(sym) = sym else {
        bail!("Symbol '{}' not found in crate '{}'. Tip: Use `cratemd search {} {}` to search.",
            args.symbol, args.crate_name, args.crate_name, args.symbol);
    };

    if cli.json {
        print_output(serde_json::to_string_pretty(sym)?, cli);
        return Ok(());
    }

    let detail = DocGenerator::render_symbol_detail_with_source(sym, Some(&index.info.root_dir), args.body);
    print_output(detail, cli);
    Ok(())
}

fn handle_ctags(args: CtagsArgs, cli: &Cli, refresh: bool, no_cache: bool) -> Result<()> {
    let index = load_and_index(&args.crate_name, refresh, no_cache)?;

    if cli.json {
        print_output(serde_json::to_string_pretty(&index.symbols)?, cli);
        return Ok(());
    }

    let tags = CtagsGenerator::generate(&index);

    if let Some(out_path) = args.out {
        fs::write(&out_path, &tags)
            .with_context(|| format!("Failed to write tags to {}", out_path.display()))?;
        eprintln!("Ctags written to {}", out_path.display());
    } else {
        print_output(tags, cli);
    }

    Ok(())
}

fn handle_treesitter(args: TreesitterArgs, cli: &Cli) -> Result<()> {
    let crate_info = CrateLocator::locate(&args.crate_name)?;

    let target_file = if let Some(rel) = args.file {
        crate_info.root_dir.join(rel)
    } else if let Some(ref lib) = crate_info.lib_path {
        lib.clone()
    } else if let Some(first_bin) = crate_info.bin_paths.first() {
        first_bin.clone()
    } else {
        bail!("No source entry file found in crate '{}'", args.crate_name);
    };

    if !target_file.exists() {
        bail!("File '{}' does not exist in crate", target_file.display());
    }

    if args.sexp {
        let sexp = TreeSitterGen::sexp_file(&target_file)?;
        print_output(format!("{}\n", sexp), cli);
    } else {
        let outline = TreeSitterGen::outline_file(&target_file)?;
        print_output(outline, cli);
    }

    Ok(())
}

fn handle_locate(args: LocateArgs, cli: &Cli) -> Result<()> {
    use std::fmt::Write;
    let crate_info = CrateLocator::locate(&args.crate_name)?;

    if cli.json {
        print_output(serde_json::to_string_pretty(&crate_info)?, cli);
        return Ok(());
    }

    let mut out = String::new();
    let _ = writeln!(out, "Crate: {} v{}", crate_info.name, crate_info.version);
    if let Some(ref desc) = crate_info.description {
        let _ = writeln!(out, "Description: {}", desc);
    }
    let _ = writeln!(out, "Root: {}", crate_info.root_dir.display());
    let _ = writeln!(out, "Manifest: {}", crate_info.manifest_path.display());
    if let Some(ref lib) = crate_info.lib_path {
        let _ = writeln!(out, "Library: {}", lib.display());
    }
    if !crate_info.bin_paths.is_empty() {
        let bins: Vec<String> = crate_info.bin_paths.iter().map(|p| p.display().to_string()).collect();
        let _ = writeln!(out, "Binaries: {}", bins.join(", "));
    }
    let _ = writeln!(out, "Edition: {}", crate_info.edition);
    if !crate_info.features.is_empty() {
        let _ = writeln!(out, "Features ({}): {}", crate_info.features.len(), crate_info.features.join(", "));
    }
    if !crate_info.dependencies.is_empty() {
        let _ = writeln!(out, "Dependencies ({}): {}", crate_info.dependencies.len(), crate_info.dependencies.join(", "));
    }

    print_output(out, cli);
    Ok(())
}

fn handle_list(args: ListArgs, cli: &Cli) -> Result<()> {
    use std::fmt::Write;
    let list = CrateLocator::list_cached(args.filter.as_deref())?;

    if cli.json {
        print_output(serde_json::to_string_pretty(&list)?, cli);
        return Ok(());
    }

    let mut out = String::new();
    let _ = writeln!(out, "Cached crates in local cargo registry ({} found):\n", list.len());
    for (name, ver, path) in list {
        let _ = writeln!(out, "  - {} v{} ({})", name, ver, path.display());
    }

    print_output(out, cli);
    Ok(())
}

fn handle_find(args: FindCliArgs, cli: &Cli, refresh: bool, no_cache: bool) -> Result<()> {
    use std::fmt::Write;
    let cache = CacheManager::new(!no_cache, refresh);
    let find_args = FindArgs {
        query: args.query.clone(),
        path: args.path,
        workspace_only: args.workspace_only,
        deps_only: args.deps_only,
        specific_crate: args.specific_crate,
        kind: args.kind,
        returns: args.returns,
        takes: args.takes,
        all: args.all,
        doc: args.doc,
        limit: args.limit,
    };

    let hits = CrossSearcher::find(&find_args, &cache)?;

    if cli.json {
        print_output(serde_json::to_string_pretty(&hits)?, cli);
        return Ok(());
    }

    if hits.is_empty() {
        print_output("No symbols found matching query across project and dependencies.\n".to_string(), cli);
        return Ok(());
    }

    let mut out = String::new();
    let query_desc = if args.query.is_empty() {
        "filters".to_string()
    } else {
        format!("'{}'", args.query)
    };
    let _ = writeln!(out, "Found {} results for {} across project and dependencies:\n", hits.len(), query_desc);

    for (i, hit) in hits.iter().enumerate() {
        let sym = &hit.symbol;
        let badge = if hit.is_workspace {
            format!("[workspace: {}]", hit.origin_crate)
        } else {
            format!("[dep: {} v{}]", hit.origin_crate, hit.origin_version)
        };
        let kind_badge = format!("[{}]", sym.kind.as_str());
        let vis_str = if sym.visibility.is_public() { "" } else { " (internal)" };
        let feat_badge = sym.feature.as_deref().map(|f| format!(" [feature: {}]", f)).unwrap_or_default();
        let clean_sig = clean_rust_syntax(&sym.signature);

        let _ = writeln!(out, "{}. {} {} `{}`{}{} ({}:{})", i + 1, badge, kind_badge, sym.id, feat_badge, vis_str, sym.file_path, sym.line_start);
        let _ = writeln!(out, "   {}", clean_sig);

        if !sym.trait_impls.is_empty() {
            let _ = writeln!(out, "   Implements: {}", sym.trait_impls.join(", "));
        }
        if !sym.methods.is_empty() {
            let total = sym.methods.len();
            let preview: Vec<&str> = sym.methods.iter().take(8).map(|m| m.name.as_str()).collect();
            let more = if total > 8 {
                format!(", ... ({} total)", total)
            } else {
                String::new()
            };
            let _ = writeln!(out, "   Methods: {}{}", preview.join(", "), more);
        }
        if !sym.doc.is_empty() {
            let first_line = sym.doc.lines().next().unwrap_or("").trim();
            if !first_line.is_empty() {
                let _ = writeln!(out, "   {}", first_line);
            }
        }
        let _ = writeln!(out);
    }

    print_output(out, cli);
    Ok(())
}

fn handle_deps(args: DepsCliArgs, cli: &Cli) -> Result<()> {
    let report = DepsInspector::inspect(args.target.as_deref())?;

    if cli.json {
        print_output(serde_json::to_string_pretty(&report)?, cli);
        return Ok(());
    }

    let md = DepsInspector::render_markdown(&report);
    print_output(md, cli);
    Ok(())
}

fn handle_workspace(args: WorkspaceCliArgs, cli: &Cli) -> Result<()> {
    let path = args.path.unwrap_or_else(|| PathBuf::from("."));
    let abs_path = if path.is_relative() {
        std::env::current_dir()?.join(&path)
    } else {
        path
    };

    let ws_root = WorkspaceInfo::find_root(&abs_path)
        .context("Could not find Cargo workspace or project root (no Cargo.toml found in directory hierarchy)")?;

    let ws = WorkspaceInfo::load(&ws_root)?;

    if cli.json {
        print_output(serde_json::to_string_pretty(&ws)?, cli);
        return Ok(());
    }

    if args.mermaid {
        print_output(ws.render_mermaid(), cli);
        return Ok(());
    }

    let blueprint = ws.render_blueprint();
    print_output(blueprint, cli);
    Ok(())
}

fn handle_tokens(args: TokensCliArgs, cli: &Cli, refresh: bool, no_cache: bool) -> Result<()> {
    let target = args.target.as_deref();

    // 1. If target is explicitly provided
    if let Some(t) = target {
        let p = Path::new(t);
        if p.exists() && p.is_dir()
            && let Some(ws_root) = WorkspaceInfo::find_root(p)
                && let Ok(ws) = WorkspaceInfo::load(&ws_root)
                    && ws.members.len() > 1 {
                        if cli.json {
                            let bp = ws.render_blueprint();
                            let tokens = tokens::estimate_tokens(&bp);
                            let json_val = serde_json::json!({
                                "workspace": ws.root_dir,
                                "members": ws.members.len(),
                                "blueprint_tokens": tokens,
                            });
                            print_output(serde_json::to_string_pretty(&json_val)?, cli);
                            return Ok(());
                        }
                        let report = tokens::render_workspace_context_impact(&ws);
                        print_output(report, cli);
                        return Ok(());
                    }

        // Try indexing as crate
        let index = load_and_index(t, refresh, no_cache)?;
        if cli.json {
            let cheat = DocGenerator::generate_cheat_sheet(&index);
            let cheat_tok = tokens::estimate_tokens(&cheat);
            let outline = DocGenerator::generate_outline(&index, 3);
            let outline_tok = tokens::estimate_tokens(&outline);
            let overview = DocGenerator::generate_llm_doc(&index, false, 3);
            let overview_tok = tokens::estimate_tokens(&overview);
            let full = DocGenerator::generate_llm_doc(&index, true, 3);
            let full_tok = tokens::estimate_tokens(&full);

            let json_val = serde_json::json!({
                "crate": index.info.name,
                "version": index.info.version,
                "cheat_tokens": cheat_tok,
                "outline_tokens": outline_tok,
                "overview_tokens": overview_tok,
                "full_tokens": full_tok,
                "symbols_total": index.stats.total_symbols,
                "symbols_public": index.stats.public_symbols,
            });
            print_output(serde_json::to_string_pretty(&json_val)?, cli);
            return Ok(());
        }

        let report = tokens::render_crate_context_impact(&index);
        print_output(report, cli);
        return Ok(());
    }

    // 2. Target is None -> check current directory
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if let Some(ws_root) = WorkspaceInfo::find_root(&cwd)
        && let Ok(ws) = WorkspaceInfo::load(&ws_root) {
            if ws.members.len() > 1 {
                if cli.json {
                    let bp = ws.render_blueprint();
                    let tokens = tokens::estimate_tokens(&bp);
                    let json_val = serde_json::json!({
                        "workspace": ws.root_dir,
                        "members": ws.members.len(),
                        "blueprint_tokens": tokens,
                    });
                    print_output(serde_json::to_string_pretty(&json_val)?, cli);
                    return Ok(());
                }
                let report = tokens::render_workspace_context_impact(&ws);
                print_output(report, cli);
                return Ok(());
            } else if let Some(single) = ws.members.first() {
                let index = load_and_index(&single.abs_path.to_string_lossy(), refresh, no_cache)?;
                let report = tokens::render_crate_context_impact(&index);
                print_output(report, cli);
                return Ok(());
            }
        }

    if cwd.join("Cargo.toml").exists() {
        let index = load_and_index(".", refresh, no_cache)?;
        let report = tokens::render_crate_context_impact(&index);
        print_output(report, cli);
        return Ok(());
    }

    bail!("No crate or workspace found in current directory. Usage: cratemd tokens [crate-name-or-path]");
}

fn handle_features(args: FeaturesArgs, cli: &Cli, refresh: bool, no_cache: bool) -> Result<()> {
    let index = load_and_index(&args.crate_name, refresh, no_cache)?;
    let report = FeaturesInspector::inspect(&index, args.feature.as_deref());
    if cli.json {
        print_output(serde_json::to_string_pretty(&report)?, cli);
    } else {
        print_output(report.render_ascii(), cli);
    }
    Ok(())
}

fn handle_impls(args: ImplsArgs, cli: &Cli, refresh: bool, no_cache: bool) -> Result<()> {
    let index = load_and_index(&args.crate_name, refresh, no_cache)?;
    let report = ImplsQuery::query(&index, args.query.as_deref());
    if cli.json {
        print_output(serde_json::to_string_pretty(&report)?, cli);
    } else {
        print_output(report.render_ascii(), cli);
    }
    Ok(())
}

fn handle_refs(args: RefsArgs, cli: &Cli) -> Result<()> {
    let report = WorkspaceRefsFinder::find(&args.symbol, args.path.as_deref(), args.limit)?;
    if cli.json {
        print_output(serde_json::to_string_pretty(&report)?, cli);
    } else {
        print_output(report.render_ascii(), cli);
    }
    Ok(())
}

fn handle_def(args: DefArgs, cli: &Cli) -> Result<()> {
    let report = DefFinder::find(
        &args.symbol,
        args.path.as_deref(),
        args.exact,
        args.snippet,
        args.limit,
    )?;

    if cli.json {
        print_output(serde_json::to_string_pretty(&report)?, cli);
    } else {
        print_output(DefFinder::render_markdown(&report), cli);
    }
    Ok(())
}

fn handle_calls(args: CallsArgs, cli: &Cli) -> Result<()> {
    let report = CallsFinder::find(
        &args.function,
        args.path.as_deref(),
        args.incoming,
        args.outgoing,
    )?;

    if cli.json {
        print_output(serde_json::to_string_pretty(&report)?, cli);
    } else {
        print_output(CallsFinder::render_markdown(&report), cli);
    }
    Ok(())
}

fn handle_hover(args: HoverArgs, cli: &Cli) -> Result<()> {
    let info = HoverInspector::hover(&args.symbol, args.path.as_deref())?;

    if cli.json {
        print_output(serde_json::to_string_pretty(&info)?, cli);
    } else if let Some(ref h) = info {
        print_output(HoverInspector::render_markdown(h), cli);
    } else {
        print_output(format!("No hover information found for `{}`\n", args.symbol), cli);
    }
    Ok(())
}

fn handle_context(args: ContextArgs, cli: &Cli) -> Result<()> {
    let report = PipelinedContextFinder::inspect(&args.symbol, args.path.as_deref())?;

    if cli.json {
        print_output(serde_json::to_string_pretty(&report)?, cli);
    } else {
        print_output(PipelinedContextFinder::render_markdown(&report), cli);
    }
    Ok(())
}

fn handle_audit(args: AuditArgs, cli: &Cli) -> Result<()> {
    let report = DependencyAuditor::audit(args.target.as_deref())?;
    if cli.json {
        print_output(serde_json::to_string_pretty(&report)?, cli);
    } else {
        print_output(report.render_ascii(), cli);
    }
    Ok(())
}

fn handle_warm(args: WarmArgs, cli: &Cli, refresh: bool) -> Result<()> {
    let report = CacheWarmer::warm(args.target.as_deref(), args.all, refresh)?;
    if cli.json {
        print_output(serde_json::to_string_pretty(&report)?, cli);
    } else {
        print_output(report.render_ascii(), cli);
    }
    Ok(())
}

fn handle_file(args: FileArgs, cli: &Cli) -> Result<()> {
    let report = FileAnalyzer::analyze(&args.path)?;
    if cli.json {
        if let Some(ref sym) = args.symbol {
            let q = sym.to_lowercase();
            let matched: Vec<_> = report.items().into_iter().filter(|it| {
                it.name == *sym || it.name.to_lowercase().contains(&q) || it.details.iter().any(|d| d.to_lowercase().contains(&q))
            }).collect();
            print_output(serde_json::to_string_pretty(&matched)?, cli);
        } else {
            print_output(serde_json::to_string_pretty(&report)?, cli);
        }
    } else if let Some(ref sym) = args.symbol {
        print_output(report.render_symbol(sym, args.body), cli);
    } else {
        print_output(report.render_ascii(), cli);
    }
    Ok(())
}

fn handle_init(args: InitArgs, cli: &Cli) -> Result<()> {
    let target = match args.path {
        Some(p) => p,
        None => std::env::current_dir()?,
    };

    let db = ProjectDb::open(Some(&target))?;
    let summary = db.init_project(&target, args.deps)?;

    if cli.json {
        let json_val = serde_json::json!({
            "status": "success",
            "db_path": db.db_path.to_string_lossy(),
            "summary": summary.trim(),
        });
        print_output(serde_json::to_string_pretty(&json_val)?, cli);
    } else {
        let mut out = format!("Initialized project context database at `{}`\n", db.db_path.display());
        out.push_str(&summary);
        print_output(out, cli);
    }
    Ok(())
}

fn handle_memory(args: MemoryArgs, cli: &Cli) -> Result<()> {
    let db = ProjectDb::open(None)?;

    match args.action {
        MemoryAction::Get { key } => {
            let mem = db.get_memory(&key)?;
            if cli.json {
                print_output(serde_json::to_string_pretty(&mem)?, cli);
            } else if let Some(m) = mem {
                let mut out = format!("# Memory: `{}` (Category: `{}`)\n*Updated: {}*\n\n", m.key, m.category, m.updated_at);
                out.push_str(&m.content);
                if !out.ends_with('\n') {
                    out.push('\n');
                }
                print_output(out, cli);
            } else {
                print_output(format!("No memory entry found for key `{}`\n", key), cli);
            }
        }
        MemoryAction::Set { key, category, content } => {
            db.set_memory(&key, &category, &content)?;
            if cli.json {
                let json_val = serde_json::json!({
                    "status": "success",
                    "key": key,
                    "category": category,
                });
                print_output(serde_json::to_string_pretty(&json_val)?, cli);
            } else {
                print_output(format!("Stored memory `{}` under category `{}`\n", key, category), cli);
            }
        }
        MemoryAction::List => {
            let list = db.list_memory()?;
            if cli.json {
                print_output(serde_json::to_string_pretty(&list)?, cli);
            } else if list.is_empty() {
                print_output("No memory entries stored in .cratemd.db\n".to_string(), cli);
            } else {
                let mut out = format!("# Project Memory Entries ({})\n\n", list.len());
                for (key, cat, updated) in list {
                    out.push_str(&format!("- `{}` [{}] (updated: {})\n", key, cat, updated));
                }
                print_output(out, cli);
            }
        }
        MemoryAction::Search { query, limit } => {
            let results = db.search_memory(&query, limit)?;
            if cli.json {
                print_output(serde_json::to_string_pretty(&results)?, cli);
            } else if results.is_empty() {
                print_output(format!("No memory matching query `{}`\n", query), cli);
            } else {
                let mut out = format!("# Memory Search: `{}` ({} results)\n\n", query, results.len());
                for r in results {
                    out.push_str(&format!("## `{}` [{}]\n*Updated: {}*\n\n", r.key, r.category, r.updated_at));
                    let snippet: String = r.content.lines().take(5).collect::<Vec<_>>().join("\n");
                    out.push_str(&snippet);
                    if r.content.lines().count() > 5 {
                        out.push_str("\n*... (more content)*");
                    }
                    out.push_str("\n\n---\n\n");
                }
                print_output(out, cli);
            }
        }
    }
    Ok(())
}

fn handle_mcp(_args: McpArgs) -> Result<()> {
    McpServer::run()
}


