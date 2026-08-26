use crate::evidence_store::workspace_state_directory;
use crate::graph::{
    GraphEdge, GraphNode, GraphPrecision, GraphProviderImport, SoftwareGraphSnapshot,
};
use crate::workspace::Workspace;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_PROVIDER_RECORDS: usize = 256;
const MAX_PROVIDER_RECORD_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StoredGraphProvider {
    pub imported_at_ms: u64,
    pub import: GraphProviderImport,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphProviderFreshness {
    Fresh,
    Stale,
    Unknown,
}

#[derive(Clone, Debug, Serialize)]
pub struct GraphProviderSummary {
    pub provider: String,
    pub precision: GraphPrecision,
    pub revision: String,
    pub freshness: GraphProviderFreshness,
    pub nodes: usize,
    pub edges: usize,
    pub imported_at_ms: u64,
}

pub(crate) fn persist(
    workspace: &Workspace,
    import: &GraphProviderImport,
) -> Result<StoredGraphProvider> {
    import.validate()?;
    let stored = StoredGraphProvider {
        imported_at_ms: now_ms(),
        import: import.clone(),
    };
    let bytes = serde_json::to_vec(&stored).context("cannot encode graph provider import")?;
    if bytes.len() as u64 > MAX_PROVIDER_RECORD_BYTES {
        bail!("graph provider import exceeds the persistent store size bound");
    }
    let directory = provider_directory(workspace)?;
    fs::create_dir_all(&directory)
        .with_context(|| format!("cannot create graph provider store {}", directory.display()))?;
    let digest = digest_bytes(&bytes);
    let path = directory.join(format!(
        "{:020}-{}.json",
        stored.imported_at_ms,
        &digest[..24]
    ));
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
            .with_context(|| format!("cannot create graph provider record {}", path.display()))?;
        file.write_all(&bytes)
            .with_context(|| format!("cannot write graph provider record {}", path.display()))?;
        file.sync_all()
            .with_context(|| format!("cannot sync graph provider record {}", path.display()))?;
        prune_directory(&directory)?;
    }
    Ok(stored)
}

pub(crate) fn load_latest(workspace: &Workspace) -> Result<Vec<StoredGraphProvider>> {
    let directory = provider_directory(workspace)?;
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let metadata = fs::symlink_metadata(&directory).with_context(|| {
        format!(
            "cannot inspect graph provider store {}",
            directory.display()
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("graph provider store path is not a regular directory");
    }
    let mut latest = BTreeMap::<String, StoredGraphProvider>::new();
    for path in provider_paths(&directory)? {
        let Some(stored) = read_record(&path)? else {
            continue;
        };
        if stored.import.validate().is_err() {
            continue;
        }
        let provider = stored.import.provider.clone();
        if latest
            .get(&provider)
            .is_none_or(|current| current.imported_at_ms <= stored.imported_at_ms)
        {
            latest.insert(provider, stored);
        }
    }
    Ok(latest.into_values().collect())
}

pub(crate) fn summaries(workspace: &Workspace) -> Result<Vec<GraphProviderSummary>> {
    let mut summaries = load_latest(workspace)?
        .into_iter()
        .map(|stored| GraphProviderSummary {
            freshness: freshness(workspace, &stored.import),
            provider: stored.import.provider,
            precision: stored.import.precision,
            revision: stored.import.revision,
            nodes: stored.import.nodes.len(),
            edges: stored.import.edges.len(),
            imported_at_ms: stored.imported_at_ms,
        })
        .collect::<Vec<_>>();
    summaries.sort_by(|left, right| left.provider.cmp(&right.provider));
    Ok(summaries)
}

pub(crate) fn freshness(
    workspace: &Workspace,
    import: &GraphProviderImport,
) -> GraphProviderFreshness {
    let mut tracked = BTreeMap::<String, String>::new();
    for node in &import.nodes {
        let Some(path) = node
            .attributes
            .get("path")
            .and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        let Some(source_sha256) = node
            .attributes
            .get("source_sha256")
            .and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        if tracked
            .insert(path.to_owned(), source_sha256.to_owned())
            .is_some_and(|existing| existing != source_sha256)
        {
            return GraphProviderFreshness::Stale;
        }
    }
    if tracked.is_empty() {
        return if import.provider.starts_with("lsp:") {
            GraphProviderFreshness::Stale
        } else {
            GraphProviderFreshness::Unknown
        };
    }
    for (path, expected_sha256) in tracked {
        match workspace.load_source(&path) {
            Ok(source) if source.sha256 == expected_sha256 => {}
            _ => return GraphProviderFreshness::Stale,
        }
    }
    GraphProviderFreshness::Fresh
}

pub(crate) fn overlay_latest(
    workspace: &Workspace,
    snapshot: &mut SoftwareGraphSnapshot,
) -> Result<usize> {
    let providers = load_latest(workspace)?;
    let mut overlayed = 0usize;
    for stored in &providers {
        if freshness(workspace, &stored.import) == GraphProviderFreshness::Stale {
            continue;
        }
        overlayed = overlayed.saturating_add(1);
        let provenance = stored.import.provenance();
        for node in &stored.import.nodes {
            if !snapshot.graph.nodes.contains_key(&node.id) {
                snapshot.graph.add_node(GraphNode {
                    id: node.id.clone(),
                    kind: node.kind,
                    label: node.label.clone(),
                    attributes: node.attributes.clone(),
                    provenance: provenance.clone(),
                })?;
            }
        }
        for edge in &stored.import.edges {
            if edge.from == edge.to
                || !snapshot.graph.nodes.contains_key(&edge.from)
                || !snapshot.graph.nodes.contains_key(&edge.to)
            {
                continue;
            }
            let graph_edge = GraphEdge {
                from: edge.from.clone(),
                to: edge.to.clone(),
                kind: edge.kind,
                provenance: provenance.clone(),
            };
            if !snapshot
                .graph
                .edges
                .iter()
                .any(|existing| existing == &graph_edge)
            {
                snapshot.graph.add_edge(graph_edge)?;
            }
        }
    }
    if overlayed > 0 {
        snapshot.provider = "wcode-composite".to_owned();
        snapshot.precision = GraphPrecision::Mixed;
        snapshot.node_count = snapshot.graph.nodes.len();
        snapshot.edge_count = snapshot.graph.edges.len();
        snapshot.graph.validate()?;
    }
    Ok(overlayed)
}

pub(crate) fn capabilities() -> serde_json::Value {
    serde_json::json!({
        "persistent": true,
        "format": "immutable-provider-imports",
        "scope": "per-workspace",
        "max_provider_records": MAX_PROVIDER_RECORDS,
        "max_record_bytes": MAX_PROVIDER_RECORD_BYTES,
        "accepted_precision": ["semantic", "runtime", "deterministic", "heuristic"],
        "freshness": "source_sha256-aware; stale first-party LSP imports are excluded from overlays"
    })
}

fn provider_directory(workspace: &Workspace) -> Result<PathBuf> {
    Ok(workspace_state_directory(workspace)?.join("graph-providers"))
}

fn provider_paths(directory: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = fs::read_dir(directory)
        .with_context(|| format!("cannot list graph provider store {}", directory.display()))?
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

fn read_record(path: &Path) -> Result<Option<StoredGraphProvider>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => return Ok(None),
    };
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_PROVIDER_RECORD_BYTES
    {
        return Ok(None);
    }
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(_) => return Ok(None),
    };
    Ok(serde_json::from_slice(&bytes).ok())
}

