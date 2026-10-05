use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureDef {
    pub name: String,
    pub is_default: bool,
    pub sub_features: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrateInfo {
    pub name: String,
    pub version: String,
    pub edition: String,
    pub description: Option<String>,
    pub root_dir: PathBuf,
    pub manifest_path: PathBuf,
    pub lib_path: Option<PathBuf>,
    pub bin_paths: Vec<PathBuf>,
    pub dependencies: Vec<String>,
    pub features: Vec<String>,
    #[serde(default)]
    pub feature_defs: Vec<FeatureDef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SymbolKind {
    Module,
    Struct,
    Enum,
    Variant,
    Trait,
    Function,
    Method,
    TypeAlias,
    Const,
    Static,
    Macro,
    Field,
    Impl,
}

impl SymbolKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SymbolKind::Module => "module",
            SymbolKind::Struct => "struct",
            SymbolKind::Enum => "enum",
            SymbolKind::Variant => "variant",
            SymbolKind::Trait => "trait",
            SymbolKind::Function => "fn",
            SymbolKind::Method => "method",
            SymbolKind::TypeAlias => "type",
            SymbolKind::Const => "const",
            SymbolKind::Static => "static",
            SymbolKind::Macro => "macro",
            SymbolKind::Field => "field",
            SymbolKind::Impl => "impl",
        }
    }

    pub fn ctags_kind(&self) -> char {
        match self {
            SymbolKind::Module => 'm',
            SymbolKind::Struct => 's',
            SymbolKind::Enum => 'g',
            SymbolKind::Variant => 'e',
            SymbolKind::Trait => 't',
            SymbolKind::Function => 'f',
            SymbolKind::Method => 'm',
            SymbolKind::TypeAlias => 'T',
            SymbolKind::Const => 'c',
            SymbolKind::Static => 'v',
            SymbolKind::Macro => 'd',
            SymbolKind::Field => 'w',
            SymbolKind::Impl => 'i',
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Visibility {
    Public,
    Restricted, // pub(crate), pub(super), etc.
    Private,
}

impl Visibility {
    pub fn is_public(&self) -> bool {
        matches!(self, Visibility::Public)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Visibility::Public => "pub",
            Visibility::Restricted => "pub(crate)",
            Visibility::Private => "private",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeExample {
    pub title: String,
    pub source_symbol: Option<String>,
    pub code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    pub id: String,
    pub name: String,
    pub kind: SymbolKind,
    pub visibility: Visibility,
    pub module_path: String,
    pub file_path: String,
    pub line_start: usize,
    pub line_end: usize,
    pub signature: String,
    pub doc: String,
    pub parent: Option<String>,
    pub detail: Option<String>,
    pub methods: Vec<Symbol>,
    pub trait_impls: Vec<String>,
    pub examples: Vec<CodeExample>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature: Option<String>,
}

impl Symbol {
    #[cfg(test)]
    pub fn new(
        name: String,
        id: String,
        kind: SymbolKind,
        visibility: Visibility,
        signature: String,
        file_path: String,
        line_start: usize,
        line_end: usize,
    ) -> Self {
        Self {
            id,
            name,
            kind,
            visibility,
            module_path: String::new(),
            file_path,
            line_start,
            line_end,
            signature,
            doc: String::new(),
            parent: None,
            detail: None,
            methods: Vec::new(),
            trait_impls: Vec::new(),
            examples: Vec::new(),
            feature: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModuleNode {
    pub name: String,
    pub full_path: String,
    pub file_path: Option<String>,
    pub doc: String,
    pub submodules: Vec<ModuleNode>,
    pub symbols: Vec<Symbol>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrateIndex {
    pub info: CrateInfo,
    pub symbols: Vec<Symbol>,
    pub root_module: ModuleNode,
    pub standalone_examples: Vec<CodeExample>,
    pub stats: CrateStats,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CrateStats {
    pub total_files: usize,
    pub total_symbols: usize,
    pub public_symbols: usize,
    pub structs_count: usize,
    pub enums_count: usize,
    pub traits_count: usize,
    pub functions_count: usize,
    pub methods_count: usize,
    pub macros_count: usize,
}


