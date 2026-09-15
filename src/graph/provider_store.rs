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
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_PROVIDER_RECORDS: usize = 256;
const MAX_PROVIDER_RECORD_BYTES: u64 = 8 * 1024 * 1024;
static LAST_IMPORTED_AT_MS: AtomicU64 = AtomicU64::new(0);
const MAX_PROVIDER_HINTS: usize = MAX_PROVIDER_RECORDS * 4;
const PROVIDER_READ_BATCH: usize = 4;

// Hints may skip only superseded, unchanged records. Selected imports are
// always read and validated again; source freshness is never cached here.
struct ProviderRecordHint {
    stamp: [u8; 32],
    provider: String,
    imported_at_ms: u64,
}

static PROVIDER_HINTS: OnceLock<Mutex<BTreeMap<PathBuf, ProviderRecordHint>>> = OnceLock::new();

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
        imported_at_ms: next_imported_at_ms(),
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
    let metadata = match fs::symlink_metadata(&directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).context("cannot inspect graph provider store"),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("graph provider store path is not a regular directory");
    }
    let mut paths = provider_paths(&directory)?;
    paths.reverse();
    let mut latest = BTreeMap::<String, StoredGraphProvider>::new();
    let hints = PROVIDER_HINTS.get_or_init(|| Mutex::new(BTreeMap::new()));
    // Keep transient decoded graphs bounded independently of history length.
    // Reverse lexical order preserves the old equal-timestamp winner exactly.
    for batch in paths.chunks(PROVIDER_READ_BATCH) {
        let records = crate::resource::parallel_io(batch, |path| -> Result<_> {
            let stamp = provider_record_stamp(path);
            if let (Some(stamp), Ok(hints)) = (stamp, hints.lock()) {
                if hints.get(path).is_some_and(|hint| {
                    hint.stamp == stamp
                        && latest
                            .get(&hint.provider)
                            .is_some_and(|current| current.imported_at_ms >= hint.imported_at_ms)
                }) {
                    return Ok(None);
                }
            }
            let Some(stored) = read_record(path)? else {
                return Ok(None);
            };
            if stored.import.validate().is_err() {
                return Ok(None);
            }
            if let Some(stamp) = stamp {
                if provider_record_stamp(path) != Some(stamp) {
                    bail!("graph provider record changed while loading; retry the request");
                }
                if let Ok(mut hints) = hints.lock() {
                    if hints.len() >= MAX_PROVIDER_HINTS && !hints.contains_key(path) {
                        hints.pop_first();
                    }
                    hints.insert(
                        path.clone(),
                        ProviderRecordHint {
                            stamp,
                            provider: stored.import.provider.clone(),
                            imported_at_ms: stored.imported_at_ms,
                        },
                    );
                }
            }
            Ok(Some(stored))
        })?;
        for result in records {
            let Some(stored) = result? else { continue };
            let provider = stored.import.provider.clone();
            if latest
                .get(&provider)
                .is_none_or(|current| current.imported_at_ms < stored.imported_at_ms)
            {
                latest.insert(provider, stored);
            }
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
    let mut tracked = BTreeMap::<&str, &str>::new();
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
            .insert(path, source_sha256)
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
    let tracked = tracked.into_iter().collect::<Vec<_>>();
    let checks = match crate::resource::parallel_io(&tracked, |(path, expected_sha256)| {
        workspace
            .load_source(path)
            .is_ok_and(|source| source.sha256 == *expected_sha256)
    }) {
        Ok(checks) => checks,
        Err(_) => return GraphProviderFreshness::Stale,
    };
    if checks.into_iter().all(|fresh| fresh) {
        GraphProviderFreshness::Fresh
    } else {
        GraphProviderFreshness::Stale
    }
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

fn bounded_provider_file(metadata: &fs::Metadata) -> bool {
    if !metadata.is_file() || metadata.len() > MAX_PROVIDER_RECORD_BYTES {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            return false;
        }
    }
    true
}

fn provider_record_stamp(path: &Path) -> Option<[u8; 32]> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = fs::symlink_metadata(path).ok()?;
        if !bounded_provider_file(&metadata) {
            return None;
        }
        let mut hasher = Sha256::new();
        hasher.update(metadata.dev().to_le_bytes());
        hasher.update(metadata.ino().to_le_bytes());
        hasher.update(metadata.len().to_le_bytes());
        hasher.update(metadata.mtime().to_le_bytes());
        hasher.update(metadata.mtime_nsec().to_le_bytes());
        hasher.update(metadata.ctime().to_le_bytes());
        hasher.update(metadata.ctime_nsec().to_le_bytes());
        hasher.update(metadata.mode().to_le_bytes());
        Some(hasher.finalize().into())
    }
    #[cfg(not(unix))]
    {
        // A length/mtime-only key cannot detect a rewrite with restored mtime.
        // Until a strong file identity/change stamp is available, read normally.
        let _ = path;
        None
    }
}

fn read_record(path: &Path) -> Result<Option<StoredGraphProvider>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) if bounded_provider_file(&metadata) => metadata,
        _ => return Ok(None),
    };
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = match options.open(path) {
        Ok(file) => file,
        Err(_) => return Ok(None),
    };
    if !file
        .metadata()
        .is_ok_and(|opened| bounded_provider_file(&opened))
    {
        return Ok(None);
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    if file
        .take(MAX_PROVIDER_RECORD_BYTES + 1)
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() as u64 > MAX_PROVIDER_RECORD_BYTES
    {
        return Ok(None);
    }
    #[cfg(test)]
    tests::record_read(path, bytes.len());
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

fn next_imported_at_ms() -> u64 {
    let wall_clock = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0);
    let mut previous = LAST_IMPORTED_AT_MS.load(Ordering::Relaxed);
    loop {
        let next = wall_clock.max(previous.saturating_add(1));
        match LAST_IMPORTED_AT_MS.compare_exchange_weak(
            previous,
            next,
            Ordering::SeqCst,
            Ordering::Relaxed,
        ) {
            Ok(_) => return next,
            Err(current) => previous = current,
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/graph/provider.rs"]
mod tests;
