use crate::graph::{EdgeKind, NodeKind, SoftwareGraphSnapshot};
use crate::harness::ChangeReviewReport;
use crate::intelligence_types::{
    CodeStatBreakdown, ProjectCodeStats, ProjectFileView, ProjectStructureView,
};
use crate::scopes;
use crate::semantic_provider::language_for_path;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

const MAX_STRUCTURE_FILES: usize = 1_500;
const MAX_LARGEST_FILES: usize = 32;
const SOURCE_LINE_LIMIT: usize = 1_000;

pub(super) fn graph_file_lines(graph: &SoftwareGraphSnapshot) -> BTreeMap<String, usize> {
    graph
        .graph
        .nodes
        .values()
        .filter(|node| node.kind == NodeKind::File)
        .filter_map(|node| {
            let path = node
                .attributes
                .get("path")
                .and_then(serde_json::Value::as_str)?;
            let lines = node
                .attributes
                .get("line_count")
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or_default();
            Some((path.to_owned(), lines))
        })
        .collect()
}

pub(super) fn code_stats(
    graph: &SoftwareGraphSnapshot,
    review: Option<&ChangeReviewReport>,
) -> ProjectCodeStats {
    let mut source_files = 0usize;
    let mut source_lines = 0usize;
    let mut source_bytes = 0u64;
    let mut symbols = 0usize;
    let mut languages = BTreeMap::<String, (usize, usize)>::new();
    let mut product_scopes = BTreeMap::<String, (usize, usize)>::new();

    for node in graph.graph.nodes.values() {
        let path = node
            .attributes
            .get("path")
            .and_then(serde_json::Value::as_str);
        if node.kind == NodeKind::File {
            let Some(path) = path else { continue };
            source_files = source_files.saturating_add(1);
            let lines = node
                .attributes
                .get("line_count")
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or_default();
            let bytes = node
                .attributes
                .get("source_bytes")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_default();
            source_lines = source_lines.saturating_add(lines);
            source_bytes = source_bytes.saturating_add(bytes);
            let language = node
                .attributes
                .get("language")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown")
                .to_owned();
            let entry = languages.entry(language).or_default();
            entry.0 = entry.0.saturating_add(1);
            entry.1 = entry.1.saturating_add(lines);
            if let Some(scope) = scopes::source_scope(path) {
                let entry = product_scopes.entry(scope.as_str().to_owned()).or_default();
                entry.0 = entry.0.saturating_add(1);
                entry.1 = entry.1.saturating_add(lines);
            }
        } else if path.is_some() {
            symbols = symbols.saturating_add(1);
        }
    }

    let mut language_breakdown = breakdown(languages);
    language_breakdown.sort_by(|left, right| {
        right
            .lines
            .cmp(&left.lines)
            .then_with(|| right.files.cmp(&left.files))
            .then_with(|| left.name.cmp(&right.name))
    });
    let mut scope_breakdown = breakdown(product_scopes);
    scope_breakdown.sort_by(|left, right| {
        right
            .lines
            .cmp(&left.lines)
            .then_with(|| left.name.cmp(&right.name))
    });

    ProjectCodeStats {
        source_files,
        source_lines,
        source_bytes,
        symbols,
        call_edges: graph
            .graph
            .edges
            .iter()
            .filter(|edge| matches!(edge.kind, EdgeKind::Calls | EdgeKind::RuntimeCalls))
            .count(),
        languages: language_breakdown,
        product_scopes: scope_breakdown,
        changed_files: review
            .map(|review| review.files_changed)
            .unwrap_or_default(),
        changed_source_files: review
            .map(|review| {
                review
                    .files
                    .iter()
                    .filter(|file| language_for_path(&file.path).is_some())
                    .count()
            })
            .unwrap_or_default(),
        additions: review.map(|review| review.additions).unwrap_or_default(),
        deletions: review.map(|review| review.deletions).unwrap_or_default(),
        untracked_files: review
            .map(|review| review.untracked_files)
            .unwrap_or_default(),
        graph_truncated: graph.truncated || graph.scan_truncated,
    }
}

fn breakdown(values: BTreeMap<String, (usize, usize)>) -> Vec<CodeStatBreakdown> {
    values
        .into_iter()
        .map(|(name, (files, lines))| CodeStatBreakdown { name, files, lines })
        .collect()
}

pub(super) fn build_project_structure(graph: &SoftwareGraphSnapshot) -> ProjectStructureView {
    let mut files = BTreeMap::<String, ProjectFileView>::new();
    for node in graph
        .graph
        .nodes
        .values()
        .filter(|node| node.kind == NodeKind::File)
    {
        let Some(path) = node
            .attributes
            .get("path")
            .and_then(serde_json::Value::as_str)
            .filter(|path| safe_relative_path(path))
        else {
            continue;
        };
        let lines = node
            .attributes
            .get("line_count")
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or_default();
        let bytes = node
            .attributes
            .get("source_bytes")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_default();
        let language = node
            .attributes
            .get("language")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown")
            .to_owned();
        let depth = path.split('/').filter(|part| !part.is_empty()).count();
        files
            .entry(path.to_owned())
            .and_modify(|entry| {
                entry.lines = entry.lines.max(lines);
                entry.bytes = entry.bytes.max(bytes);
                entry.over_limit |= lines > SOURCE_LINE_LIMIT;
                if entry.language == "unknown" && language != "unknown" {
                    entry.language.clone_from(&language);
                }
            })
            .or_insert(ProjectFileView {
                path: path.to_owned(),
                language,
                lines,
                bytes,
                depth,
                over_limit: lines > SOURCE_LINE_LIMIT,
            });
    }

    let total_files = files.len();
    let entries = files
        .into_values()
        .take(MAX_STRUCTURE_FILES)
        .collect::<Vec<_>>();
    let mut directories = BTreeSet::new();
    let mut max_depth = 0usize;
    for entry in &entries {
        max_depth = max_depth.max(entry.depth);
        let parts = entry.path.split('/').collect::<Vec<_>>();
        for end in 1..parts.len() {
            directories.insert(parts[..end].join("/"));
        }
    }
    let oversized_files = entries.iter().filter(|entry| entry.over_limit).count();
    let mut largest_files = entries.clone();
    largest_files.sort_by(|left, right| {
        right
            .lines
            .cmp(&left.lines)
            .then_with(|| right.bytes.cmp(&left.bytes))
            .then_with(|| left.path.cmp(&right.path))
    });
    largest_files.truncate(MAX_LARGEST_FILES);

    ProjectStructureView {
        entries,
        largest_files,
        directory_count: directories.len(),
        max_depth,
        oversized_files,
        line_limit: SOURCE_LINE_LIMIT,
        truncated: graph.truncated || graph.scan_truncated || total_files > MAX_STRUCTURE_FILES,
    }
}

fn safe_relative_path(value: &str) -> bool {
    !value.is_empty()
        && !value.contains('\\')
        && Path::new(value)
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}
