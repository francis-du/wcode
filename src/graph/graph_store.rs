use crate::evidence_store::workspace_state_directory;
use crate::graph::{
    EdgeKind, GraphEdge, GraphNode, GraphPrecision, NodeKind, SoftwareGraphSnapshot,
};
use crate::workspace::Workspace;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_GRAPH_SNAPSHOTS: usize = 64;
const MAX_GRAPH_SNAPSHOT_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StoredGraphSnapshot {
    pub id: String,
    pub captured_at_ms: u64,
    pub snapshot: SoftwareGraphSnapshot,
}

#[derive(Clone, Debug, Serialize)]
pub struct GraphHistoryEntry {
    pub id: String,
    pub captured_at_ms: u64,
    pub provider: String,
    pub precision: crate::graph::GraphPrecision,
    pub path: String,
    pub nodes: usize,
    pub edges: usize,
    pub files_indexed: usize,
    pub truncated: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum GraphDirection {
    Incoming,
    Outgoing,
    Both,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphQueryInput {
    #[serde(default)]
    pub snapshot_id: Option<String>,
    #[serde(default)]
    pub node_id: Option<String>,
    #[serde(default)]
    pub kind: Option<NodeKind>,
    #[serde(default)]
    pub label_contains: Option<String>,
    #[serde(default)]
    pub related_to: Option<String>,
    #[serde(default)]
    pub edge_kind: Option<EdgeKind>,
    #[serde(default)]
    pub direction: Option<GraphDirection>,
    #[serde(default = "default_query_limit")]
    pub limit: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct GraphQueryResult {
    pub snapshot_id: String,
    pub captured_at_ms: u64,
    pub provider: String,
    pub precision: GraphPrecision,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphDiffInput {
    #[serde(default)]
    pub from_snapshot_id: Option<String>,
    #[serde(default)]
    pub to_snapshot_id: Option<String>,
    #[serde(default = "default_diff_limit")]
    pub limit: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct GraphNodeChange {
    pub before: GraphNode,
    pub after: GraphNode,
}

#[derive(Clone, Debug, Serialize)]
pub struct GraphEdgeChange {
    pub before: GraphEdge,
    pub after: GraphEdge,
}

#[derive(Clone, Debug, Serialize)]
pub struct GraphDiffResult {
    pub from_snapshot_id: String,
    pub to_snapshot_id: String,
    pub from_captured_at_ms: u64,
    pub to_captured_at_ms: u64,
    pub added_node_count: usize,
    pub removed_node_count: usize,
    pub changed_node_count: usize,
    pub added_edge_count: usize,
    pub removed_edge_count: usize,
    pub changed_edge_count: usize,
    pub added_nodes: Vec<GraphNode>,
    pub removed_nodes: Vec<GraphNode>,
    pub changed_nodes: Vec<GraphNodeChange>,
    pub added_edges: Vec<GraphEdge>,
    pub removed_edges: Vec<GraphEdge>,
    pub changed_edges: Vec<GraphEdgeChange>,
    pub truncated: bool,
}

pub(crate) fn persist(
    workspace: &Workspace,
    snapshot: &SoftwareGraphSnapshot,
) -> Result<StoredGraphSnapshot> {
    snapshot.graph.validate()?;
    let snapshot_bytes = serde_json::to_vec(snapshot).context("cannot encode graph snapshot")?;
    if snapshot_bytes.len() as u64 > MAX_GRAPH_SNAPSHOT_BYTES {
        bail!("software graph snapshot exceeds the persistent store size bound");
    }
    let digest = digest_bytes(&snapshot_bytes);
    let stored = StoredGraphSnapshot {
        id: format!("GRAPH-{}", &digest[..24]),
        captured_at_ms: now_ms(),
        snapshot: snapshot.clone(),
    };
    let bytes = serde_json::to_vec(&stored).context("cannot encode stored graph snapshot")?;
    let directory = graph_directory(workspace)?;
    fs::create_dir_all(&directory)
        .with_context(|| format!("cannot create graph history store {}", directory.display()))?;
    for existing_path in graph_paths(&directory)?.into_iter().rev() {
        if let Some(existing) = read_snapshot(&existing_path)? {
            if existing.id == stored.id {
                return Ok(existing);
            }
        }
    }
    let path = directory.join(format!("{:020}-{}.json", stored.captured_at_ms, stored.id));
    if !path.exists() {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&path)
            .with_context(|| format!("cannot create graph snapshot {}", path.display()))?;
        file.write_all(&bytes)
            .with_context(|| format!("cannot write graph snapshot {}", path.display()))?;
        file.sync_all()
            .with_context(|| format!("cannot sync graph snapshot {}", path.display()))?;
        prune_directory(&directory)?;
    }
    Ok(stored)
}

pub(crate) fn history(workspace: &Workspace, limit: usize) -> Result<Vec<GraphHistoryEntry>> {
    let directory = graph_directory(workspace)?;
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    for path in graph_paths(&directory)?.into_iter().rev() {
        if let Some(stored) = read_snapshot(&path)? {
            entries.push(history_entry(&stored));
            if entries.len() >= limit.clamp(1, MAX_GRAPH_SNAPSHOTS) {
                break;
            }
        }
    }
    Ok(entries)
}

pub(crate) fn query(workspace: &Workspace, input: &GraphQueryInput) -> Result<GraphQueryResult> {
    let stored = load_selected(workspace, input.snapshot_id.as_deref())?
        .ok_or_else(|| anyhow::anyhow!("no stored software graph snapshot is available"))?;
    let limit = input.limit.clamp(1, 500);
    let label = input
        .label_contains
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase);
    let mut selected = BTreeSet::new();
    for node in stored.snapshot.graph.nodes.values() {
        if input.node_id.as_ref().is_some_and(|id| id != &node.id)
            || input.kind.is_some_and(|kind| kind != node.kind)
            || label
                .as_ref()
                .is_some_and(|needle| !node.label.to_ascii_lowercase().contains(needle))
        {
            continue;
        }
        selected.insert(node.id.clone());
        if selected.len() >= limit {
            break;
        }
    }

    let mut edges = Vec::new();
    if let Some(related_to) = input.related_to.as_deref() {
        let direction = input.direction.unwrap_or(GraphDirection::Both);
        for edge in &stored.snapshot.graph.edges {
            if input.edge_kind.is_some_and(|kind| kind != edge.kind) {
                continue;
            }
            let matches = match direction {
                GraphDirection::Incoming => edge.to == related_to,
                GraphDirection::Outgoing => edge.from == related_to,
                GraphDirection::Both => edge.from == related_to || edge.to == related_to,
            };
            if !matches {
                continue;
            }
            selected.insert(edge.from.clone());
            selected.insert(edge.to.clone());
            edges.push(edge.clone());
            if edges.len() >= limit {
                break;
            }
        }
    } else {
        for edge in &stored.snapshot.graph.edges {
            if input.edge_kind.is_some_and(|kind| kind != edge.kind)
                || (!selected.is_empty()
                    && !selected.contains(&edge.from)
                    && !selected.contains(&edge.to))
            {
                continue;
            }
            edges.push(edge.clone());
            if edges.len() >= limit {
                break;
            }
        }
    }

    if selected.is_empty()
        && input.node_id.is_none()
        && input.kind.is_none()
        && label.is_none()
        && input.related_to.is_none()
    {
        selected.extend(stored.snapshot.graph.nodes.keys().take(limit).cloned());
    }
    let nodes = selected
        .iter()
        .filter_map(|id| stored.snapshot.graph.nodes.get(id).cloned())
        .take(limit)
        .collect::<Vec<_>>();
    let truncated = nodes.len() >= limit
        || edges.len() >= limit
        || stored.snapshot.graph.nodes.len() > nodes.len()
            && input.node_id.is_none()
            && input.kind.is_none()
            && label.is_none();
    Ok(GraphQueryResult {
        snapshot_id: stored.id,
        captured_at_ms: stored.captured_at_ms,
        provider: stored.snapshot.provider,
        precision: stored.snapshot.precision,
        nodes,
        edges,
        truncated,
    })
}

pub(crate) fn diff(workspace: &Workspace, input: &GraphDiffInput) -> Result<GraphDiffResult> {
    let (from, to) = select_diff_snapshots(workspace, input)?;
    let limit = input.limit.clamp(1, 200);

    let mut added_nodes = Vec::new();
    let mut changed_nodes = Vec::new();
    for (id, after) in &to.snapshot.graph.nodes {
        match from.snapshot.graph.nodes.get(id) {
            None => added_nodes.push(after.clone()),
            Some(before) if before != after => changed_nodes.push(GraphNodeChange {
                before: before.clone(),
                after: after.clone(),
            }),
            Some(_) => {}
        }
    }
    let mut removed_nodes = from
        .snapshot
        .graph
        .nodes
        .iter()
        .filter(|(id, _)| !to.snapshot.graph.nodes.contains_key(*id))
        .map(|(_, node)| node.clone())
        .collect::<Vec<_>>();

    let from_edges = grouped_edges(&from.snapshot.graph.edges);
    let mut to_edges = grouped_edges(&to.snapshot.graph.edges);
    let mut added_edges = Vec::new();
    let mut removed_edges = Vec::new();
    let mut changed_edges = Vec::new();
    for (identity, before_group) in from_edges {
        let after_group = to_edges.remove(&identity).unwrap_or_default();
        diff_edge_group(
            before_group,
            after_group,
            &mut added_edges,
            &mut removed_edges,
            &mut changed_edges,
        );
    }
    for after_group in to_edges.into_values() {
        added_edges.extend(after_group);
    }

    added_nodes.sort_by(|left, right| left.id.cmp(&right.id));
    removed_nodes.sort_by(|left, right| left.id.cmp(&right.id));
    changed_nodes.sort_by(|left, right| left.before.id.cmp(&right.before.id));
    added_edges.sort_by(graph_edge_order);
    removed_edges.sort_by(graph_edge_order);
    changed_edges.sort_by(|left, right| graph_edge_order(&left.before, &right.before));

    let added_node_count = added_nodes.len();
    let removed_node_count = removed_nodes.len();
    let changed_node_count = changed_nodes.len();
    let added_edge_count = added_edges.len();
    let removed_edge_count = removed_edges.len();
    let changed_edge_count = changed_edges.len();
    added_nodes.truncate(limit);
    removed_nodes.truncate(limit);
    changed_nodes.truncate(limit);
    added_edges.truncate(limit);
    removed_edges.truncate(limit);
    changed_edges.truncate(limit);
    let truncated = added_node_count > added_nodes.len()
        || removed_node_count > removed_nodes.len()
        || changed_node_count > changed_nodes.len()
        || added_edge_count > added_edges.len()
        || removed_edge_count > removed_edges.len()
        || changed_edge_count > changed_edges.len();

    Ok(GraphDiffResult {
        from_snapshot_id: from.id.clone(),
        to_snapshot_id: to.id.clone(),
        from_captured_at_ms: from.captured_at_ms,
        to_captured_at_ms: to.captured_at_ms,
        added_node_count,
        removed_node_count,
        changed_node_count,
        added_edge_count,
        removed_edge_count,
        changed_edge_count,
        added_nodes,
        removed_nodes,
        changed_nodes,
        added_edges,
        removed_edges,
        changed_edges,
        truncated,
    })
}

type EdgeIdentity = (String, String, EdgeKind, String, GraphPrecision);

fn edge_identity(edge: &GraphEdge) -> EdgeIdentity {
    (
        edge.from.clone(),
        edge.to.clone(),
        edge.kind,
        edge.provenance.provider.clone(),
        edge.provenance.precision,
    )
}

fn grouped_edges(edges: &[GraphEdge]) -> HashMap<EdgeIdentity, Vec<GraphEdge>> {
    let mut groups = HashMap::<EdgeIdentity, Vec<GraphEdge>>::new();
    for edge in edges {
        groups
            .entry(edge_identity(edge))
            .or_default()
            .push(edge.clone());
    }
    groups
}

fn diff_edge_group(
    before: Vec<GraphEdge>,
    after: Vec<GraphEdge>,
    added: &mut Vec<GraphEdge>,
    removed: &mut Vec<GraphEdge>,
    changed: &mut Vec<GraphEdgeChange>,
) {
    let after_revisions = after
        .iter()
        .map(|edge| edge.provenance.revision.clone())
        .collect::<BTreeSet<_>>();
    let before_revisions = before
        .iter()
        .map(|edge| edge.provenance.revision.clone())
        .collect::<BTreeSet<_>>();
    let mut unmatched_before = before
        .into_iter()
        .filter(|edge| !after_revisions.contains(edge.provenance.revision.as_str()))
        .collect::<Vec<_>>();
    let mut unmatched_after = after
        .into_iter()
        .filter(|edge| !before_revisions.contains(edge.provenance.revision.as_str()))
        .collect::<Vec<_>>();
    unmatched_before.sort_by(graph_edge_order);
    unmatched_after.sort_by(graph_edge_order);
    let paired = unmatched_before.len().min(unmatched_after.len());
    for (before, after) in unmatched_before
        .drain(..paired)
        .zip(unmatched_after.drain(..paired))
    {
        changed.push(GraphEdgeChange { before, after });
    }
    removed.extend(unmatched_before);
    added.extend(unmatched_after);
}

fn graph_edge_order(left: &GraphEdge, right: &GraphEdge) -> std::cmp::Ordering {
    left.from
        .cmp(&right.from)
        .then_with(|| left.to.cmp(&right.to))
        .then_with(|| format!("{:?}", left.kind).cmp(&format!("{:?}", right.kind)))
        .then_with(|| left.provenance.provider.cmp(&right.provenance.provider))
        .then_with(|| left.provenance.revision.cmp(&right.provenance.revision))
}

fn select_diff_snapshots(
    workspace: &Workspace,
    input: &GraphDiffInput,
) -> Result<(StoredGraphSnapshot, StoredGraphSnapshot)> {
    let directory = graph_directory(workspace)?;
    if !directory.exists() {
        bail!("no stored software graph snapshot is available");
    }
    let paths = graph_paths(&directory)?;
    match (
        input.from_snapshot_id.as_deref(),
        input.to_snapshot_id.as_deref(),
    ) {
        (Some(from_id), Some(to_id)) => Ok((
            load_snapshot_by_id(&paths, from_id, "from_snapshot_id")?,
            load_snapshot_by_id(&paths, to_id, "to_snapshot_id")?,
        )),
        (Some(from_id), None) => {
            let from = load_snapshot_by_id(&paths, from_id, "from_snapshot_id")?;
            let to = latest_valid_snapshot(&paths)?;
            if from.id == to.id {
                bail!("from_snapshot_id is already the latest graph revision");
            }
            Ok((from, to))
        }
        (None, Some(to_id)) => {
            let (to_index, to) = load_snapshot_by_id_with_index(&paths, to_id, "to_snapshot_id")?;
            let from = previous_valid_snapshot(&paths[..to_index])?.ok_or_else(|| {
                anyhow::anyhow!("selected graph snapshot has no previous revision")
            })?;
            Ok((from, to))
        }
        (None, None) => {
            let mut latest = paths
                .iter()
                .rev()
                .filter_map(|path| read_snapshot(path).transpose());
            let to = latest
                .next()
                .transpose()?
                .ok_or_else(|| anyhow::anyhow!("no stored software graph snapshot is available"))?;
            let from = latest.next().transpose()?.ok_or_else(|| {
                anyhow::anyhow!(
                    "at least two stored software graph snapshots are required for an implicit diff"
                )
            })?;
            Ok((from, to))
        }
    }
}

fn load_snapshot_by_id(paths: &[PathBuf], id: &str, field: &str) -> Result<StoredGraphSnapshot> {
    load_snapshot_by_id_with_index(paths, id, field).map(|(_, snapshot)| snapshot)
}

fn load_snapshot_by_id_with_index(
    paths: &[PathBuf],
    id: &str,
    field: &str,
) -> Result<(usize, StoredGraphSnapshot)> {
    let suffix = format!("-{id}.json");
    let Some((index, path)) = paths.iter().enumerate().find(|(_, path)| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(&suffix))
    }) else {
        bail!("{field} does not exist");
    };
    let Some(snapshot) = read_snapshot(path)? else {
        bail!("{field} does not exist");
    };
    if snapshot.id != id {
        bail!("{field} does not exist");
    }
    Ok((index, snapshot))
}

fn latest_valid_snapshot(paths: &[PathBuf]) -> Result<StoredGraphSnapshot> {
    previous_valid_snapshot(paths)?
        .ok_or_else(|| anyhow::anyhow!("no stored software graph snapshot is available"))
}

fn previous_valid_snapshot(paths: &[PathBuf]) -> Result<Option<StoredGraphSnapshot>> {
    for path in paths.iter().rev() {
        if let Some(snapshot) = read_snapshot(path)? {
            return Ok(Some(snapshot));
        }
    }
    Ok(None)
}

pub(crate) fn capabilities() -> serde_json::Value {
    serde_json::json!({
        "persistent": true,
        "format": "bounded-composite-graph-snapshots",
        "scope": "per-workspace",
        "max_snapshots": MAX_GRAPH_SNAPSHOTS,
        "max_snapshot_bytes": MAX_GRAPH_SNAPSHOT_BYTES,
        "query_limit": 500,
        "diff_limit_per_category": 200
    })
}

fn load_selected(workspace: &Workspace, id: Option<&str>) -> Result<Option<StoredGraphSnapshot>> {
    let directory = graph_directory(workspace)?;
    if !directory.exists() {
        return Ok(None);
    }
    for path in graph_paths(&directory)?.into_iter().rev() {
        let Some(stored) = read_snapshot(&path)? else {
            continue;
        };
        if id.is_none_or(|id| id == stored.id) {
            return Ok(Some(stored));
        }
    }
    Ok(None)
}

fn graph_directory(workspace: &Workspace) -> Result<PathBuf> {
    Ok(workspace_state_directory(workspace)?.join("graph-history"))
}

fn graph_paths(directory: &Path) -> Result<Vec<PathBuf>> {
    let metadata = fs::symlink_metadata(directory)
        .with_context(|| format!("cannot inspect graph history store {}", directory.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("graph history store path is not a regular directory");
    }
    let mut paths = fs::read_dir(directory)
        .with_context(|| format!("cannot list graph history store {}", directory.display()))?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_str()?;
            name.ends_with(".json").then(|| entry.path())
        })
        .collect::<Vec<_>>();
    paths.sort();
    Ok(paths)
}

