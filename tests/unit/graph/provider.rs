use super::*;
use crate::graph::{EdgeKind, GraphImportEdge, GraphImportNode, NodeKind};
use std::collections::BTreeMap;

static RECORD_READS: Mutex<BTreeMap<PathBuf, (usize, usize)>> = Mutex::new(BTreeMap::new());

pub(super) fn record_read(path: &Path, bytes: usize) {
    let mut reads = RECORD_READS.lock().unwrap();
    let entry = reads
        .entry(path.parent().unwrap().to_path_buf())
        .or_default();
    entry.0 += 1;
    entry.1 += bytes;
}

fn take_reads(workspace: &Workspace) -> (usize, usize) {
    RECORD_READS
        .lock()
        .unwrap()
        .remove(&provider_directory(workspace).unwrap())
        .unwrap_or_default()
}

fn provider_fixture(provider: &str, timestamp: u64) -> StoredGraphProvider {
    StoredGraphProvider {
        imported_at_ms: timestamp,
        import: GraphProviderImport {
            provider: provider.into(),
            precision: GraphPrecision::Semantic,
            revision: format!("revision-{timestamp}"),
            nodes: vec![GraphImportNode {
                id: "semantic:function:a".into(),
                kind: NodeKind::Function,
                label: "a".into(),
                attributes: BTreeMap::from([(
                    "payload".into(),
                    serde_json::json!("x".repeat(16_384)),
                )]),
            }],
            edges: vec![],
        },
    }
}

fn write_fixture(
    workspace: &Workspace,
    record: &StoredGraphProvider,
    name: Option<&str>,
) -> PathBuf {
    let directory = provider_directory(workspace).unwrap();
    fs::create_dir_all(&directory).unwrap();
    let bytes = serde_json::to_vec(record).unwrap();
    let canonical = format!(
        "{:020}-{}.json",
        record.imported_at_ms,
        &digest_bytes(&bytes)[..24]
    );
    let path = directory.join(name.unwrap_or(&canonical));
    fs::write(&path, bytes).unwrap();
    path
}

fn legacy_latest(workspace: &Workspace) -> Vec<StoredGraphProvider> {
    let mut latest = BTreeMap::<String, StoredGraphProvider>::new();
    for path in provider_paths(&provider_directory(workspace).unwrap()).unwrap() {
        let Some(stored) = read_record(&path).unwrap() else {
            continue;
        };
        if stored.import.validate().is_ok()
            && latest
                .get(&stored.import.provider)
                .is_none_or(|current| current.imported_at_ms <= stored.imported_at_ms)
        {
            latest.insert(stored.import.provider.clone(), stored);
        }
    }
    latest.into_values().collect()
}

