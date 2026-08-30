use super::*;

pub(super) fn remove_file_record(state: &mut IndexState, key: &FileKey) {
    if let Some(record) = state.files.remove(key) {
        for symbol in &record.symbols {
            if state.symbol_files.get(&symbol.id) == Some(key) {
                state.symbol_files.remove(&symbol.id);
            }
        }
    }
}

pub(super) fn prune_ast_cache(state: &mut IndexState) {
    while state.ast_cache.len() > MAX_AST_CACHE_FILES {
        let Some(oldest) = state
            .ast_cache
            .iter()
            .min_by_key(|(_, entry)| entry.last_used)
            .map(|(key, _)| key.clone())
        else {
            break;
        };
        state.ast_cache.remove(&oldest);
    }
}

pub(super) fn matching_symbols_many(
    record: &FileRecord,
    queries: &[String],
    kind: Option<&str>,
) -> Vec<(usize, u8, CodeSymbol)> {
    queries
        .iter()
        .enumerate()
        .flat_map(|(query_index, query)| {
            matching_symbols(record, query, kind)
                .into_iter()
                .map(move |(score, symbol)| (query_index, score, symbol))
        })
        .collect()
}

pub(super) fn matching_symbols(
    record: &FileRecord,
    query: &str,
    kind: Option<&str>,
) -> Vec<(u8, CodeSymbol)> {
    record
        .symbols
        .iter()
        .filter(|symbol| symbol.is_definition)
        .filter(|symbol| kind.is_none_or(|kind| symbol.kind.eq_ignore_ascii_case(kind)))
        .filter_map(|symbol| symbol_score(symbol, query).map(|score| (score, symbol.clone())))
        .collect()
}

pub(super) fn symbol_score(symbol: &CodeSymbol, query: &str) -> Option<u8> {
    if symbol.name == query || symbol.qualified_name == query {
        return Some(0);
    }
    if query.is_ascii() && symbol.name.is_ascii() && symbol.qualified_name.is_ascii() {
        if symbol.name.eq_ignore_ascii_case(query)
            || symbol.qualified_name.eq_ignore_ascii_case(query)
        {
            return Some(1);
        }
        if starts_with_ascii_case_insensitive(&symbol.name, query)
            || starts_with_ascii_case_insensitive(&symbol.qualified_name, query)
        {
            return Some(2);
        }
        if contains_ascii_case_insensitive(symbol.name.as_bytes(), query.as_bytes())
            || contains_ascii_case_insensitive(symbol.qualified_name.as_bytes(), query.as_bytes())
        {
            return Some(3);
        }
        return None;
    }

    let query = query.to_lowercase();
    let name = symbol.name.to_lowercase();
    let qualified = symbol.qualified_name.to_lowercase();
    if name == query || qualified == query {
        Some(1)
    } else if name.starts_with(&query) || qualified.starts_with(&query) {
        Some(2)
    } else if name.contains(&query) || qualified.contains(&query) {
        Some(3)
    } else {
        None
    }
}

pub(super) fn normalize_symbol_kind(kind: &str) -> &str {
    match kind {
        "send" => "call",
        _ => kind,
    }
}

pub(super) fn semantic_extent<'tree>(
    language: LanguageId,
    node: Node<'tree>,
    kind: &str,
    is_definition: bool,
) -> Node<'tree> {
    if !is_definition
        || !matches!(language, LanguageId::C | LanguageId::Cpp)
        || !matches!(kind, "function" | "method")
    {
        return node;
    }

    let mut current = Some(node);
    while let Some(candidate) = current {
        if matches!(
            candidate.kind(),
            "function_definition" | "declaration" | "field_declaration"
        ) {
            return candidate;
        }
        if candidate.kind() == "translation_unit" {
            break;
        }
        current = candidate.parent();
    }
    node
}

pub(super) fn symbol_query_leaf(query: &str) -> &str {
    query
        .rsplit([':', '.', '#', '/', '\\'])
        .find(|segment| !segment.is_empty())
        .unwrap_or(query)
}

pub(super) fn contains_case_insensitive(content: &str, query: &str) -> bool {
    if content.contains(query) {
        return true;
    }
    if content.is_ascii() && query.is_ascii() {
        return contains_ascii_case_insensitive(content.as_bytes(), query.as_bytes());
    }
    content.to_lowercase().contains(&query.to_lowercase())
}

pub(super) fn starts_with_ascii_case_insensitive(haystack: &str, needle: &str) -> bool {
    haystack
        .as_bytes()
        .get(..needle.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(needle.as_bytes()))
}

