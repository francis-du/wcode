//! Tokenization and ranking for the context query that drives Agent Context and
//! the convergence plan. Kept beside the runtime so `reconcile.rs` stays under
//! the source bound.

pub(crate) fn literal_symbol_queries(query: &str) -> Vec<String> {
    let code_literals = crate::intelligence::code_query_literals(query);
    if !code_literals.is_empty() {
        // The literal parser and final search plan already cap this at eight.
        return code_literals;
    }
    query
        .split(|ch: char| !ch.is_alphanumeric() && !matches!(ch, '_' | ':' | '.'))
        .map(|word| word.trim_matches([':', '.']))
        .filter(|word| !word.is_empty() && *word == query.trim())
        .take(1)
        .map(str::to_ascii_lowercase)
        .collect()
}

pub(crate) fn is_context_symbol_stopword(token: &str) -> bool {
    matches!(
        token,
        "find"
            | "where"
            | "which"
            | "show"
            | "please"
            | "check"
            | "inspect"
            | "understand"
            | "explain"
            | "code"
            | "function"
            | "method"
            | "class"
            | "file"
            | "module"
            | "behavior"
            | "logic"
            | "implementation"
            | "implement"
            | "change"
            | "modify"
            | "update"
            | "fix"
            | "issue"
            | "bug"
            | "performance"
            | "optimize"
            | "fast"
            | "faster"
            | "call"
            | "calls"
            | "caller"
            | "callee"
            | "reference"
            | "references"
            | "usage"
            | "impact"
    )
}

pub(crate) fn context_symbol_rank(
    symbol: &serde_json::Value,
    literals: &[String],
    queries: &[String],
    prefer_test_symbols: bool,
) -> (u8, usize, usize, usize) {
    let name = symbol["name"].as_str().unwrap_or("").to_ascii_lowercase();
    let qualified = symbol["qualified_name"]
        .as_str()
        .unwrap_or("")
        .to_ascii_lowercase();
    let path = symbol["path"].as_str().unwrap_or("").to_ascii_lowercase();
    let signature = symbol["signature"]
        .as_str()
        .unwrap_or("")
        .to_ascii_lowercase();
    let query_hits = queries
        .iter()
        .filter(|term| {
            name.contains(term.as_str())
                || qualified.contains(term.as_str())
                || signature.contains(term.as_str())
        })
        .count();
    let test_penalty = usize::from(!prefer_test_symbols && context_symbol_test_path(&path));
    let coverage_rank = usize::MAX.saturating_sub(query_hits);
    for (class, terms, exact) in [
        (0, literals, true),
        (1, queries, true),
        (2, literals, false),
        (3, queries, false),
    ] {
        if let Some(index) = terms.iter().position(|term| {
            if exact {
                name == *term || qualified == *term
            } else {
                name.contains(term.as_str())
                    || qualified.contains(term.as_str())
                    || signature.contains(term.as_str())
                    || path.contains(term.as_str())
            }
        }) {
            let class = if literals.is_empty() && matches!(class, 1 | 3) {
                1
            } else {
                class
            };
            return (class, test_penalty, coverage_rank, index);
        }
    }
    (4, test_penalty, coverage_rank, usize::MAX)
}

fn context_symbol_test_path(path: &str) -> bool {
    let path = path.replace('\\', "/");
    path.starts_with("tests/")
        || path.starts_with("test/")
        || path.starts_with("__tests__/")
        || path.contains("/tests/")
        || path.contains("/__tests__/")
}

pub(crate) fn context_query_prefers_tests(tokens: &[String]) -> bool {
    tokens.iter().any(|token| {
        matches!(
            token.as_str(),
            "test"
                | "tests"
                | "testing"
                | "regression"
                | "verify"
                | "verification"
                | "测试"
                | "回归"
                | "验证"
        )
    })
}
