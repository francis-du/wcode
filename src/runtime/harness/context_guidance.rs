use super::*;

const FOCUSED_GUIDANCE_CHARS: usize = 560;
const BASELINE_GUIDANCE_CHARS: usize = 320;
const CONTEXT_LINES_BEFORE: usize = 2;
const CONTEXT_LINES_AFTER: usize = 3;

pub(super) fn select_guidance(
    documents: &[GuidanceDocument],
    query: &str,
    requested_scopes: &[String],
    context: &SoftwareContext,
    limit: usize,
) -> Vec<Value> {
    if limit == 0 || documents.is_empty() {
        return Vec::new();
    }
    let signals = task_signals(query, requested_scopes, context);
    let baseline = documents
        .iter()
        .position(|document| instruction_document(&document.path));
    let mut selected = Vec::<(usize, usize, Value)>::new();

    if let Some(index) = baseline {
        let document = &documents[index];
        let (excerpt, score, truncated) =
            focused_excerpt(document, &signals).unwrap_or_else(|| baseline_excerpt(document));
        selected.push((
            usize::MAX,
            index,
            guidance_value(document, excerpt, truncated),
        ));
        if score > 0 {
            selected[0].0 = score;
        }
    }

    let mut relevant = documents
        .iter()
        .enumerate()
        .filter(|(index, _)| Some(*index) != baseline)
        .filter_map(|(index, document)| {
            let (excerpt, score, truncated) = focused_excerpt(document, &signals)?;
            Some((score, index, guidance_value(document, excerpt, truncated)))
        })
        .collect::<Vec<_>>();
    relevant.sort_by(
        |(left_score, left_index, _), (right_score, right_index, _)| {
            right_score
                .cmp(left_score)
                .then_with(|| left_index.cmp(right_index))
        },
    );

    if baseline.is_none() {
        selected.extend(relevant.into_iter().take(limit));
    } else {
        selected.extend(relevant.into_iter().take(limit.saturating_sub(1)));
    }
    selected
        .into_iter()
        .take(limit)
        .map(|(_, _, value)| value)
        .collect()
}

fn guidance_value(document: &GuidanceDocument, excerpt: String, truncated: bool) -> Value {
    json!({
        "path": document.path,
        "excerpt": excerpt,
        "truncated": document.truncated || truncated,
    })
}

fn baseline_excerpt(document: &GuidanceDocument) -> (String, usize, bool) {
    let (excerpt, truncated) =
        short_text_with_truncation(&document.excerpt, BASELINE_GUIDANCE_CHARS);
    (excerpt, 0, truncated)
}

fn focused_excerpt(
    document: &GuidanceDocument,
    signals: &BTreeSet<String>,
) -> Option<(String, usize, bool)> {
    if signals.is_empty() {
        return None;
    }
    let lines = document.excerpt.lines().collect::<Vec<_>>();
    let (best_index, best_score) = lines
        .iter()
        .enumerate()
        .map(|(index, line)| (index, line_relevance(line, signals)))
        .max_by(|(left_index, left_score), (right_index, right_score)| {
            left_score
                .cmp(right_score)
                .then_with(|| right_index.cmp(left_index))
        })?;
    if best_score == 0 {
        return None;
    }

    let mut start = best_index.saturating_sub(CONTEXT_LINES_BEFORE);
    for index in (0..best_index).rev().take(6) {
        if lines[index].trim_start().starts_with('#') {
            start = index;
            break;
        }
    }
    let end = (best_index + CONTEXT_LINES_AFTER + 1).min(lines.len());
    let raw = lines[start..end].join("\n");
    let (excerpt, char_truncated) = short_text_with_truncation(&raw, FOCUSED_GUIDANCE_CHARS);
    let selection_truncated = start > 0 || end < lines.len() || char_truncated;
    Some((excerpt, best_score, selection_truncated))
}

fn line_relevance(line: &str, signals: &BTreeSet<String>) -> usize {
    let lower = line.to_ascii_lowercase();
    signals
        .iter()
        .filter(|signal| lower.contains(signal.as_str()))
        .map(|signal| {
            if signal.contains('/') {
                8
            } else if signal.len() >= 10 {
                5
            } else if signal.len() >= 6 {
                4
            } else {
                2
            }
        })
        .sum()
}

fn task_signals(
    query: &str,
    requested_scopes: &[String],
    context: &SoftwareContext,
) -> BTreeSet<String> {
    let mut signals = BTreeSet::new();
    insert_terms(&mut signals, query);
    for scope in requested_scopes {
        insert_terms(&mut signals, scope);
    }
    for symbol in &context.symbols {
        if let Some(path) = symbol.get("path").and_then(Value::as_str) {
            insert_path_terms(&mut signals, path);
        }
        if let Some(name) = symbol.get("qualified_name").and_then(Value::as_str) {
            insert_terms(&mut signals, name);
        }
    }
    for item in &context.design_items {
        insert_terms(&mut signals, &item.id);
        insert_terms(&mut signals, &item.title);
        insert_terms(&mut signals, &item.summary);
        for values in item.relations.values() {
            for value in values {
                insert_terms(&mut signals, value);
            }
        }
    }
    signals
}

fn insert_path_terms(signals: &mut BTreeSet<String>, path: &str) {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    if normalized.len() <= 160 && normalized.contains('/') {
        signals.insert(normalized.clone());
    }
    for part in normalized.split(['/', '.', '-']) {
        insert_signal(signals, part);
    }
}

fn insert_terms(signals: &mut BTreeSet<String>, text: &str) {
    for term in text
        .to_ascii_lowercase()
        .split(|character: char| !character.is_alphanumeric() && character != '_')
    {
        insert_signal(signals, term);
    }
}

fn insert_signal(signals: &mut BTreeSet<String>, raw: &str) {
    let signal = raw.trim_matches('_');
    if signal.chars().count() < 3 || stop_signal(signal) {
        return;
    }
    signals.insert(signal.to_owned());
}

fn stop_signal(signal: &str) -> bool {
    matches!(
        signal,
        "src"
            | "rs"
            | "md"
            | "the"
            | "and"
            | "for"
            | "with"
            | "from"
            | "into"
            | "this"
            | "that"
            | "code"
            | "file"
            | "files"
            | "change"
            | "changes"
            | "update"
            | "inspect"
            | "project"
            | "repository"
    )
}

fn instruction_document(path: &str) -> bool {
    matches!(
        path,
        "AGENTS.md" | "CLAUDE.md" | ".github/copilot-instructions.md"
    )
}