fn read_snapshot(path: &Path) -> Result<Option<StoredGraphSnapshot>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => return Ok(None),
    };
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_GRAPH_SNAPSHOT_BYTES.saturating_add(512 * 1024)
    {
        return Ok(None);
    }
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(_) => return Ok(None),
    };
    let stored: StoredGraphSnapshot = match serde_json::from_slice(&bytes) {
        Ok(stored) => stored,
        Err(_) => return Ok(None),
    };
    if stored.snapshot.graph.validate().is_err() {
        return Ok(None);
    }
    Ok(Some(stored))
}

fn history_entry(stored: &StoredGraphSnapshot) -> GraphHistoryEntry {
    GraphHistoryEntry {
        id: stored.id.clone(),
        captured_at_ms: stored.captured_at_ms,
        provider: stored.snapshot.provider.clone(),
        precision: stored.snapshot.precision,
        path: stored.snapshot.path.clone(),
        nodes: stored.snapshot.node_count,
        edges: stored.snapshot.edge_count,
        files_indexed: stored.snapshot.files_indexed,
        truncated: stored.snapshot.truncated || stored.snapshot.scan_truncated,
    }
}

fn prune_directory(directory: &Path) -> Result<()> {
    let paths = graph_paths(directory)?;
    let excess = paths.len().saturating_sub(MAX_GRAPH_SNAPSHOTS);
    for path in paths.into_iter().take(excess) {
        let _ = fs::remove_file(path);
    }
    Ok(())
}

