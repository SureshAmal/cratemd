use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "cratemd",
    author,
    version,
    about = "Instant offline Rust crate intelligence for LLMs and developers",
    long_about = "Analyze local Rust crates, generate ctags, tree-sitter outlines, LLM markdown documentation, and search symbols offline without downloading anything."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Name of the crate or path (shorthand for generating LLM documentation)
    #[arg(global = false)]
    pub crate_name: Option<String>,

    /// Output results in JSON format for automated tooling and agents
    #[arg(long, global = true)]
    pub json: bool,

    /// Force re-analyzing and refresh persistent cache
    #[arg(long, global = true)]
    pub refresh: bool,

    /// Disable disk cache for this invocation
    #[arg(long, global = true)]
    pub no_cache: bool,

    /// Enforce maximum token budget on stdout output to prevent context window overflow
    #[arg(long, global = true)]
    pub max_tokens: Option<usize>,

    /// Show estimated token count of the output
    #[arg(long, global = true)]
    pub tokens: bool,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// Generate LLM-optimized single document documentation for a crate
    Doc(DocArgs),

    /// Search for symbols, signatures, methods, or docstrings in a crate
    Search(SearchArgs),

    /// Generate an ultra-condensed ~500-token cheat sheet of key types & functions
    Cheat(CheatArgs),

    /// Extract and view runnable code examples from docs and examples/ directory
    Examples(ExamplesArgs),

    /// Show the hierarchical module and symbol outline of a crate
    Outline(OutlineArgs),

    /// Inspect a specific symbol (struct, enum, trait, fn, method) in detail
    View(ViewArgs),

    /// Generate Universal Ctags for a crate
    Ctags(CtagsArgs),

    /// Generate Tree-sitter AST outline or S-expressions for a crate or file
    Treesitter(TreesitterArgs),

    /// Locate a crate on the local system and print its metadata
    Locate(LocateArgs),

    /// List all crates available in the local cargo cache
    List(ListArgs),

    /// Search across workspace member crates and external dependencies
    Find(FindCliArgs),

    /// List and inspect dependencies of the current project or workspace
    Deps(DepsCliArgs),

    /// Inspect workspace architecture, member crates, and dependency relationships
    Workspace(WorkspaceCliArgs),

    /// Analyze token footprint and context window impact of a crate or workspace
    Tokens(TokensCliArgs),

    /// Inspect cargo feature flags, default features, and feature-gated symbols
    Features(FeaturesArgs),

    /// Query trait implementations and find implementors of traits or types
    Impls(ImplsArgs),

    /// Find all references and usages of a symbol across workspace crates
    Refs(RefsArgs),

    /// Audit dependency health, version splits, and offline cache readiness
    Audit(AuditArgs),

    /// Pre-warm offline disk cache for workspace crates and dependencies
    Warm(WarmArgs),

    /// Read and analyze a single Rust source file (.rs)
    File(FileArgs),

    /// Start Model Context Protocol (MCP) server over stdio for LLMs and agents
    Mcp(McpArgs),
}

#[derive(Args, Debug, Clone)]
pub struct DocArgs {
    /// Name or path of the crate
    pub crate_name: String,

    /// Generate full documentation (including all fields, methods, and full docstrings)
    #[arg(long)]
    pub full: bool,

    /// Maximum module nesting depth to include in the overview
    #[arg(long, default_value = "3")]
    pub max_depth: usize,

    /// Write output to a file instead of stdout
    #[arg(short, long)]
    pub out: Option<PathBuf>,
}

#[derive(Args, Debug, Clone)]
pub struct SearchArgs {
    /// Name or path of the crate
    pub crate_name: String,

    /// Search query (symbol name, path, method, or keyword)
    #[arg(default_value = "")]
    pub query: String,

    /// Filter by symbol kind
    #[arg(short, long)]
    pub kind: Option<CliSymbolKind>,

    /// Filter functions/methods returning this type (e.g. --returns Result)
    #[arg(long)]
    pub returns: Option<String>,

    /// Filter functions/methods taking this type (e.g. --takes TcpStream)
    #[arg(long)]
    pub takes: Option<String>,

    /// Include private and restricted items (default is public-only)
    #[arg(long)]
    pub all: bool,

    /// Search inside docstrings in addition to symbol names and signatures
    #[arg(long)]
    pub doc: bool,

    /// Maximum number of search results to return
    #[arg(short, long, default_value = "20")]
    pub limit: usize,
}

#[derive(Args, Debug, Clone)]
pub struct CheatArgs {
    /// Name or path of the crate
    pub crate_name: String,
}

#[derive(Args, Debug, Clone)]
pub struct ExamplesArgs {
    /// Name or path of the crate
    pub crate_name: String,

    /// Optional query to filter examples by title or keyword
    pub query: Option<String>,
}

#[derive(Args, Debug, Clone)]
pub struct OutlineArgs {
    /// Name or path of the crate
    pub crate_name: String,

    /// Maximum module depth
    #[arg(long, default_value = "4")]
    pub max_depth: usize,
}

#[derive(Args, Debug, Clone)]
pub struct ViewArgs {
    /// Name or path of the crate
    pub crate_name: String,

    /// Symbol name or path to inspect (e.g. "Serialize", "tokio::task::spawn")
    pub symbol: String,

    /// Include exact source code implementation block
    #[arg(short = 'b', long = "body")]
    pub body: bool,
}

#[derive(Args, Debug, Clone)]
pub struct CtagsArgs {
    /// Name or path of the crate
    pub crate_name: String,