pub(super) fn contains_ascii_case_insensitive(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    if needle.len() > haystack.len() {
        return false;
    }

    let lower = needle[0].to_ascii_lowercase();
    let upper = needle[0].to_ascii_uppercase();
    let mut offset = 0usize;
    while offset + needle.len() <= haystack.len() {
        let remaining = &haystack[offset..];
        let next = if lower == upper {
            memchr(lower, remaining)
        } else {
            memchr2(lower, upper, remaining)
        };
        let Some(relative) = next else {
            return false;
        };
        let start = offset + relative;
        let end = start + needle.len();
        if end > haystack.len() {
            return false;
        }
        if haystack[start..end].eq_ignore_ascii_case(needle) {
            return true;
        }
        offset = start + 1;
    }
    false
}

pub(super) fn assign_containers(language: LanguageId, symbols: &mut [CodeSymbol]) {
    let mut definitions = symbols
        .iter()
        .enumerate()
        .filter(|(_, symbol)| symbol.is_definition)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    definitions.sort_by(|left, right| {
        symbols[*left]
            .start_byte
            .cmp(&symbols[*right].start_byte)
            .then_with(|| symbols[*right].end_byte.cmp(&symbols[*left].end_byte))
    });

    let mut stack = Vec::<usize>::new();
    for index in definitions {
        while stack
            .last()
            .is_some_and(|parent| symbols[*parent].end_byte <= symbols[index].start_byte)
        {
            stack.pop();
        }
        let nested_parent = stack.last().copied().filter(|parent| {
            symbols[*parent].start_byte <= symbols[index].start_byte
                && symbols[*parent].end_byte >= symbols[index].end_byte
                && (symbols[*parent].start_byte != symbols[index].start_byte
                    || symbols[*parent].end_byte != symbols[index].end_byte)
        });
        let container = nested_parent
            .map(|parent| symbols[parent].qualified_name.clone())
            .or_else(|| symbols[index].container_hint.clone());
        if let Some(container) = container {
            symbols[index].container = Some(container.clone());
            symbols[index].qualified_name = format!(
                "{container}{}{}",
                language.qualifier_separator(),
                symbols[index].name
            );
        }
        stack.push(index);
    }
}

pub(super) fn syntactic_container_hint(
    language: LanguageId,
    node: Node<'_>,
    source: &str,
) -> Option<String> {
    if language != LanguageId::Rust {
        return None;
    }
    let mut ancestor = node.parent();
    while let Some(parent) = ancestor {
        if parent.kind() == "impl_item" {
            return parent
                .child_by_field_name("type")
                .and_then(|node| source.get(node.byte_range()))
                .map(collapse_whitespace)
                .filter(|value| !value.is_empty());
        }
        ancestor = parent.parent();
    }
    None
}

pub(super) fn node_range(node: Node<'_>) -> SourceRange {
    let start = node.start_position();
    let end = node.end_position();
    SourceRange {
        start_line: start.row + 1,
        start_column: start.column + 1,
        end_line: end.row + 1,
        end_column: end.column + 1,
        end_exclusive: true,
    }
}

pub(super) fn inclusive_end_line(start: Point, end: Point) -> usize {
    if end.column == 0 && end.row > start.row {
        end.row.max(1)
    } else {
        end.row + 1
    }
}

pub(super) fn line_excerpt(source: &str, byte: usize, max_chars: usize) -> String {
    let bytes = source.as_bytes();
    let byte = byte.min(bytes.len());
    let mut start = byte;
    while start > 0 && bytes[start - 1] != b'\n' {
        start -= 1;
    }
    let mut end = byte;
    while end < bytes.len() && bytes[end] != b'\n' {
        end += 1;
    }
    let line = source.get(start..end).unwrap_or_default();
    truncate_chars(&collapse_whitespace(line), max_chars)
}

pub(super) fn collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(super) fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_owned();
    }
    let mut output = value
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    output.push('…');
    output
}

pub(super) fn symbol_id(
    root: &Path,
    path: &str,
    name: &str,
    kind: &str,
    is_definition: bool,
    start: usize,
    end: usize,
) -> String {
    let mut digest = Sha256::new();
    digest.update(root.to_string_lossy().as_bytes());
    digest.update([0]);
    digest.update(path.as_bytes());
    digest.update([0]);
    digest.update(name.as_bytes());
    digest.update([0]);
    digest.update(kind.as_bytes());
    digest.update([u8::from(is_definition)]);
    digest.update(start.to_le_bytes());
    digest.update(end.to_le_bytes());
    let encoded = format!("{:x}", digest.finalize());
    format!("ts:{}", &encoded[..24])
}