#[test]
fn provider_history_warm_reads_skip_only_superseded_records() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    for index in 0..MAX_PROVIDER_RECORDS {
        write_fixture(
            &workspace,
            &provider_fixture(&format!("fixture-{}", index % 4), index as u64 + 1),
            None,
        );
    }
    let started = std::time::Instant::now();
    let cold = load_latest(&workspace).unwrap();
    let cold_ms = started.elapsed().as_secs_f64() * 1_000.0;
    let cold_reads = take_reads(&workspace);
    let started = std::time::Instant::now();
    let warm = load_latest(&workspace).unwrap();
    let warm_ms = started.elapsed().as_secs_f64() * 1_000.0;
    let warm_reads = take_reads(&workspace);
    assert_eq!(cold.len(), 4);
    assert_eq!(cold_reads.0, MAX_PROVIDER_RECORDS);
    assert_eq!(
        serde_json::to_value(&cold).unwrap(),
        serde_json::to_value(&warm).unwrap()
    );
    #[cfg(unix)]
    assert_eq!(warm_reads.0, PROVIDER_READ_BATCH);
    #[cfg(not(unix))]
    assert_eq!(warm_reads.0, MAX_PROVIDER_RECORDS);
    let report = serde_json::json!({
        "fixture": "256 immutable records, 4 providers, 16KiB node payload each",
        "history_records": MAX_PROVIDER_RECORDS,
        "providers_returned": warm.len(),
        "cold_record_reads": cold_reads.0, "warm_record_reads": warm_reads.0,
        "cold_bytes_read": cold_reads.1, "warm_bytes_read": warm_reads.1,
        "cold_ms": cold_ms, "warm_ms": warm_ms,
        "results_equal": true, "metadata_hint_acceleration": cfg!(unix),
        "timing_is_informational": true
    });
    let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
    fs::create_dir_all(&target).unwrap();
    fs::write(
        target.join("wcode-provider-history.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
}

#[test]
fn provider_history_preserves_legacy_order_ties_and_invalid_fallback() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    for index in 0..23 {
        let record = provider_fixture(&format!("fixture-{}", index % 3), (index * 11 % 7) as u64);
        write_fixture(
            &workspace,
            &record,
            Some(&format!("legacy-{index:03}.json")),
        );
    }
    let mut invalid = provider_fixture("fixture-0", 999);
    invalid.import.precision = GraphPrecision::Syntax;
    write_fixture(&workspace, &invalid, Some("z-invalid.json"));
    fs::write(
        provider_directory(&workspace)
            .unwrap()
            .join("z-malformed.json"),
        b"{",
    )
    .unwrap();
    let expected = serde_json::to_value(legacy_latest(&workspace)).unwrap();
    for _ in 0..3 {
        assert_eq!(
            serde_json::to_value(load_latest(&workspace).unwrap()).unwrap(),
            expected
        );
    }
}

#[test]
fn provider_history_rechecks_corruption_deletion_and_replacement() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let paths = (1..=12)
        .map(|index| write_fixture(&workspace, &provider_fixture("fixture-a", index), None))
        .collect::<Vec<_>>();
    assert_eq!(load_latest(&workspace).unwrap()[0].imported_at_ms, 12);
    fs::write(&paths[11], b"corrupted").unwrap();
    assert_eq!(load_latest(&workspace).unwrap()[0].imported_at_ms, 11);
    fs::remove_file(&paths[10]).unwrap();
    assert_eq!(load_latest(&workspace).unwrap()[0].imported_at_ms, 10);
    let replacement = provider_fixture("fixture-b", 100);
    fs::write(&paths[0], serde_json::to_vec(&replacement).unwrap()).unwrap();
    let latest = load_latest(&workspace).unwrap();
    assert_eq!(latest.len(), 2);
    assert_eq!(latest[1].import.provider, "fixture-b");
    assert_eq!(latest[1].imported_at_ms, 100);
    assert_eq!(
        serde_json::to_value(latest).unwrap(),
        serde_json::to_value(legacy_latest(&workspace)).unwrap()
    );
}

#[cfg(unix)]
#[test]
fn provider_hints_detect_same_size_rewrite_with_restored_mtime() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let paths = (1..=12)
        .map(|index| write_fixture(&workspace, &provider_fixture("fixture-a", index), None))
        .collect::<Vec<_>>();
    load_latest(&workspace).unwrap();
    let original = fs::metadata(&paths[0]).unwrap();
    let before = provider_record_stamp(&paths[0]);
    let changed = serde_json::to_vec(&provider_fixture("fixture-b", 1)).unwrap();
    assert_eq!(changed.len() as u64, original.len());
    fs::write(&paths[0], changed).unwrap();
    fs::File::open(&paths[0])
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(original.modified().unwrap()))
        .unwrap();
    assert_eq!(
        fs::metadata(&paths[0]).unwrap().modified().unwrap(),
        original.modified().unwrap()
    );
    assert_ne!(provider_record_stamp(&paths[0]), before);
    let latest = load_latest(&workspace).unwrap();
    assert_eq!(
        latest.len(),
        2,
        "a cached old provider name must not hide the replacement"
    );
    assert_eq!(latest[1].import.provider, "fixture-b");
}

