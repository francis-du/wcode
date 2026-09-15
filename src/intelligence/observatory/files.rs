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

pub(super) fn graph_file_lines(
    files: &BTreeMap<String, ProjectFileView>,
) -> BTreeMap<String, usize> {
    files
        .iter()
        .map(|(path, file)| (path.clone(), file.lines))
        .collect()
}

pub(super) fn code_stats(
    graph: &SoftwareGraphSnapshot,
    files: &BTreeMap<String, ProjectFileView>,
    review: Option<&ChangeReviewReport>,
) -> ProjectCodeStats {
    let mut source_files = 0usize;
    let mut source_lines = 0usize;
    let mut source_bytes = 0u64;
    let symbols = graph
        .graph
        .nodes
        .values()
        .filter(|node| {
            node.kind != NodeKind::File
                && node
                    .attributes
                    .get("path")
                    .and_then(serde_json::Value::as_str)
                    .is_some()
        })
        .count();
    let mut languages = BTreeMap::<String, (usize, usize)>::new();
    let mut product_scopes = BTreeMap::<String, (usize, usize)>::new();

    for file in files.values() {
        source_files = source_files.saturating_add(1);
        source_lines = source_lines.saturating_add(file.lines);
        source_bytes = source_bytes.saturating_add(file.bytes);
        let entry = languages.entry(file.language.clone()).or_default();
        entry.0 = entry.0.saturating_add(1);
        entry.1 = entry.1.saturating_add(file.lines);
        if let Some(scope) = scopes::source_scope(&file.path) {
            let entry = product_scopes.entry(scope.as_str().to_owned()).or_default();
            entry.0 = entry.0.saturating_add(1);
            entry.1 = entry.1.saturating_add(file.lines);
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

pub(super) fn project_files(graph: &SoftwareGraphSnapshot) -> BTreeMap<String, ProjectFileView> {
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
        let generated = node
            .attributes
            .get("generated_source")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or_else(|| crate::conventions::generated_source_path(path));
        let over_limit = lines > SOURCE_LINE_LIMIT && !generated;
        let depth = path.split('/').filter(|part| !part.is_empty()).count();
        files
            .entry(path.to_owned())
            .and_modify(|entry| {
                entry.lines = entry.lines.max(lines);
                entry.bytes = entry.bytes.max(bytes);
                entry.generated |= generated;
                entry.over_limit = entry.lines > SOURCE_LINE_LIMIT && !entry.generated;
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
                generated,
                over_limit,
            });
    }

    files
}

pub(super) fn build_project_structure(
    files: BTreeMap<String, ProjectFileView>,
    graph_truncated: bool,
) -> ProjectStructureView {
    let total_files = files.len();
    let mut directories = BTreeSet::new();
    let mut max_depth = 0usize;
    for entry in files.values() {
        max_depth = max_depth.max(entry.depth);
        let parts = entry.path.split('/').collect::<Vec<_>>();
        for end in 1..parts.len() {
            directories.insert(parts[..end].join("/"));
        }
    }
    let oversized_files = files.values().filter(|entry| entry.over_limit).count();
    // Rank the whole snapshot before bounding the tree. Clone only the winners.
    let mut largest_files = files.values().collect::<Vec<_>>();
    let compare = |left: &&ProjectFileView, right: &&ProjectFileView| {
        right
            .lines
            .cmp(&left.lines)
            .then_with(|| right.bytes.cmp(&left.bytes))
            .then_with(|| left.path.cmp(&right.path))
    };
    if largest_files.len() > MAX_LARGEST_FILES {
        largest_files.select_nth_unstable_by(MAX_LARGEST_FILES, compare);
        largest_files.truncate(MAX_LARGEST_FILES);
    }
    largest_files.sort_by(compare);
    let largest_files = largest_files.into_iter().cloned().collect();
    let entries = files.into_values().take(MAX_STRUCTURE_FILES).collect();

    ProjectStructureView {
        entries,
        largest_files,
        directory_count: directories.len(),
        max_depth,
        oversized_files,
        line_limit: SOURCE_LINE_LIMIT,
        truncated: graph_truncated || total_files > MAX_STRUCTURE_FILES,
    }
}

fn safe_relative_path(value: &str) -> bool {
    !value.is_empty()
        && !value.contains('\\')
        && Path::new(value)
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}
