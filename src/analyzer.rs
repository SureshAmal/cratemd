use anyhow::{Context, Result};
use quote::ToTokens;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use syn::spanned::Spanned;
use syn::{
    Attribute, Expr, ExprLit, Fields, FnArg, Generics, Item, ItemConst, ItemEnum, ItemFn,
    ItemImpl, ItemMacro, ItemStatic, ItemStruct, ItemTrait, ItemType, ItemUse, Lit, Meta,
    ReturnType, Signature, TraitItem, Type, UseGroup, UseName, UsePath, UseRename, UseTree,
    Visibility as SynVisibility,
};
use walkdir::WalkDir;

use crate::model::*;

pub struct CrateAnalyzer {
    crate_info: CrateInfo,
}

impl CrateAnalyzer {
    pub fn new(crate_info: CrateInfo) -> Self {
        Self { crate_info }
    }

    pub fn analyze(&self) -> Result<CrateIndex> {
        let mut all_symbols = Vec::new();
        let mut stats = CrateStats::default();
        let mut visited_files: HashSet<PathBuf> = HashSet::new();

        let crate_name = self.crate_info.name.replace('-', "_");

        // 1. Entry point: lib.rs or main.rs
        let entry_point = if let Some(ref lib) = self.crate_info.lib_path {
            lib.clone()
        } else if let Some(first_bin) = self.crate_info.bin_paths.first() {
            first_bin.clone()
        } else {
            self.crate_info.root_dir.join("src/lib.rs")
        };

        let mut root_mod = if entry_point.exists() {
            visited_files.insert(canonicalize_or_self(&entry_point));
            self.parse_module_file(
                &entry_point,
                &crate_name,
                &mut all_symbols,
                &mut stats,
                &mut visited_files,
            )?
        } else {
            ModuleNode {
                name: crate_name.clone(),
                full_path: crate_name.clone(),
                file_path: None,
                doc: String::new(),
                submodules: Vec::new(),
                symbols: Vec::new(),
            }
        };

        // 2. Discover all other .rs files in src/ that were not covered by standard mod declarations
        let src_dir = self.crate_info.root_dir.join("src");
        if src_dir.exists() {
            for entry in WalkDir::new(&src_dir).into_iter().filter_map(Result::ok) {
                let path = entry.path();
                if path.is_file() && path.extension().map_or(false, |ext| ext == "rs") {
                    let canon = canonicalize_or_self(path);
                    if !visited_files.contains(&canon) {
                        visited_files.insert(canon);

                        // Deduce module path from relative path to src/
                        if let Ok(rel) = path.strip_prefix(&src_dir) {
                            let inferred_mod = deduce_module_path(&crate_name, rel);
                            if let Ok(orphan_mod) = self.parse_module_file(
                                path,
                                &inferred_mod,
                                &mut all_symbols,
                                &mut stats,
                                &mut visited_files,
                            ) {
                                root_mod.submodules.push(orphan_mod);
                            }
                        }
                    }
                }
            }
        }

        // Attach inherent impl methods and trait implementations to structs/enums
        attach_impl_data(&mut all_symbols);

        let standalone_examples = load_standalone_examples(&self.crate_info.root_dir);

        // Update statistics
        stats.total_symbols = all_symbols.len();
        stats.public_symbols = all_symbols.iter().filter(|s| s.visibility.is_public()).count();

        Ok(CrateIndex {
            info: self.crate_info.clone(),
            symbols: all_symbols,
            root_module: root_mod,
            standalone_examples,
            stats,
        })
    }