fn digest_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

const fn default_query_limit() -> usize {
    100
}

const fn default_diff_limit() -> usize {
    50
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{GraphPrecision, SoftwareGraph};
    use std::collections::BTreeMap;

    #[test]
    fn graph_history_round_trips_and_queries_nodes() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(dir.path(), false, false).unwrap();
        let mut graph = SoftwareGraph::default();
        graph
            .add_node(GraphNode {
                id: "component:auth".into(),
                kind: NodeKind::Component,
                label: "Authentication".into(),
                attributes: BTreeMap::new(),
                provenance: crate::graph::GraphProvenance {
                    provider: "design".into(),
                    precision: GraphPrecision::Declared,
                    revision: "design:1".into(),
                },
            })
            .unwrap();
        let snapshot = SoftwareGraphSnapshot {
            workspace: "demo".into(),
            path: ".".into(),
            provider: "wcode-composite".into(),
            precision: GraphPrecision::Mixed,
            files_considered: 0,
            files_indexed: 0,
            files_failed: 0,
            scan_truncated: false,
            truncated: false,
            node_count: 1,
            edge_count: 0,
            failures: vec![],
            graph,
        };
        let stored = persist(&workspace, &snapshot).unwrap();
        assert_eq!(history(&workspace, 10).unwrap().len(), 1);
        let result = query(
            &workspace,
            &GraphQueryInput {
                snapshot_id: Some(stored.id.clone()),
                node_id: None,
                kind: Some(NodeKind::Component),
                label_contains: Some("auth".into()),
                related_to: None,
                edge_kind: None,
                direction: None,
                limit: 10,
            },
        )
        .unwrap();
        assert_eq!(result.snapshot_id, stored.id);
        assert_eq!(result.nodes.len(), 1);
    }

    #[test]
    fn graph_diff_separates_structural_changes_from_revision_churn() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(dir.path(), false, false).unwrap();
        let provenance = |revision: &str| crate::graph::GraphProvenance {
            provider: "lsp:fixture".into(),
            precision: GraphPrecision::Semantic,
            revision: revision.into(),
        };
        let node = |id: &str, label: &str, revision: &str| GraphNode {
            id: id.into(),
            kind: NodeKind::Function,
            label: label.into(),
            attributes: BTreeMap::new(),
            provenance: provenance(revision),
        };

        let mut before_graph = SoftwareGraph::default();
        before_graph
            .add_node(node("function:a", "a", "semantic:1"))
            .unwrap();
        before_graph
            .add_node(node("function:b", "b", "semantic:1"))
            .unwrap();
        before_graph
            .add_node(node("function:removed", "removed", "semantic:1"))
            .unwrap();
        before_graph
            .add_edge(GraphEdge {
                from: "function:a".into(),
                to: "function:b".into(),
                kind: EdgeKind::Calls,
                provenance: provenance("semantic:1"),
            })
            .unwrap();
        let before = SoftwareGraphSnapshot {
            workspace: "demo".into(),
            path: ".".into(),
            provider: "wcode-composite".into(),
            precision: GraphPrecision::Mixed,
            files_considered: 0,
            files_indexed: 0,
            files_failed: 0,
            scan_truncated: false,
            truncated: false,
            node_count: 3,
            edge_count: 1,
            failures: vec![],
            graph: before_graph,
        };
        let before = persist(&workspace, &before).unwrap();

        let mut after_graph = SoftwareGraph::default();
        after_graph
            .add_node(node("function:a", "a-renamed", "semantic:2"))
            .unwrap();
        after_graph
            .add_node(node("function:b", "b", "semantic:1"))
            .unwrap();
        after_graph
            .add_node(node("function:added", "added", "semantic:2"))
            .unwrap();
        after_graph
            .add_edge(GraphEdge {
                from: "function:a".into(),
                to: "function:b".into(),
                kind: EdgeKind::Calls,
                provenance: provenance("semantic:2"),
            })
            .unwrap();
        let after = SoftwareGraphSnapshot {
            workspace: "demo".into(),
            path: ".".into(),
            provider: "wcode-composite".into(),
            precision: GraphPrecision::Mixed,
            files_considered: 0,
            files_indexed: 0,
            files_failed: 0,
            scan_truncated: false,
            truncated: false,
            node_count: 3,
            edge_count: 1,
            failures: vec![],
            graph: after_graph,
        };
        let after = persist(&workspace, &after).unwrap();

        let result = diff(
            &workspace,
            &GraphDiffInput {
                from_snapshot_id: Some(before.id.clone()),
                to_snapshot_id: Some(after.id.clone()),
                limit: 10,
            },
        )
        .unwrap();
        assert_eq!(result.from_snapshot_id, before.id);
        assert_eq!(result.to_snapshot_id, after.id);
        assert_eq!(result.added_node_count, 1);
        assert_eq!(result.removed_node_count, 1);
        assert_eq!(result.changed_node_count, 1);
        assert_eq!(result.added_edge_count, 0);
        assert_eq!(result.removed_edge_count, 0);
        assert_eq!(result.changed_edge_count, 1);
        assert_eq!(
            result.changed_edges[0].before.provenance.revision,
            "semantic:1"
        );
        assert_eq!(
            result.changed_edges[0].after.provenance.revision,
            "semantic:2"
        );
    }
}
