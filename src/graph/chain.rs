use crate::graph::{EdgeKind, GraphPrecision, NodeKind};
use crate::graph_store::{
    load_selected, GraphChainInput, GraphChainMode, GraphChainNode, GraphChainResult,
};
use crate::workspace::{SearchMode, Workspace};
use anyhow::{bail, Result};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

pub(crate) fn query(workspace: &Workspace, input: &GraphChainInput) -> Result<GraphChainResult> {
    let stored = load_selected(workspace, input.snapshot_id.as_deref())?
        .ok_or_else(|| anyhow::anyhow!("no stored software graph snapshot is available"))?;
    let depth = input.depth.clamp(1, 4);
    let limit = input.limit.clamp(16, 240);
    let edge_limit = limit.saturating_mul(3);
    let query = input
        .node_id
        .clone()
        .or_else(|| input.label_contains.clone())
        .unwrap_or_default();
    if query.trim().is_empty() {
        bail!("code graph chain requires node_id or label_contains");
    }

    let mut root_ids = if let Some(node_id) = input.node_id.as_ref() {
        if stored
            .snapshot
            .graph
            .nodes
            .get(node_id)
            .is_some_and(|node| code_chain_node_kind(node.kind))
        {
            vec![node_id.clone()]
        } else {
            Vec::new()
        }
    } else {
        let needle = input
            .label_contains
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .to_ascii_lowercase();
        if needle.len() < 2 {
            bail!("code graph label query must contain at least two characters");
        }
        let mut matches = stored
            .snapshot
            .graph
            .nodes
            .values()
            .filter(|node| code_chain_node_kind(node.kind))
            .filter_map(|node| {
                let label = node.label.to_ascii_lowercase();
                let path = node
                    .attributes
                    .get("path")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                let score = if label == needle {
                    0
                } else if graph_label_leaf(&label) == needle {
                    1
                } else if label.starts_with(&needle) {
                    2
                } else if label.contains(&needle) {
                    3
                } else if path.contains(&needle) {
                    4
                } else {
                    return None;
                };
                Some((score, node.label.clone(), node.id.clone()))
            })
            .collect::<Vec<_>>();
        matches.sort();
        let direct = matches
            .into_iter()
            .take(8)
            .map(|(_, _, id)| id)
            .collect::<Vec<_>>();
        if direct.is_empty() {
            keyword_file_roots(workspace, &stored.snapshot.graph.nodes, &needle)
        } else {
            direct
        }
    };
    root_ids.sort();
    root_ids.dedup();
    if root_ids.is_empty() {
        bail!("no code graph symbol matches the requested chain root");
    }

    let root_set = root_ids.iter().cloned().collect::<BTreeSet<_>>();
    let mut state = HashMap::<String, (usize, bool, bool)>::new();
    let mut queue = VecDeque::<(String, i8)>::new();
    for root in &root_ids {
        state.insert(root.clone(), (0, false, false));
        queue.push_back((root.clone(), 0));
    }
    let mut edges = Vec::new();
    let mut edge_keys = HashSet::new();
    let mut truncated = false;

    while let Some((current, branch)) = queue.pop_front() {
        let current_distance = state.get(&current).map_or(0, |entry| entry.0);
        if current_distance >= depth {
            continue;
        }
        let current_kind = stored
            .snapshot
            .graph
            .nodes
            .get(&current)
            .map(|node| node.kind);
        for edge in &stored.snapshot.graph.edges {
            let structural_root_edge = root_set.contains(&current)
                && matches!(input.mode, GraphChainMode::Calls | GraphChainMode::Impact)
                && matches!(
                    current_kind,
                    Some(NodeKind::Package | NodeKind::Module | NodeKind::File)
                )
                && matches!(edge.kind, EdgeKind::Contains | EdgeKind::Defines)
                && edge.from == current;
            if !structural_root_edge && !chain_edge_allowed(input.mode, edge.kind) {
                continue;
            }
            let (neighbor, edge_direction) = if edge.from == current {
                (&edge.to, 1_i8)
            } else if edge.to == current {
                (&edge.from, -1_i8)
            } else {
                continue;
            };
            // Call-chain mode is directional after the first hop: upstream
            // follows callers-of-callers, downstream follows callees-of-callees.
            if input.mode == GraphChainMode::Calls && branch != 0 && edge_direction != branch {
                continue;
            }
            let Some(neighbor_node) = stored.snapshot.graph.nodes.get(neighbor) else {
                continue;
            };
            if !code_chain_node_kind(neighbor_node.kind) {
                continue;
            }
            if edge_keys.insert((edge.from.clone(), edge.to.clone(), edge.kind)) {
                if edges.len() >= edge_limit {
                    truncated = true;
                    continue;
                }
                edges.push(edge.clone());
            }
            if root_set.contains(neighbor) {
                continue;
            }
            let next_branch = if structural_root_edge {
                0
            } else if branch == 0 {
                edge_direction
            } else {
                branch
            };
            let upstream = next_branch < 0;
            let downstream = next_branch > 0;
            let next_distance = current_distance.saturating_add(1);
            if let Some(entry) = state.get_mut(neighbor) {
                let branch_added = (upstream && !entry.1) || (downstream && !entry.2);
                entry.1 |= upstream;
                entry.2 |= downstream;
                if branch_added && next_distance <= entry.0 {
                    queue.push_back((neighbor.clone(), next_branch));
                }
                continue;
            }
            if state.len() >= limit {
                truncated = true;
                continue;
            }
            state.insert(neighbor.clone(), (next_distance, upstream, downstream));
            queue.push_back((neighbor.clone(), next_branch));
        }
    }

    let mut nodes = state
        .into_iter()
        .filter_map(|(id, (distance, upstream, downstream))| {
            stored
                .snapshot
                .graph
                .nodes
                .get(&id)
                .cloned()
                .map(|node| GraphChainNode {
                    node,
                    distance,
                    upstream,
                    downstream,
                })
        })
        .collect::<Vec<_>>();
    nodes.sort_by(|left, right| {
        left.distance
            .cmp(&right.distance)
            .then_with(|| {
                root_set
                    .contains(&right.node.id)
                    .cmp(&root_set.contains(&left.node.id))
            })
            .then_with(|| left.node.label.cmp(&right.node.label))
    });
    let upstream_nodes = nodes.iter().filter(|node| node.upstream).count();
    let downstream_nodes = nodes.iter().filter(|node| node.downstream).count();
    let mut precision_counts = BTreeMap::new();
    for edge in &edges {
        *precision_counts
            .entry(graph_precision_name(edge.provenance.precision).to_owned())
            .or_insert(0) += 1;
    }

    Ok(GraphChainResult {
        snapshot_id: stored.id,
        captured_at_ms: stored.captured_at_ms,
        provider: stored.snapshot.provider,
        precision: stored.snapshot.precision,
        query,
        mode: match input.mode {
            GraphChainMode::Calls => "calls",
            GraphChainMode::Impact => "impact",
            GraphChainMode::All => "all",
        },
        depth,
        root_ids,
        nodes,
        edges,
        precision_counts,
        upstream_nodes,
        downstream_nodes,
        truncated,
    })
}