fn prune_directory(directory: &Path) -> Result<()> {
    let paths = provider_paths(directory)?;
    let excess = paths.len().saturating_sub(MAX_PROVIDER_RECORDS);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{EdgeKind, GraphImportEdge, GraphImportNode, NodeKind};
    use std::collections::BTreeMap;

    #[test]
    fn first_party_lsp_freshness_tracks_source_hashes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
        let workspace = Workspace::new(dir.path(), false, false).unwrap();
        let source = workspace.load_source("a.rs").unwrap();
        let import = GraphProviderImport {
            provider: "lsp:fixture".into(),
            precision: GraphPrecision::Semantic,
            revision: "sha256:fixture".into(),
            nodes: vec![GraphImportNode {
                id: "semantic:function:a".into(),
                kind: NodeKind::Function,
                label: "a".into(),
                attributes: BTreeMap::from([
                    ("path".into(), serde_json::json!("a.rs")),
                    ("source_sha256".into(), serde_json::json!(source.sha256)),
                ]),
            }],
            edges: vec![],
        };
        assert_eq!(
            freshness(&workspace, &import),
            GraphProviderFreshness::Fresh
        );
        std::fs::write(dir.path().join("a.rs"), "fn changed() {}\n").unwrap();
        assert_eq!(
            freshness(&workspace, &import),
            GraphProviderFreshness::Stale
        );

        let external = GraphProviderImport {
            provider: "external-scip".into(),
            nodes: vec![GraphImportNode {
                attributes: BTreeMap::new(),
                ..import.nodes[0].clone()
            }],
            ..import
        };
        assert_eq!(
            freshness(&workspace, &external),
            GraphProviderFreshness::Unknown
        );
    }

    #[test]
    fn latest_provider_revision_wins_without_losing_other_providers() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(dir.path(), false, false).unwrap();
        let first = GraphProviderImport {
            provider: "rust-analyzer".into(),
            precision: GraphPrecision::Semantic,
            revision: "one".into(),
            nodes: vec![GraphImportNode {
                id: "semantic:function:a".into(),
                kind: NodeKind::Function,
                label: "a".into(),
                attributes: BTreeMap::new(),
            }],
            edges: vec![],
        };
        persist(&workspace, &first).unwrap();
        let second = GraphProviderImport {
            revision: "two".into(),
            nodes: vec![GraphImportNode {
                id: "semantic:function:b".into(),
                kind: NodeKind::Function,
                label: "b".into(),
                attributes: BTreeMap::new(),
            }],
            edges: vec![GraphImportEdge {
                from: "semantic:function:b".into(),
                to: "symbol:external".into(),
                kind: EdgeKind::Calls,
            }],
            ..first
        };
        persist(&workspace, &second).unwrap();
        let latest = load_latest(&workspace).unwrap();
        assert_eq!(latest.len(), 1);
        assert_eq!(latest[0].import.revision, "two");
        assert_eq!(latest[0].import.nodes[0].label, "b");
    }
}