#[test]
fn provider_history_hints_are_workspace_scoped_and_never_cache_source_freshness() {
    let root = tempfile::tempdir().unwrap();
    let other_root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let other = Workspace::new(other_root.path(), false, false).unwrap();
    fs::write(root.path().join("a.rs"), "fn a() {}\n").unwrap();
    let source = workspace.load_source("a.rs").unwrap();
    let mut record = provider_fixture("lsp:fixture", 1);
    record.import.nodes[0].attributes = BTreeMap::from([
        ("path".into(), serde_json::json!("a.rs")),
        ("source_sha256".into(), serde_json::json!(source.sha256)),
    ]);
    write_fixture(&workspace, &record, None);
    write_fixture(&other, &provider_fixture("lsp:fixture", 99), None);
    assert_eq!(
        summaries(&workspace).unwrap()[0].freshness,
        GraphProviderFreshness::Fresh
    );
    assert_eq!(load_latest(&other).unwrap()[0].imported_at_ms, 99);
    assert_eq!(load_latest(&workspace).unwrap()[0].imported_at_ms, 1);
    fs::write(root.path().join("a.rs"), "fn b() {}\n").unwrap();
    assert_eq!(
        summaries(&workspace).unwrap()[0].freshness,
        GraphProviderFreshness::Stale
    );
}

#[test]
fn provider_record_reads_reject_oversized_and_non_file_entries() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    write_fixture(&workspace, &provider_fixture("fixture", 1), None);
    let directory = provider_directory(&workspace).unwrap();
    fs::File::create(directory.join("z-large.json"))
        .unwrap()
        .set_len(MAX_PROVIDER_RECORD_BYTES + 1)
        .unwrap();
    fs::create_dir(directory.join("z-directory.json")).unwrap();
    assert_eq!(load_latest(&workspace).unwrap().len(), 1);
    assert_eq!(take_reads(&workspace).0, 1);
}

#[cfg(unix)]
#[test]
fn provider_store_rejects_linked_records_and_dangling_directory_links() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let path = write_fixture(&workspace, &provider_fixture("fixture", 1), None);
    load_latest(&workspace).unwrap();
    let link = path.with_file_name("z-link.json");
    symlink(&path, &link).unwrap();
    assert_eq!(load_latest(&workspace).unwrap().len(), 1);
    fs::hard_link(&path, root.path().join("hardlink.json")).unwrap();
    assert!(load_latest(&workspace).unwrap().is_empty());
    for dangling in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(root.path(), false, false).unwrap();
        let directory = provider_directory(&workspace).unwrap();
        fs::create_dir_all(directory.parent().unwrap()).unwrap();
        let target = root.path().join("target-directory");
        if !dangling {
            fs::create_dir(&target).unwrap();
        }
        symlink(&target, &directory).unwrap();
        assert!(load_latest(&workspace).is_err());
    }
}

#[test]
fn concurrent_provider_history_reads_preserve_complete_results() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    for index in 0..24 {
        write_fixture(
            &workspace,
            &provider_fixture(&format!("fixture-{}", index % 4), index + 1),
            None,
        );
    }
    let expected = serde_json::to_value(legacy_latest(&workspace)).unwrap();
    let barrier = std::sync::Barrier::new(6);
    std::thread::scope(|scope| {
        let handles = (0..6)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    for _ in 0..3 {
                        assert_eq!(
                            serde_json::to_value(load_latest(&workspace).unwrap()).unwrap(),
                            expected
                        );
                    }
                })
            })
            .collect::<Vec<_>>();
        for handle in handles {
            handle.join().unwrap();
        }
    });
    assert!(PROVIDER_HINTS.get().unwrap().lock().unwrap().len() <= MAX_PROVIDER_HINTS);
}

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
    let first_stored = persist(&workspace, &first).unwrap();
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
    let second_stored = persist(&workspace, &second).unwrap();
    assert!(second_stored.imported_at_ms > first_stored.imported_at_ms);
    let latest = load_latest(&workspace).unwrap();
    assert_eq!(latest.len(), 1);
    assert_eq!(latest[0].import.revision, "two");
    assert_eq!(latest[0].import.nodes[0].label, "b");
}