fn keyword_file_roots(
    workspace: &Workspace,
    nodes: &BTreeMap<String, crate::graph::GraphNode>,
    query: &str,
) -> Vec<String> {
    if query.len() < 2 {
        return Vec::new();
    }
    let pattern = format!("(?i:{})", regex::escape(query));
    let Ok((matches, _)) = workspace.search_with_options(&pattern, ".", 120, SearchMode::Regex, 0)
    else {
        return Vec::new();
    };
    let mut counts = BTreeMap::<String, usize>::new();
    for item in matches {
        let Some(path) = item.get("path").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let id = format!("file:{path}");
        if nodes
            .get(&id)
            .is_some_and(|node| node.kind == NodeKind::File)
        {
            *counts.entry(path.to_owned()).or_default() += 1;
        }
    }
    let mut ranked = counts.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|(left_path, left_count), (right_path, right_count)| {
        right_count
            .cmp(left_count)
            .then_with(|| left_path.cmp(right_path))
    });
    ranked
        .into_iter()
        .take(8)
        .map(|(path, _)| format!("file:{path}"))
        .collect()
}

fn code_chain_node_kind(kind: NodeKind) -> bool {
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

fn chain_edge_allowed(mode: GraphChainMode, kind: EdgeKind) -> bool {
    match mode {
        GraphChainMode::Calls => matches!(kind, EdgeKind::Calls | EdgeKind::RuntimeCalls),
        GraphChainMode::Impact => matches!(
            kind,
            EdgeKind::References
                | EdgeKind::Calls
                | EdgeKind::Imports
                | EdgeKind::DependsOn
                | EdgeKind::Implements
                | EdgeKind::Extends
                | EdgeKind::RuntimeCalls
        ),
        GraphChainMode::All => matches!(
            kind,
            EdgeKind::Contains
                | EdgeKind::Defines
                | EdgeKind::References
                | EdgeKind::Calls
                | EdgeKind::Imports
                | EdgeKind::DependsOn
                | EdgeKind::Implements
                | EdgeKind::Extends
                | EdgeKind::RuntimeCalls
        ),
    }
}

fn graph_label_leaf(label: &str) -> &str {
    label
        .rsplit([':', '.', '/', '#'])
        .find(|part| !part.is_empty())
        .unwrap_or(label)
}

const fn graph_precision_name(precision: GraphPrecision) -> &'static str {
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