    /// Output tags file path (default prints to stdout)
    #[arg(short, long)]
    pub out: Option<PathBuf>,
}

#[derive(Args, Debug, Clone)]
pub struct TreesitterArgs {
    /// Name or path of the crate
    pub crate_name: String,

    /// Relative file path within crate (e.g. "src/lib.rs"), defaults to entry file
    pub file: Option<String>,

    /// Output S-expression instead of human-readable AST outline
    #[arg(long)]
    pub sexp: bool,
}

#[derive(Args, Debug, Clone)]
pub struct LocateArgs {
    /// Name of the crate
    pub crate_name: String,
}

#[derive(Args, Debug, Clone)]
pub struct ListArgs {
    /// Optional filter for crate names
    pub filter: Option<String>,
}

#[derive(Args, Debug, Clone)]
pub struct FindCliArgs {
    /// Search query (symbol name, path, method, or keyword)
    #[arg(default_value = "")]
    pub query: String,

    /// Target project directory (defaults to current directory)
    #[arg(short, long)]
    pub path: Option<PathBuf>,

    /// Search only within workspace member crates (exclude external dependencies)
    #[arg(short = 'w', long)]
    pub workspace_only: bool,

    /// Search only within external dependencies (exclude workspace members)
    #[arg(short = 'd', long)]
    pub deps_only: bool,

    /// Restrict search to a specific crate name (either workspace member or dependency)
    #[arg(short = 'c', long = "crate")]
    pub specific_crate: Option<String>,

    /// Filter by symbol kind
    #[arg(short, long)]
    pub kind: Option<CliSymbolKind>,

    /// Filter functions/methods returning this type (e.g. --returns Result)
    #[arg(long)]
    pub returns: Option<String>,

    /// Filter functions/methods taking this type (e.g. --takes TcpStream)
    #[arg(long)]
    pub takes: Option<String>,

    /// Include private and restricted items (default is public-only)
    #[arg(long)]
    pub all: bool,

    /// Search inside docstrings in addition to symbol names and signatures
    #[arg(long)]
    pub doc: bool,

    /// Maximum number of search results to return
    #[arg(short, long, default_value = "25")]
    pub limit: usize,
}

#[derive(Args, Debug, Clone)]
pub struct DepsCliArgs {
    /// Path to crate or workspace root (defaults to current directory)
    pub target: Option<String>,
}

#[derive(Args, Debug, Clone)]
pub struct WorkspaceCliArgs {
    /// Path to workspace directory (defaults to current directory)
    pub path: Option<PathBuf>,
}

#[derive(Args, Debug, Clone)]
pub struct TokensCliArgs {
    /// Crate name, directory path, or workspace root (defaults to current directory)
    pub target: Option<String>,
}

#[derive(Args, Debug, Clone)]
pub struct FeaturesArgs {
    /// Name or path of the crate
    pub crate_name: String,

    /// Inspect a specific feature and list its enabled symbols and dependencies
    pub feature: Option<String>,
}

#[derive(Args, Debug, Clone)]
pub struct ImplsArgs {
    /// Name or path of the crate
    pub crate_name: String,

    /// Trait or struct/enum type name to query implementations for
    pub query: Option<String>,
}

#[derive(Args, Debug, Clone)]
pub struct RefsArgs {
    /// Symbol name or path to search references for across workspace crates
    pub symbol: String,

    /// Workspace path (defaults to current directory)
    pub path: Option<PathBuf>,

    /// Maximum number of matching references to return
    #[arg(short, long, default_value = "50")]
    pub limit: usize,
}

#[derive(Args, Debug, Clone)]
pub struct AuditArgs {
    /// Workspace or crate directory path (defaults to current directory)
    pub target: Option<PathBuf>,
}

#[derive(Args, Debug, Clone)]
pub struct WarmArgs {
    /// Crate name or path (defaults to current project/workspace members and direct dependencies)
    pub target: Option<String>,

    /// Also pre-warm transitive dependencies found in Cargo.lock
    #[arg(long)]
    pub all: bool,
}

#[derive(Args, Debug, Clone)]
pub struct FileArgs {
    /// Path to the .rs file to analyze
    pub path: PathBuf,

    /// Optional symbol or function name to inspect specifically within this file
    pub symbol: Option<String>,

    /// Include exact source code implementation block
    #[arg(short = 'b', long = "body")]
    pub body: bool,
}

#[derive(Args, Debug, Clone)]
pub struct McpArgs {}


#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum CliSymbolKind {
    Fn,
    Struct,
    Enum,
    Trait,
    Method,
    Type,
    Const,
    Static,
    Macro,
    Module,
    Impl,
}

impl CliSymbolKind {
    pub fn to_model_kind(&self) -> crate::model::SymbolKind {
        match self {
            CliSymbolKind::Fn => crate::model::SymbolKind::Function,
            CliSymbolKind::Struct => crate::model::SymbolKind::Struct,
            CliSymbolKind::Enum => crate::model::SymbolKind::Enum,
            CliSymbolKind::Trait => crate::model::SymbolKind::Trait,
            CliSymbolKind::Method => crate::model::SymbolKind::Method,
            CliSymbolKind::Type => crate::model::SymbolKind::TypeAlias,
            CliSymbolKind::Const => crate::model::SymbolKind::Const,
            CliSymbolKind::Static => crate::model::SymbolKind::Static,
            CliSymbolKind::Macro => crate::model::SymbolKind::Macro,
            CliSymbolKind::Module => crate::model::SymbolKind::Module,
            CliSymbolKind::Impl => crate::model::SymbolKind::Impl,
        }
    }
}
