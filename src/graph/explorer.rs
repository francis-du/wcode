use crate::graph::{EdgeKind, GraphNode, GraphPrecision, NodeKind};
use crate::graph_store::load_selected;
use crate::workspace::{SearchMode, Workspace};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct GraphSearchInput {
    pub snapshot_id: Option<String>,
    pub query: String,
    pub limit: usize,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct GraphSearchCandidate {
    pub node: GraphNode,
    pub match_kind: &'static str,
    pub score: usize,
    pub relations: usize,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct GraphSearchResult {
    pub snapshot_id: String,
    pub captured_at_ms: u64,
    pub provider: String,
    pub precision: GraphPrecision,
    pub query: String,
    pub results: Vec<GraphSearchCandidate>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct GraphOverviewInput {
    pub snapshot_id: Option<String>,
    pub limit: usize,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct GraphOverviewNode {
    pub id: String,
    pub label: String,
    pub path: String,
    pub language: String,
    pub symbols: usize,
    pub degree: usize,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct GraphOverviewEdge {
    pub from: String,
    pub to: String,
    pub count: usize,
    pub kinds: BTreeMap<String, usize>,
    pub precision: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct GraphOverviewResult {
    pub snapshot_id: String,
    pub captured_at_ms: u64,
    pub provider: String,
    pub precision: GraphPrecision,
    pub files_considered: usize,
    pub files_indexed: usize,
    pub files_failed: usize,
    pub scan_truncated: bool,
    pub graph_truncated: bool,
    pub total_nodes: usize,
    pub total_edges: usize,
    pub total_files: usize,
    pub languages: BTreeMap<String, usize>,
    pub relation_counts: BTreeMap<String, usize>,
    pub nodes: Vec<GraphOverviewNode>,
    pub edges: Vec<GraphOverviewEdge>,
    pub truncated: bool,
}

pub(crate) fn search(workspace: &Workspace, input: &GraphSearchInput) -> Result<GraphSearchResult> {
    let stored = load_selected(workspace, input.snapshot_id.as_deref())?
        .ok_or_else(|| anyhow::anyhow!("no stored software graph snapshot is available"))?;
    let query = input.query.trim();
    if query.len() < 2 {
        bail!("code graph search query must contain at least two characters");
    }
    let needle = query.to_ascii_lowercase();
    let relation_counts = node_relation_counts(&stored.snapshot.graph.edges);
    let limit = input.limit.clamp(1, 40);
    let mut matches = stored
        .snapshot
        .graph
        .nodes
        .values()
        .filter(|node| code_node_kind(node.kind))
        .filter_map(|node| {
            let label = node.label.to_ascii_lowercase();
            let path = node_path(node).unwrap_or_default().to_ascii_lowercase();
            let leaf = graph_label_leaf(&label);
            let (score, match_kind) = if label == needle {
                (0, "exact_label")
            } else if path == needle {
                (0, "exact_path")
            } else if leaf == needle {
                (1, "exact_leaf")
            } else if label.starts_with(&needle) {
                (2, "label_prefix")
            } else if path.ends_with(&needle) {
                (2, "path_suffix")
            } else if label.contains(&needle) {
                (3, "label_contains")
            } else if path.contains(&needle) {
                (4, "path_contains")
            } else {
                return None;
            };
            let relations = relation_counts.get(&node.id).copied().unwrap_or(0);
            Some((
                score,
                Reverse(relations),
                node.label.clone(),
                node.id.clone(),
                match_kind,
            ))
        })
        .collect::<Vec<_>>();
    matches.sort();

    let direct_count = matches.len();
    let mut candidates = matches
        .into_iter()
        .take(limit)
        .filter_map(|(score, Reverse(relations), _, id, match_kind)| {
            stored
                .snapshot
                .graph
                .nodes
                .get(&id)
                .cloned()
                .map(|node| GraphSearchCandidate {
                    node,
                    match_kind,
                    score,
                    relations,
                })
        })
        .collect::<Vec<_>>();

    if candidates.is_empty() {
        let pattern = format!("(?i:{})", regex::escape(query));
        if let Ok((results, _)) = workspace.search_with_options(
            &pattern,
            ".",
            limit.saturating_mul(8),
            SearchMode::Regex,
            0,
        ) {
            let mut file_hits = BTreeMap::<String, usize>::new();
            for result in results {
                if let Some(path) = result.get("path").and_then(serde_json::Value::as_str) {
                    let id = format!("file:{path}");
                    if stored
                        .snapshot
                        .graph
                        .nodes
                        .get(&id)
                        .is_some_and(|node| node.kind == NodeKind::File)
                    {
                        *file_hits.entry(id).or_default() += 1;
                    }
                }
            }
            let mut ranked = file_hits.into_iter().collect::<Vec<_>>();
            ranked.sort_by_key(|(id, hits)| (Reverse(*hits), id.clone()));
            candidates.extend(ranked.into_iter().take(limit).filter_map(|(id, hits)| {
                stored.snapshot.graph.nodes.get(&id).cloned().map(|node| {
                    let relations = relation_counts.get(&id).copied().unwrap_or(0);
                    GraphSearchCandidate {
                        node,
                        match_kind: "source_content",
                        score: 5,
                        relations: relations.saturating_add(hits),
                    }
                })
            }));
        }
    }

    Ok(GraphSearchResult {
        snapshot_id: stored.id,
        captured_at_ms: stored.captured_at_ms,
        provider: stored.snapshot.provider,
        precision: stored.snapshot.precision,
        query: query.to_owned(),
        truncated: direct_count > candidates.len(),
        results: candidates,
    })
}

pub(crate) fn overview(
    workspace: &Workspace,
    input: &GraphOverviewInput,
) -> Result<GraphOverviewResult> {
    let stored = load_selected(workspace, input.snapshot_id.as_deref())?
        .ok_or_else(|| anyhow::anyhow!("no stored software graph snapshot is available"))?;
    let graph = &stored.snapshot.graph;
    let limit = input.limit.clamp(16, 320);
    let mut file_nodes = BTreeMap::<String, &GraphNode>::new();
    let mut symbol_counts = BTreeMap::<String, usize>::new();
    let mut languages = BTreeMap::<String, usize>::new();
    for node in graph.nodes.values() {
        let Some(path) = node_path(node) else {
            continue;
        };
        if node.kind == NodeKind::File {
            file_nodes.insert(path.to_owned(), node);
            let language = node
                .attributes
                .get("language")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown");
            *languages.entry(language.to_owned()).or_default() += 1;
        } else if code_node_kind(node.kind) {
            *symbol_counts.entry(path.to_owned()).or_default() += 1;
        }
    }

    #[derive(Default)]
    struct EdgeAggregate {
        count: usize,
        kinds: BTreeMap<String, usize>,
        precision: BTreeMap<String, usize>,
    }

    let mut relation_counts = BTreeMap::<String, usize>::new();
    let mut aggregates = BTreeMap::<(String, String), EdgeAggregate>::new();
    let mut degree = HashMap::<String, usize>::new();
    for edge in &graph.edges {
        *relation_counts
            .entry(edge_kind_name(edge.kind).to_owned())
            .or_default() += 1;
        let Some(from) = graph.nodes.get(&edge.from).and_then(node_path) else {
            continue;
        };
        let Some(to) = graph.nodes.get(&edge.to).and_then(node_path) else {
            continue;
        };
        if from == to || !file_nodes.contains_key(from) || !file_nodes.contains_key(to) {
            continue;
        }
        let aggregate = aggregates
            .entry((from.to_owned(), to.to_owned()))
            .or_default();
        aggregate.count += 1;
        *aggregate
            .kinds
            .entry(edge_kind_name(edge.kind).to_owned())
            .or_default() += 1;
        *aggregate
            .precision
            .entry(precision_name(edge.provenance.precision).to_owned())
            .or_default() += 1;
        *degree.entry(from.to_owned()).or_default() += 1;
        *degree.entry(to.to_owned()).or_default() += 1;
    }

    let total_files = file_nodes.len();
    let mut ranked = file_nodes
        .keys()
        .map(|path| {
            (
                Reverse(degree.get(path).copied().unwrap_or(0)),
                Reverse(symbol_counts.get(path).copied().unwrap_or(0)),
                path.clone(),
            )
        })
        .collect::<Vec<_>>();
    ranked.sort();
    let selected = ranked
        .into_iter()
        .take(limit)
        .map(|(_, _, path)| path)
        .collect::<BTreeSet<_>>();

    let nodes = selected
        .iter()
        .filter_map(|path| {
            let node = file_nodes.get(path)?;
            Some(GraphOverviewNode {
                id: node.id.clone(),
                label: node.label.clone(),
                path: path.clone(),
                language: node
                    .attributes
                    .get("language")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unknown")
                    .to_owned(),
                symbols: symbol_counts.get(path).copied().unwrap_or(0),
                degree: degree.get(path).copied().unwrap_or(0),
            })
        })
        .collect::<Vec<_>>();

    let edges = aggregates
        .into_iter()
        .filter(|((from, to), _)| selected.contains(from) && selected.contains(to))
        .map(|((from, to), aggregate)| GraphOverviewEdge {
            from: format!("file:{from}"),
            to: format!("file:{to}"),
            count: aggregate.count,
            kinds: aggregate.kinds,
            precision: aggregate.precision,
        })
        .collect::<Vec<_>>();

    Ok(GraphOverviewResult {
        snapshot_id: stored.id,
        captured_at_ms: stored.captured_at_ms,
        provider: stored.snapshot.provider,
        precision: stored.snapshot.precision,
        files_considered: stored.snapshot.files_considered,
        files_indexed: stored.snapshot.files_indexed,
        files_failed: stored.snapshot.files_failed,
        scan_truncated: stored.snapshot.scan_truncated,
        graph_truncated: stored.snapshot.truncated,
        total_nodes: stored.snapshot.node_count,
        total_edges: stored.snapshot.edge_count,
        total_files,
        languages,
        relation_counts,
        truncated: stored.snapshot.truncated
            || stored.snapshot.scan_truncated
            || total_files > nodes.len(),
        nodes,
        edges,
    })
}

fn node_path(node: &GraphNode) -> Option<&str> {
    node.attributes
        .get("path")
        .and_then(serde_json::Value::as_str)
        .or_else(|| node.id.strip_prefix("file:"))
}

fn node_relation_counts(edges: &[crate::graph::GraphEdge]) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for edge in edges {
        *counts.entry(edge.from.clone()).or_default() += 1;
        *counts.entry(edge.to.clone()).or_default() += 1;
    }
    counts
}

fn code_node_kind(kind: NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::Package
            | NodeKind::Module
            | NodeKind::File
            | NodeKind::Symbol
            | NodeKind::Function
            | NodeKind::Struct
            | NodeKind::Trait
            | NodeKind::Class
            | NodeKind::Enum
            | NodeKind::Interface
            | NodeKind::Api
            | NodeKind::Test
    )
}

fn graph_label_leaf(label: &str) -> &str {
    label
        .rsplit([':', '.', '/', '#'])
        .find(|part| !part.is_empty())
        .unwrap_or(label)
}

const fn edge_kind_name(kind: EdgeKind) -> &'static str {
    match kind {
        EdgeKind::Contains => "contains",
        EdgeKind::Defines => "defines",
        EdgeKind::References => "references",
        EdgeKind::Calls => "calls",
        EdgeKind::Imports => "imports",
        EdgeKind::DependsOn => "depends_on",
        EdgeKind::Implements => "implements",
        EdgeKind::Extends => "extends",
        EdgeKind::ImplementsRequirement => "implements_requirement",
        EdgeKind::ConstrainedBy => "constrained_by",
        EdgeKind::TestedBy => "tested_by",
        EdgeKind::VerifiedBy => "verified_by",
        EdgeKind::GuardsAgainst => "guards_against",
        EdgeKind::ProducesEvidence => "produces_evidence",
        EdgeKind::RuntimeCalls => "runtime_calls",
        EdgeKind::ConflictsWith => "conflicts_with",
    }
}

const fn precision_name(precision: GraphPrecision) -> &'static str {
    match precision {
        GraphPrecision::Declared => "declared",
        GraphPrecision::Syntax => "syntax",
        GraphPrecision::Semantic => "semantic",
        GraphPrecision::Runtime => "runtime",
        GraphPrecision::Deterministic => "deterministic",
        GraphPrecision::Heuristic => "heuristic",
        GraphPrecision::Mixed => "mixed",
    }
}
#[cfg(test)]
#[path = "../../tests/unit/graph/explorer.rs"]
mod tests;
