use serde::Serialize;
use crate::model::{CrateIndex, Symbol, SymbolKind};

#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub text: String,
    pub kind_filter: Option<SymbolKind>,
    pub pub_only: bool,
    pub search_docs: bool,
    pub search_signatures: bool,
    pub returns_filter: Option<String>,
    pub takes_filter: Option<String>,
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    pub symbol: Symbol,
    pub score: i32,
    pub matched_in: Vec<&'static str>,
}

pub struct CrateSearcher;

impl CrateSearcher {
    pub fn search(index: &CrateIndex, query: &SearchQuery) -> Vec<SearchHit> {
        let q = query.text.to_lowercase().trim().to_string();
        let tokens: Vec<&str> = q.split_whitespace().collect();
        let ret_filter = query.returns_filter.as_ref().map(|s| s.to_lowercase().trim().to_string());
        let takes_filter = query.takes_filter.as_ref().map(|s| s.to_lowercase().trim().to_string());

        let mut hits = Vec::new();

        for sym in &index.symbols {
            if query.pub_only && !sym.visibility.is_public() {
                continue;
            }

            if let Some(ref k) = query.kind_filter
                && sym.kind != *k {
                    continue;
                }

            // Apply signature filters
            if !matches_signature_filters(sym, &ret_filter, &takes_filter) {
                continue;
            }

            let (score, matched_in) = score_symbol(sym, &q, &tokens, query);
            if score > 0 {
                hits.push(SearchHit {
                    symbol: sym.clone(),
                    score,
                    matched_in,
                });
            }

            // Also check methods attached to this symbol
            for method in &sym.methods {
                if query.pub_only && !method.visibility.is_public() {
                    continue;
                }

                if let Some(ref k) = query.kind_filter
                    && method.kind != *k {
                        continue;
                    }

                if !matches_signature_filters(method, &ret_filter, &takes_filter) {
                    continue;
                }

                let (m_score, m_matched) = score_symbol(method, &q, &tokens, query);
                if m_score > 0 {
                    hits.push(SearchHit {
                        symbol: method.clone(),
                        score: m_score,
                        matched_in: m_matched,
                    });
                }
            }
        }

        // Sort descending by score
        hits.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.symbol.id.cmp(&b.symbol.id)));

        // Deduplicate by symbol ID
        hits.dedup_by(|a, b| a.symbol.id == b.symbol.id);

        if hits.len() > query.limit {
            hits.truncate(query.limit);
        }

        hits
    }
}

fn matches_signature_filters(
    sym: &Symbol,
    ret_filter: &Option<String>,
    takes_filter: &Option<String>,
) -> bool {
    if ret_filter.is_none() && takes_filter.is_none() {
        return true;
    }

    let sig_lower = sym.signature.to_lowercase();

    if let Some(ret) = ret_filter {
        if let Some(idx) = sig_lower.find("->") {
            let ret_part = &sig_lower[idx + 2..];
            if !ret_part.contains(ret.as_str()) {
                return false;
            }
        } else {
            return false;
        }
    }

    if let Some(takes) = takes_filter {
        if let Some(start) = sig_lower.find('(') {
            if let Some(end) = sig_lower.rfind(')') {
                if start < end {
                    let args_part = &sig_lower[start + 1..end];
                    if !args_part.contains(takes.as_str()) {
                        return false;
                    }
                } else {
                    return false;
                }
            } else {
                return false;
            }
        } else {
            return false;
        }
    }

    true
}

fn score_symbol(
    sym: &Symbol,
    q: &str,
    tokens: &[&str],
    query: &SearchQuery,
) -> (i32, Vec<&'static str>) {
    let mut score = 0;
    let mut matched_in = Vec::new();

    // If query text is empty but filters are active, give default score
    if q.is_empty() {
        if query.returns_filter.is_some() || query.takes_filter.is_some() || query.kind_filter.is_some() {
            return (50, vec!["filter"]);
        }
        return (0, matched_in);
    }

    let name_lower = sym.name.to_lowercase();
    let id_lower = sym.id.to_lowercase();
    let sig_lower = sym.signature.to_lowercase();
    let doc_lower = sym.doc.to_lowercase();

    // 1. Exact / prefix / substring match on entire query string
    if name_lower == q {
        score += 100;
        matched_in.push("exact_name");
    } else if name_lower.starts_with(q) {
        score += 70;
        matched_in.push("prefix_name");
    } else if name_lower.contains(q) {
        score += 50;
        matched_in.push("substring_name");
    }

    if id_lower.contains(q) && !matched_in.contains(&"exact_name") && !matched_in.contains(&"substring_name") {
        score += 40;
        matched_in.push("path");
    }

    if (query.search_signatures || score == 0) && sig_lower.contains(q) {
        score += 25;
        matched_in.push("signature");
    }

    if query.search_docs && doc_lower.contains(q) {
        score += 15;
        matched_in.push("doc");
    }

    // 2. Multi-token scoring if there are multiple words (e.g. "json parse")
    if tokens.len() > 1 {
        let mut tokens_found = 0;
        for &tok in tokens {
            if name_lower.contains(tok) || sig_lower.contains(tok) || (query.search_docs && doc_lower.contains(tok)) {
                tokens_found += 1;
            }
        }
        if tokens_found == tokens.len() {
            score += 40; // All tokens matched
            matched_in.push("all_tokens");
        } else if tokens_found > 0 {
            score += tokens_found as i32 * 10;
        }
    }

    (score, matched_in)
}
