use crate::model::{CrateIndex, Symbol};

pub struct CtagsGenerator;

impl CtagsGenerator {
    /// Generates standard Universal Ctags tag file content
    pub fn generate(index: &CrateIndex) -> String {
        let mut lines = Vec::new();

        // Ctags headers
        lines.push("!_TAG_FILE_FORMAT\t2\t/extended format/".to_string());
        lines.push("!_TAG_FILE_SORTED\t1\t/0=unsorted, 1=sorted, 2=foldcase/".to_string());
        lines.push("!_TAG_PROGRAM_NAME\tcratemd\t//".to_string());
        lines.push(format!("!_TAG_PROGRAM_VERSION\t{}\t//", env!("CARGO_PKG_VERSION")));

        let mut tags: Vec<String> = Vec::new();

        for sym in &index.symbols {
            tags.push(format_tag(sym));

            // Also emit tags for struct/enum methods
            for method in &sym.methods {
                tags.push(format_tag(method));
            }
        }

        // Ctags specification requires tags to be sorted alphabetically by tag name
        tags.sort();
        lines.extend(tags);

        lines.join("\n") + "\n"
    }
}

fn format_tag(sym: &Symbol) -> String {
    let name = &sym.name;
    let file = &sym.file_path;
    let kind = sym.kind.ctags_kind();

    // Pattern or line address
    let address = if sym.line_start > 0 {
        format!("{}", sym.line_start)
    } else {
        // Fallback pattern matching name
        format!("/fn {}|struct {}|enum {}|trait {}/", name, name, name, name)
    };

    let mut fields = vec![
        format!("kind:{}", sym.kind.as_str()),
        format!("access:{}", sym.visibility.as_str()),
    ];

    if !sym.module_path.is_empty() {
        fields.push(format!("scope:module:{}", sym.module_path));
    }

    if let Some(ref parent) = sym.parent {
        fields.push(format!("scope:type:{}", parent));
    }

    if !sym.signature.is_empty() {
        // Escape tabs or newlines in signature
        let clean_sig = sym.signature.replace(['\t', '\n'], " ");
        fields.push(format!("signature:{}", clean_sig));
    }

    format!("{}\t{}\t{};\"\t{}\t{}", name, file, address, kind, fields.join("\t"))
}