    fn parse_module_file(
        &self,
        file_path: &Path,
        mod_path: &str,
        all_symbols: &mut Vec<Symbol>,
        stats: &mut CrateStats,
        visited_files: &mut HashSet<PathBuf>,
    ) -> Result<ModuleNode> {
        stats.total_files += 1;
        let content = std::fs::read_to_string(file_path)
            .with_context(|| format!("Failed to read {}", file_path.display()))?;

        let rel_file_path = file_path
            .strip_prefix(&self.crate_info.root_dir)
            .unwrap_or(file_path)
            .to_string_lossy()
            .to_string();

        let syntax = match syn::parse_file(&content) {
            Ok(s) => s,
            Err(_) => {
                return Ok(ModuleNode {
                    name: mod_name_from_path(mod_path),
                    full_path: mod_path.to_string(),
                    file_path: Some(rel_file_path),
                    doc: String::new(),
                    submodules: Vec::new(),
                    symbols: Vec::new(),
                });
            }
        };

        let mod_doc = extract_inner_docs(&syntax.attrs);
        let mut module_symbols = Vec::new();
        let mut submodules = Vec::new();

        let base_dir = file_path.parent().unwrap_or(Path::new(""));

        for item in syntax.items {
            match item {
                Item::Struct(s) => {
                    stats.structs_count += 1;
                    let sym = self.extract_struct(&s, mod_path, &rel_file_path);
                    module_symbols.push(sym.clone());
                    all_symbols.push(sym);
                }
                Item::Enum(e) => {
                    stats.enums_count += 1;
                    let sym = self.extract_enum(&e, mod_path, &rel_file_path);
                    module_symbols.push(sym.clone());
                    all_symbols.push(sym);
                }
                Item::Trait(t) => {
                    stats.traits_count += 1;
                    let sym = self.extract_trait(&t, mod_path, &rel_file_path);
                    module_symbols.push(sym.clone());
                    all_symbols.push(sym);
                }
                Item::Fn(f) => {
                    stats.functions_count += 1;
                    let sym = self.extract_fn(&f, mod_path, &rel_file_path);
                    module_symbols.push(sym.clone());
                    all_symbols.push(sym);
                }
                Item::Type(t) => {
                    let sym = self.extract_type_alias(&t, mod_path, &rel_file_path);
                    module_symbols.push(sym.clone());
                    all_symbols.push(sym);
                }
                Item::Const(c) => {
                    let sym = self.extract_const(&c, mod_path, &rel_file_path);
                    module_symbols.push(sym.clone());
                    all_symbols.push(sym);
                }
                Item::Static(st) => {
                    let sym = self.extract_static(&st, mod_path, &rel_file_path);
                    module_symbols.push(sym.clone());
                    all_symbols.push(sym);
                }
                Item::Macro(m) => {
                    stats.macros_count += 1;
                    if let Some(sym) = self.extract_macro(&m, mod_path, &rel_file_path) {
                        module_symbols.push(sym.clone());
                        all_symbols.push(sym);
                    }
                }
                Item::Use(u) => {
                    let use_symbols = self.extract_pub_use(&u, mod_path, &rel_file_path);
                    for sym in use_symbols {
                        module_symbols.push(sym.clone());
                        all_symbols.push(sym);
                    }
                }
                Item::Impl(imp) => {
                    let impl_symbols = self.extract_impl(&imp, mod_path, &rel_file_path, stats);
                    for s in impl_symbols {
                        all_symbols.push(s);
                    }
                }
                Item::Mod(m) => {
                    let sub_name = m.ident.to_string();
                    let sub_mod_path = format!("{}::{}", mod_path, sub_name);

                    if let Some((_, items)) = m.content {
                        let inline_node = self.parse_inline_module(
                            &sub_name,
                            &sub_mod_path,
                            &m.attrs,
                            items,
                            &rel_file_path,
                            all_symbols,
                            stats,
                        );
                        submodules.push(inline_node);
                    } else {
                        let candidate1 = base_dir.join(format!("{}.rs", sub_name));
                        let candidate2 = base_dir.join(&sub_name).join("mod.rs");

                        let file_stem = file_path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                        let candidate3 = base_dir.join(file_stem).join(format!("{}.rs", sub_name));
                        let candidate4 = base_dir.join(file_stem).join(&sub_name).join("mod.rs");

                        let candidate_file = if candidate1.exists() {
                            Some(candidate1)
                        } else if candidate2.exists() {
                            Some(candidate2)
                        } else if candidate3.exists() {
                            Some(candidate3)
                        } else if candidate4.exists() {
                            Some(candidate4)
                        } else {
                            None
                        };

                        if let Some(target_file) = candidate_file {
                            let canon = canonicalize_or_self(&target_file);
                            if !visited_files.contains(&canon) {
                                visited_files.insert(canon);
                                if let Ok(sub_node) = self.parse_module_file(
                                    &target_file,
                                    &sub_mod_path,
                                    all_symbols,
                                    stats,
                                    visited_files,
                                ) {
                                    submodules.push(sub_node);
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        Ok(ModuleNode {
            name: mod_name_from_path(mod_path),
            full_path: mod_path.to_string(),
            file_path: Some(rel_file_path),
            doc: mod_doc,
            submodules,
            symbols: module_symbols,
        })
    }

    fn parse_inline_module(
        &self,
        name: &str,
        mod_path: &str,
        attrs: &[Attribute],
        items: Vec<Item>,
        rel_file_path: &str,
        all_symbols: &mut Vec<Symbol>,
        stats: &mut CrateStats,
    ) -> ModuleNode {
        let mod_doc = extract_docs(attrs);
        let mut module_symbols = Vec::new();
        let submodules = Vec::new();

        for item in items {
            match item {
                Item::Struct(s) => {
                    stats.structs_count += 1;
                    let sym = self.extract_struct(&s, mod_path, rel_file_path);
                    module_symbols.push(sym.clone());
                    all_symbols.push(sym);
                }
                Item::Enum(e) => {
                    stats.enums_count += 1;
                    let sym = self.extract_enum(&e, mod_path, rel_file_path);
                    module_symbols.push(sym.clone());
                    all_symbols.push(sym);
                }
                Item::Trait(t) => {
                    stats.traits_count += 1;
                    let sym = self.extract_trait(&t, mod_path, rel_file_path);
                    module_symbols.push(sym.clone());
                    all_symbols.push(sym);
                }
                Item::Fn(f) => {
                    stats.functions_count += 1;
                    let sym = self.extract_fn(&f, mod_path, rel_file_path);
                    module_symbols.push(sym.clone());
                    all_symbols.push(sym);
                }
                Item::Type(t) => {
                    let sym = self.extract_type_alias(&t, mod_path, rel_file_path);
                    module_symbols.push(sym.clone());
                    all_symbols.push(sym);
                }
                Item::Const(c) => {
                    let sym = self.extract_const(&c, mod_path, rel_file_path);
                    module_symbols.push(sym.clone());
                    all_symbols.push(sym);
                }
                Item::Static(st) => {
                    let sym = self.extract_static(&st, mod_path, rel_file_path);
                    module_symbols.push(sym.clone());
                    all_symbols.push(sym);
                }
                Item::Macro(m) => {
                    stats.macros_count += 1;
                    if let Some(sym) = self.extract_macro(&m, mod_path, rel_file_path) {
                        module_symbols.push(sym.clone());
                        all_symbols.push(sym);
                    }
                }
                Item::Use(u) => {
                    let use_symbols = self.extract_pub_use(&u, mod_path, rel_file_path);
                    for sym in use_symbols {
                        module_symbols.push(sym.clone());
                        all_symbols.push(sym);
                    }
                }
                Item::Impl(imp) => {
                    let impl_symbols = self.extract_impl(&imp, mod_path, rel_file_path, stats);
                    for s in impl_symbols {
                        all_symbols.push(s);
                    }
                }
                _ => {}
            }
        }

        ModuleNode {
            name: name.to_string(),
            full_path: mod_path.to_string(),
            file_path: Some(rel_file_path.to_string()),
            doc: mod_doc,
            submodules,
            symbols: module_symbols,
        }
    }

    fn extract_struct(&self, s: &ItemStruct, mod_path: &str, file: &str) -> Symbol {
        let name = s.ident.to_string();
        let full_id = format!("{}::{}", mod_path, name);
        let vis = parse_visibility(&s.vis);
        let doc = extract_docs(&s.attrs);
        let generics_str = format_generics(&s.generics);
        let span = s.span();

        let detail = match &s.fields {
            Fields::Named(named) => {
                let fields: Vec<String> = named
                    .named
                    .iter()
                    .map(|f| {
                        let f_name = f.ident.as_ref().map(|i| i.to_string()).unwrap_or_default();
                        let f_ty = clean_rust_syntax(&f.ty.to_token_stream().to_string());
                        let f_vis = parse_visibility(&f.vis);
                        format!("{} {}: {}", f_vis.as_str(), f_name, f_ty)
                    })
                    .collect();
                Some(format!("{{\n  {}\n}}", fields.join(",\n  ")))
            }
            Fields::Unnamed(unnamed) => {
                let types: Vec<String> = unnamed
                    .unnamed
                    .iter()
                    .map(|f| clean_rust_syntax(&f.ty.to_token_stream().to_string()))
                    .collect();
                Some(format!("({});", types.join(", ")))
            }
            Fields::Unit => Some(";".to_string()),
        };

        let signature = clean_rust_syntax(&format!("{}struct {}{}", vis_prefix(vis), name, generics_str));
        let derives = extract_derives(&s.attrs);
        let examples = extract_examples_from_doc(&doc, &name);

        Symbol {
            id: full_id,
            name,
            kind: SymbolKind::Struct,
            visibility: vis,
            module_path: mod_path.to_string(),
            file_path: file.to_string(),
            line_start: span.start().line,
            line_end: span.end().line,
            signature,
            doc,
            parent: None,
            detail,
            methods: Vec::new(),
            trait_impls: derives,
            examples,
            feature: extract_cfg_feature(&s.attrs),
        }
    }

    fn extract_enum(&self, e: &ItemEnum, mod_path: &str, file: &str) -> Symbol {
        let name = e.ident.to_string();
        let full_id = format!("{}::{}", mod_path, name);
        let vis = parse_visibility(&e.vis);
        let doc = extract_docs(&e.attrs);
        let generics_str = format_generics(&e.generics);
        let span = e.span();

        let variants: Vec<String> = e
            .variants
            .iter()
            .map(|v| {
                let v_name = v.ident.to_string();
                let v_doc = extract_docs(&v.attrs);
                let v_body = match &v.fields {
                    Fields::Named(named) => {
                        let flds: Vec<String> = named
                            .named
                            .iter()
                            .map(|f| {
                                format!(
                                    "{}: {}",
                                    f.ident.as_ref().unwrap(),
                                    clean_rust_syntax(&f.ty.to_token_stream().to_string())
                                )
                            })
                            .collect();
                        format!(" {{ {} }}", flds.join(", "))
                    }
                    Fields::Unnamed(unnamed) => {
                        let flds: Vec<String> = unnamed
                            .unnamed
                            .iter()
                            .map(|f| clean_rust_syntax(&f.ty.to_token_stream().to_string()))
                            .collect();
                        format!("({})", flds.join(", "))
                    }
                    Fields::Unit => String::new(),
                };
                if v_doc.is_empty() {
                    format!("{}{}", v_name, v_body)
                } else {
                    format!("/// {}\n  {}{}", v_doc.replace('\n', " "), v_name, v_body)
                }
            })
            .collect();

        let detail = Some(format!("{{\n  {}\n}}", variants.join(",\n  ")));
        let signature = clean_rust_syntax(&format!("{}enum {}{}", vis_prefix(vis), name, generics_str));
        let derives = extract_derives(&e.attrs);
        let examples = extract_examples_from_doc(&doc, &name);

        Symbol {
            id: full_id,
            name,
            kind: SymbolKind::Enum,
            visibility: vis,
            module_path: mod_path.to_string(),
            file_path: file.to_string(),
            line_start: span.start().line,
            line_end: span.end().line,
            signature,
            doc,
            parent: None,
            detail,
            methods: Vec::new(),
            trait_impls: derives,
            examples,
            feature: extract_cfg_feature(&e.attrs),
        }
    }

    fn extract_trait(&self, t: &ItemTrait, mod_path: &str, file: &str) -> Symbol {
        let name = t.ident.to_string();
        let full_id = format!("{}::{}", mod_path, name);
        let vis = parse_visibility(&t.vis);
        let doc = extract_docs(&t.attrs);
        let generics_str = format_generics(&t.generics);
        let span = t.span();

        let supertraits = if !t.supertraits.is_empty() {
            format!(": {}", clean_rust_syntax(&t.supertraits.to_token_stream().to_string()))
        } else {
            String::new()
        };

        let mut trait_items = Vec::new();
        for item in &t.items {
            match item {
                TraitItem::Fn(m) => {
                    let sig = format_signature(&m.sig);
                    let item_doc = extract_docs(&m.attrs);
                    let has_default = m.default.is_some();
                    let suffix = if has_default { " { ... }" } else { ";" };
                    if item_doc.is_empty() {
                        trait_items.push(format!("  {}{}", sig, suffix));
                    } else {
                        trait_items.push(format!("  /// {}\n  {}{}", item_doc.replace('\n', " "), sig, suffix));
                    }
                }
                TraitItem::Type(ty) => {
                    let ty_name = ty.ident.to_string();
                    let bounds = if !ty.bounds.is_empty() {
                        format!(": {}", clean_rust_syntax(&ty.bounds.to_token_stream().to_string()))
                    } else {
                        String::new()
                    };
                    trait_items.push(format!("  type {}{};", ty_name, bounds));
                }
                TraitItem::Const(c) => {
                    let c_name = c.ident.to_string();
                    let c_ty = clean_rust_syntax(&c.ty.to_token_stream().to_string());
                    trait_items.push(format!("  const {}: {};", c_name, c_ty));
                }
                _ => {}
            }
        }

        let detail = Some(format!("{{\n{}\n}}", trait_items.join("\n")));
        let signature = clean_rust_syntax(&format!("{}trait {}{}{}", vis_prefix(vis), name, generics_str, supertraits));
        let examples = extract_examples_from_doc(&doc, &name);

        Symbol {
            id: full_id,
            name,
            kind: SymbolKind::Trait,
            visibility: vis,
            module_path: mod_path.to_string(),
            file_path: file.to_string(),
            line_start: span.start().line,
            line_end: span.end().line,
            signature,
            doc,
            parent: None,
            detail,
            methods: Vec::new(),
            trait_impls: Vec::new(),
            examples,
            feature: extract_cfg_feature(&t.attrs),
        }
    }

    fn extract_fn(&self, f: &ItemFn, mod_path: &str, file: &str) -> Symbol {
        let name = f.sig.ident.to_string();
        let full_id = format!("{}::{}", mod_path, name);
        let vis = parse_visibility(&f.vis);
        let doc = extract_docs(&f.attrs);
        let signature = clean_rust_syntax(&format!("{}{};", vis_prefix(vis), format_signature(&f.sig)));
        let span = f.span();
        let examples = extract_examples_from_doc(&doc, &name);

        Symbol {
            id: full_id,
            name,
            kind: SymbolKind::Function,
            visibility: vis,
            module_path: mod_path.to_string(),
            file_path: file.to_string(),
            line_start: span.start().line,
            line_end: span.end().line,
            signature,
            doc,
            parent: None,
            detail: None,
            methods: Vec::new(),
            trait_impls: Vec::new(),
            examples,
            feature: extract_cfg_feature(&f.attrs),
        }
    }

    fn extract_type_alias(&self, t: &ItemType, mod_path: &str, file: &str) -> Symbol {
        let name = t.ident.to_string();
        let full_id = format!("{}::{}", mod_path, name);
        let vis = parse_visibility(&t.vis);
        let doc = extract_docs(&t.attrs);
        let generics_str = format_generics(&t.generics);
        let target_ty = clean_rust_syntax(&t.ty.to_token_stream().to_string());
        let signature = clean_rust_syntax(&format!("{}type {}{} = {};", vis_prefix(vis), name, generics_str, target_ty));
        let span = t.span();
        let examples = extract_examples_from_doc(&doc, &name);

        Symbol {
            id: full_id,
            name,
            kind: SymbolKind::TypeAlias,
            visibility: vis,
            module_path: mod_path.to_string(),
            file_path: file.to_string(),
            line_start: span.start().line,
            line_end: span.end().line,
            signature,
            doc,
            parent: None,
            detail: None,
            methods: Vec::new(),
            trait_impls: Vec::new(),
            examples,
            feature: extract_cfg_feature(&t.attrs),
        }
    }

    fn extract_const(&self, c: &ItemConst, mod_path: &str, file: &str) -> Symbol {
        let name = c.ident.to_string();
        let full_id = format!("{}::{}", mod_path, name);
        let vis = parse_visibility(&c.vis);
        let doc = extract_docs(&c.attrs);
        let ty_str = clean_rust_syntax(&c.ty.to_token_stream().to_string());
        let signature = clean_rust_syntax(&format!("{}const {}: {};", vis_prefix(vis), name, ty_str));
        let span = c.span();
        let examples = extract_examples_from_doc(&doc, &name);

        Symbol {
            id: full_id,
            name,
            kind: SymbolKind::Const,
            visibility: vis,
            module_path: mod_path.to_string(),
            file_path: file.to_string(),
            line_start: span.start().line,
            line_end: span.end().line,
            signature,
            doc,
            parent: None,
            detail: None,
            methods: Vec::new(),
            trait_impls: Vec::new(),
            examples,
            feature: extract_cfg_feature(&c.attrs),
        }
    }

    fn extract_static(&self, st: &ItemStatic, mod_path: &str, file: &str) -> Symbol {
        let name = st.ident.to_string();
        let full_id = format!("{}::{}", mod_path, name);
        let vis = parse_visibility(&st.vis);
        let doc = extract_docs(&st.attrs);
        let ty_str = clean_rust_syntax(&st.ty.to_token_stream().to_string());
        let mut_str = match &st.mutability {
            syn::StaticMutability::Mut(_) => "mut ",
            _ => "",
        };
        let signature = clean_rust_syntax(&format!("{}static {}{}: {};", vis_prefix(vis), mut_str, name, ty_str));
        let span = st.span();
        let examples = extract_examples_from_doc(&doc, &name);

        Symbol {
            id: full_id,
            name,
            kind: SymbolKind::Static,
            visibility: vis,
            module_path: mod_path.to_string(),
            file_path: file.to_string(),
            line_start: span.start().line,
            line_end: span.end().line,
            signature,
            doc,
            parent: None,
            detail: None,
            methods: Vec::new(),
            trait_impls: Vec::new(),
            examples,
            feature: extract_cfg_feature(&st.attrs),
        }
    }

    fn extract_macro(&self, m: &ItemMacro, mod_path: &str, file: &str) -> Option<Symbol> {
        let name = m.ident.as_ref()?.to_string();
        let full_id = format!("{}::{}", mod_path, name);
        let doc = extract_docs(&m.attrs);
        let signature = format!("macro_rules! {} {{ ... }}", name);
        let span = m.span();
        let examples = extract_examples_from_doc(&doc, &name);

        Some(Symbol {
            id: full_id,
            name,
            kind: SymbolKind::Macro,
            visibility: Visibility::Public,
            module_path: mod_path.to_string(),
            file_path: file.to_string(),
            line_start: span.start().line,
            line_end: span.end().line,
            signature,
            doc,
            parent: None,
            detail: None,
            methods: Vec::new(),
            trait_impls: Vec::new(),
            examples,
            feature: extract_cfg_feature(&m.attrs),
        })
    }

    fn extract_pub_use(&self, u: &ItemUse, mod_path: &str, file: &str) -> Vec<Symbol> {
        let vis = parse_visibility(&u.vis);
        if !vis.is_public() {
            return Vec::new();
        }

        let mut symbols = Vec::new();
        let doc = extract_docs(&u.attrs);
        let span = u.span();

        let mut names = Vec::new();
        collect_use_names(&u.tree, String::new(), &mut names);

        for (imported_name, full_source) in names {
            let full_id = format!("{}::{}", mod_path, imported_name);
            let signature = format!("pub use {};", full_source);

            symbols.push(Symbol {
                id: full_id,
                name: imported_name,
                kind: SymbolKind::TypeAlias, // Re-exported type/item
                visibility: Visibility::Public,
                module_path: mod_path.to_string(),
                file_path: file.to_string(),
                line_start: span.start().line,
                line_end: span.end().line,
                signature,
                doc: doc.clone(),
                parent: None,
                detail: Some(format!("Re-export of `{}`", full_source)),
                methods: Vec::new(),
                trait_impls: Vec::new(),
                examples: Vec::new(),
                feature: extract_cfg_feature(&u.attrs),
            });
        }

        symbols
    }

    fn extract_impl(
        &self,
        imp: &ItemImpl,
        mod_path: &str,
        file: &str,
        stats: &mut CrateStats,
    ) -> Vec<Symbol> {
        let mut symbols = Vec::new();
        let target_name = get_type_name(&imp.self_ty);
        let trait_name = imp.trait_.as_ref().map(|(path, _)| clean_rust_syntax(&path.to_token_stream().to_string()));
        let impl_feat = extract_cfg_feature(&imp.attrs);

        if let Some(ref t_name) = trait_name {
            let impl_name = clean_rust_syntax(&format!("impl {} for {}", t_name, target_name));
            let full_id = format!("{}::(impl {} for {})", mod_path, t_name, target_name);
            let span = imp.span();
            symbols.push(Symbol {
                id: full_id,
                name: impl_name.clone(),
                kind: SymbolKind::Impl,
                visibility: Visibility::Public,
                module_path: mod_path.to_string(),
                file_path: file.to_string(),
                line_start: span.start().line,
                line_end: span.end().line,
                signature: impl_name,
                doc: extract_docs(&imp.attrs),
                parent: Some(target_name.clone()),
                detail: Some(t_name.clone()),
                methods: Vec::new(),
                trait_impls: Vec::new(),
                examples: Vec::new(),
                feature: impl_feat.clone(),
            });
        }

        for item in &imp.items {
            if let syn::ImplItem::Fn(m) = item {
                stats.methods_count += 1;
                let m_name = m.sig.ident.to_string();
                let m_vis = parse_visibility(&m.vis);
                let m_doc = extract_docs(&m.attrs);
                let full_id = format!("{}::{}::{}", mod_path, target_name, m_name);
                let span = m.span();

                let sig_str = clean_rust_syntax(&format!("{}{};", vis_prefix(m_vis), format_signature(&m.sig)));
                let detail = trait_name.as_ref().map(|t| format!("implements {}", t));
                let examples = extract_examples_from_doc(&m_doc, &m_name);
                let m_feat = extract_cfg_feature(&m.attrs).or_else(|| impl_feat.clone());

                symbols.push(Symbol {
                    id: full_id,
                    name: m_name,
                    kind: SymbolKind::Method,
                    visibility: m_vis,
                    module_path: mod_path.to_string(),
                    file_path: file.to_string(),
                    line_start: span.start().line,
                    line_end: span.end().line,
                    signature: sig_str,
                    doc: m_doc,
                    parent: Some(target_name.clone()),
                    detail,
                    methods: Vec::new(),
                    trait_impls: Vec::new(),
                    examples,
                    feature: m_feat,
                });
            }
        }

        symbols
    }
}

fn collect_use_names(tree: &UseTree, prefix: String, out: &mut Vec<(String, String)>) {
    match tree {
        UseTree::Path(UsePath { ident, tree, .. }) => {
            let next_prefix = if prefix.is_empty() {
                ident.to_string()
            } else {
                format!("{}::{}", prefix, ident)
            };
            collect_use_names(tree, next_prefix, out);
        }
        UseTree::Name(UseName { ident }) => {
            let name = ident.to_string();
            let full = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{}::{}", prefix, name)
            };
            out.push((name, full));
        }
        UseTree::Rename(UseRename { rename, .. }) => {
            let name = rename.to_string();
            let full = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{}::{}", prefix, name)
            };
            out.push((name, full));
        }
        UseTree::Group(UseGroup { items, .. }) => {
            for item in items {
                collect_use_names(item, prefix.clone(), out);
            }
        }
        UseTree::Glob(_) => {
            // Glob import
            let name = "*".to_string();
            let full = format!("{}::*", prefix);
            out.push((name, full));
        }
    }
}

fn canonicalize_or_self(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

fn deduce_module_path(crate_name: &str, rel_path: &Path) -> String {
    let mut parts = vec![crate_name.to_string()];
    for component in rel_path.components() {
        let seg = component.as_os_str().to_string_lossy();
        if seg == "mod.rs" || seg == "lib.rs" || seg == "main.rs" {
            continue;
        }
        let clean = seg.trim_end_matches(".rs");
        if !clean.is_empty() {
            parts.push(clean.to_string());
        }
    }
    parts.join("::")
}

fn attach_impl_data(all_symbols: &mut [Symbol]) {
    let mut methods_by_parent: HashMap<String, Vec<Symbol>> = HashMap::new();
    let mut traits_by_parent: HashMap<String, Vec<String>> = HashMap::new();

    for s in all_symbols.iter() {
        if s.kind == SymbolKind::Method {
            if let Some(ref p) = s.parent {
                methods_by_parent.entry(p.clone()).or_default().push(s.clone());
            }
        } else if s.kind == SymbolKind::Impl {
            if let (Some(p), Some(t)) = (&s.parent, &s.detail) {
                traits_by_parent.entry(p.clone()).or_default().push(t.clone());
            }
        }
    }

    for s in all_symbols.iter_mut() {
        if s.kind == SymbolKind::Struct || s.kind == SymbolKind::Enum {
            if let Some(methods) = methods_by_parent.get(&s.name) {
                s.methods = methods.clone();
            }
            if let Some(traits) = traits_by_parent.get(&s.name) {
                for t in traits {
                    if !s.trait_impls.contains(t) {
                        s.trait_impls.push(t.clone());
                    }
                }
            }
            s.trait_impls.sort();
            s.trait_impls.dedup();
        }
    }
}

fn extract_derives(attrs: &[Attribute]) -> Vec<String> {
    let mut derives = Vec::new();
    for attr in attrs {
        if attr.path().is_ident("derive") {
            let _ = attr.parse_nested_meta(|meta| {
                if let Some(ident) = meta.path.get_ident() {
                    derives.push(ident.to_string());
                } else {
                    derives.push(meta.path.to_token_stream().to_string());
                }
                Ok(())
            });
        }
    }
    derives
}

fn extract_examples_from_doc(doc: &str, symbol_name: &str) -> Vec<CodeExample> {
    let mut examples = Vec::new();
    let mut in_block = false;
    let mut current_code = Vec::new();
    let mut example_idx = 1;
    let mut current_title = format!("Example {}", example_idx);

    for line in doc.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("# ") || trimmed.starts_with("## ") || trimmed.starts_with("### ") {
            let heading = trimmed.trim_start_matches('#').trim();
            if heading.to_lowercase().contains("example") {
                current_title = heading.to_string();
            }
        } else if trimmed.starts_with("```") {
            if in_block {
                in_block = false;
                let code = current_code.join("\n").trim().to_string();
                if !code.is_empty() {
                    examples.push(CodeExample {
                        title: current_title.clone(),
                        source_symbol: Some(symbol_name.to_string()),
                        code,
                    });
                    example_idx += 1;
                    current_title = format!("Example {}", example_idx);
                }
                current_code.clear();
            } else {
                in_block = true;
                current_code.clear();
            }
        } else if in_block {
            current_code.push(line);
        }
    }
    examples
}

fn load_standalone_examples(root_dir: &Path) -> Vec<CodeExample> {
    let examples_dir = root_dir.join("examples");
    let mut result = Vec::new();
    if !examples_dir.is_dir() {
        return result;
    }

    if let Ok(entries) = std::fs::read_dir(&examples_dir) {
        let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        paths.sort();
        for path in paths {
            if path.extension().map_or(false, |ext| ext == "rs") {
                let file_name = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
                if let Ok(content) = std::fs::read_to_string(&path) {
                    result.push(CodeExample {
                        title: format!("examples/{}.rs", file_name),
                        source_symbol: None,
                        code: content,
                    });
                }
            }
        }
    }
    result
}

fn get_type_name(ty: &Type) -> String {
    match ty {
        Type::Path(p) => p
            .path
            .segments
            .last()
            .map(|s| s.ident.to_string())
            .unwrap_or_else(|| p.path.to_token_stream().to_string()),
        _ => ty.to_token_stream().to_string(),
    }
}

fn parse_visibility(vis: &SynVisibility) -> Visibility {
    match vis {
        SynVisibility::Public(_) => Visibility::Public,
        SynVisibility::Restricted(_) => Visibility::Restricted,
        SynVisibility::Inherited => Visibility::Private,
    }
}

fn vis_prefix(vis: Visibility) -> &'static str {
    match vis {
        Visibility::Public => "pub ",
        Visibility::Restricted => "pub(crate) ",
        Visibility::Private => "",
    }
}

pub(crate) fn extract_docs(attrs: &[Attribute]) -> String {
    let mut lines = Vec::new();
    for attr in attrs {
        if attr.path().is_ident("doc") {
            if let Meta::NameValue(nv) = &attr.meta {
                if let Expr::Lit(ExprLit {
                    lit: Lit::Str(s), ..
                }) = &nv.value
                {
                    lines.push(s.value());
                }
            }
        }
    }
    clean_doc_lines(&lines)
}

fn extract_inner_docs(attrs: &[Attribute]) -> String {
    extract_docs(attrs)
}

fn clean_doc_lines(lines: &[String]) -> String {
    let trimmed: Vec<&str> = lines.iter().map(|l| l.trim_matches('\n')).collect();
    trimmed.join("\n").trim().to_string()
}

fn extract_cfg_feature(attrs: &[Attribute]) -> Option<String> {
    for attr in attrs {
        let tokens = attr.to_token_stream().to_string();
        if tokens.contains("feature = ") || tokens.contains("feature=") {
            if let Some(start) = tokens.find("feature = \"") {
                let rest = &tokens[start + 11..];
                if let Some(end) = rest.find('"') {
                    return Some(rest[..end].to_string());
                }
            } else if let Some(start) = tokens.find("feature =") {
                let rest = &tokens[start + 9..].trim_start();
                if rest.starts_with('"') {
                    let rest = &rest[1..];
                    if let Some(end) = rest.find('"') {
                        return Some(rest[..end].to_string());
                    }
                }
            }
        }
    }
    None
}

fn format_generics(generics: &Generics) -> String {
    if generics.params.is_empty() {
        String::new()
    } else {
        let params: Vec<String> = generics.params.iter().map(|p| p.to_token_stream().to_string()).collect();
        format!("<{}>", params.join(", "))
    }
}

pub(crate) fn format_signature(sig: &Signature) -> String {
    let mut parts = Vec::new();
    if sig.constness.is_some() {
        parts.push("const");
    }
    if sig.asyncness.is_some() {
        parts.push("async");
    }
    match &sig.safety {
        syn::Safety::Unsafe(_) => parts.push("unsafe"),
        _ => {}
    }
    parts.push("fn");

    let name = sig.ident.to_string();
    let generics = format_generics(&sig.generics);

    let inputs: Vec<String> = sig.inputs.iter().map(|arg| match arg {
        FnArg::Receiver(r) => clean_rust_syntax(&r.to_token_stream().to_string()),
        FnArg::Typed(t) => {
            let pat = clean_rust_syntax(&t.pat.to_token_stream().to_string());
            let ty = clean_rust_syntax(&t.ty.to_token_stream().to_string());
            format!("{}: {}", pat, ty)
        }
    }).collect();

    let output = match &sig.output {
        ReturnType::Default => String::new(),
        ReturnType::Type(_, ty) => format!(" -> {}", clean_rust_syntax(&ty.to_token_stream().to_string())),
    };

    let where_clause = if let Some(wh) = &sig.generics.where_clause {
        format!(" {}", clean_rust_syntax(&wh.to_token_stream().to_string()))
    } else {
        String::new()
    };

    let prefix = parts.join(" ");
    clean_rust_syntax(&format!("{} {}{}({}){}{}", prefix, name, generics, inputs.join(", "), output, where_clause))
}

fn mod_name_from_path(mod_path: &str) -> String {
    mod_path.rsplit("::").next().unwrap_or(mod_path).to_string()
}

/// Cleans raw Syn token stream output into idiomatic, compact Rust syntax,
/// removing extraneous whitespace around punctuation and generics to save LLM tokens.
pub fn clean_rust_syntax(input: &str) -> String {
    let mut s = input.to_string();

    // 1. Path qualifiers and double colons
    s = s.replace(" :: ", "::");
    s = s.replace(":: ", "::");
    s = s.replace(" ::", "::");

    // 2. References, pointers, and mutability
    s = s.replace("& mut ", "&mut ");
    s = s.replace("& self", "&self");
    s = s.replace("& mut self", "&mut self");
    s = s.replace("* const ", "*const ");
    s = s.replace("* mut ", "*mut ");
    s = s.replace("& '", "&'");

    // 3. Generics and angle brackets (<T, U>)
    s = s.replace(" < ", "<");
    s = s.replace("< ", "<");
    s = s.replace(" <", "<");
    s = s.replace(" >", ">");

    // 4. Slices and arrays
    s = s.replace("& [", "&[");
    s = s.replace("&mut [", "&mut [");
    s = s.replace("[ ", "[");
    s = s.replace(" ]", "]");

    // 5. Parentheses and argument lists
    s = s.replace("( ", "(");
    s = s.replace(" )", ")");
    s = s.replace("()", "()");
    s = s.replace(" ()", "()");

    // 6. Commas, colons, and semicolons
    s = s.replace(" ,", ",");
    s = s.replace(" ;", ";");
    s = s.replace(" :", ":");
    s = s.replace(",;", ";");
    s = s.replace(", ;", ";");
    s = s.replace(",<", ", <");
    s = s.replace(",'", ", '");

    let mut comma_spaced = String::with_capacity(s.len() + 10);
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        comma_spaced.push(chars[i]);
        if chars[i] == ','
            && i + 1 < chars.len()
            && chars[i + 1] != ' '
            && chars[i + 1] != '\n'
            && chars[i + 1] != ';'
            && chars[i + 1] != ')'
            && chars[i + 1] != '>'
        {
            comma_spaced.push(' ');
        }
        i += 1;
    }
    s = comma_spaced;

    // 7. Compact reference to identifiers or types (e.g. `& StreamConfig` -> `&StreamConfig`)
    let mut cleaned = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '&'
            && i + 1 < chars.len()
            && chars[i + 1] == ' '
            && i + 2 < chars.len()
            && (chars[i + 2].is_alphanumeric() || chars[i + 2] == '_' || chars[i + 2] == '[' || chars[i + 2] == '\'')
        {
            cleaned.push('&');
            i += 2;
            continue;
        }
        cleaned.push(chars[i]);
        i += 1;
    }
    s = cleaned;

    // 8. Collapse multiple spaces
    while s.contains("  ") {
        s = s.replace("  ", " ");
    }

    s.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_rust_syntax_methods() {
        let raw = "pub fn process_capture_i16(& mut self, src: & [i16], dest: & mut [i16]) -> Result < () , Error > ;";
        let cleaned = clean_rust_syntax(raw);
        assert_eq!(cleaned, "pub fn process_capture_i16(&mut self, src: &[i16], dest: &mut [i16]) -> Result<(), Error>;");
    }

    #[test]
    fn test_clean_rust_syntax_types_and_paths() {
        let raw = "bytes :: bytes_mut :: BytesMut < Option < Vec < u8 > > >";
        let cleaned = clean_rust_syntax(raw);
        assert_eq!(cleaned, "bytes::bytes_mut::BytesMut<Option<Vec<u8>>>");
    }

    #[test]
    fn test_clean_rust_syntax_refs_and_configs() {
        let raw = "pub fn process(& mut self, config: & StreamConfig) -> Result < () , Error > ;";
        let cleaned = clean_rust_syntax(raw);
        assert_eq!(cleaned, "pub fn process(&mut self, config: &StreamConfig) -> Result<(), Error>;");
    }

    #[test]
    fn test_clean_rust_syntax_idempotency() {
        let expected = "pub fn process_capture_i16(&mut self, src: &[i16], dest: &mut [i16]) -> Result<(), Error>;";
        assert_eq!(clean_rust_syntax(expected), expected);
    }
}
