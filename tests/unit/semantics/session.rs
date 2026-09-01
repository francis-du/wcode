use super::*;

#[test]
fn session_pool_is_bounded_and_empty_by_default() {
    let pool = SemanticSessionPool::default();
    let status = pool.status();
    assert_eq!(status.sessions, 0);
    assert_eq!(status.documents, 0);
    assert_eq!(status.requests, 0);
    assert_eq!(status.max_sessions, session_limit());
}

#[test]
fn session_validation_requires_a_successful_provider_start() {
    let workspace_dir = tempfile::tempdir().unwrap();
    let tools = tempfile::tempdir().unwrap();
    let executable = tools.path().join("rust-analyzer");
    std::fs::write(&executable, "fixture").unwrap();
    let workspace = Workspace::new(workspace_dir.path(), false, true).unwrap();
    let provider = PROVIDERS
        .iter()
        .copied()
        .find(|provider| provider.id == "rust-analyzer")
        .unwrap();
    let pool = SemanticSessionPool::default();
    let handle = pool.handle(&workspace, provider, &executable).unwrap();
    assert!(!pool.validated(&workspace, provider, &executable));
    handle.slot.metrics.starts.store(1, Ordering::Relaxed);
    assert!(pool.validated(&workspace, provider, &executable));
}

#[test]
fn same_workspace_provider_reuses_one_session_slot() {
    let workspace_dir = tempfile::tempdir().unwrap();
    let tools = tempfile::tempdir().unwrap();
    let executable = tools.path().join("rust-analyzer");
    std::fs::write(&executable, "fixture").unwrap();
    let workspace = Workspace::new(workspace_dir.path(), false, true).unwrap();
    let provider = PROVIDERS
        .iter()
        .copied()
        .find(|provider| provider.id == "rust-analyzer")
        .unwrap();
    let pool = SemanticSessionPool::default();
    let first = pool.handle(&workspace, provider, &executable).unwrap();
    let second = pool.handle(&workspace, provider, &executable).unwrap();
    assert!(Arc::ptr_eq(&first.slot, &second.slot));
    assert_eq!(pool.status_for(&workspace).sessions, 1);
}

#[test]
fn session_key_changes_when_provider_binary_changes() {
    let workspace_dir = tempfile::tempdir().unwrap();
    let tools = tempfile::tempdir().unwrap();
    let executable = tools.path().join("rust-analyzer");
    std::fs::write(&executable, "first").unwrap();
    let workspace = Workspace::new(workspace_dir.path(), false, true).unwrap();
    let provider = PROVIDERS
        .iter()
        .copied()
        .find(|provider| provider.id == "rust-analyzer")
        .unwrap();
    let first = session_key(&workspace, provider, &executable).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(2));
    std::fs::write(&executable, "second-binary").unwrap();
    let second = session_key(&workspace, provider, &executable).unwrap();
    assert_ne!(first, second);
}

#[test]
fn idle_pruning_removes_only_unleased_slots() {
    let workspace_dir = tempfile::tempdir().unwrap();
    let tools = tempfile::tempdir().unwrap();
    let executable = tools.path().join("rust-analyzer");
    std::fs::write(&executable, "fixture").unwrap();
    let workspace = Workspace::new(workspace_dir.path(), false, true).unwrap();
    let provider = PROVIDERS
        .iter()
        .copied()
        .find(|provider| provider.id == "rust-analyzer")
        .unwrap();
    let pool = SemanticSessionPool::default();
    let handle = pool.handle(&workspace, provider, &executable).unwrap();
    handle.slot.metrics.last_used_ms.store(0, Ordering::Relaxed);
    pool.prune_idle();
    assert_eq!(pool.status_for(&workspace).sessions, 1);
    drop(handle);
    pool.prune_idle();
    assert_eq!(pool.status_for(&workspace).sessions, 0);
}

#[test]
fn session_capacity_fails_closed_when_every_slot_is_leased() {
    let tools = tempfile::tempdir().unwrap();
    let executable = tools.path().join("rust-analyzer");
    std::fs::write(&executable, "fixture").unwrap();
    let provider = PROVIDERS
        .iter()
        .copied()
        .find(|provider| provider.id == "rust-analyzer")
        .unwrap();
    let limit = session_limit();
    let roots = (0..=limit)
        .map(|_| tempfile::tempdir().unwrap())
        .collect::<Vec<_>>();
    let pool = SemanticSessionPool::default();
    let mut handles = Vec::new();
    for root in roots.iter().take(limit) {
        let workspace = Workspace::new(root.path(), false, true).unwrap();
        handles.push(pool.handle(&workspace, provider, &executable).unwrap());
    }
    let extra = Workspace::new(roots[limit].path(), false, true).unwrap();
    let error = pool.handle(&extra, provider, &executable).err().unwrap();
    assert!(error.to_string().contains("at capacity"));
    drop(handles.pop());
    assert!(pool.handle(&extra, provider, &executable).is_ok());
}

#[test]
fn provider_identity_change_waits_for_a_leased_session() {
    let workspace_dir = tempfile::tempdir().unwrap();
    let tools = tempfile::tempdir().unwrap();
    let executable = tools.path().join("rust-analyzer");
    std::fs::write(&executable, "first").unwrap();
    let workspace = Workspace::new(workspace_dir.path(), false, true).unwrap();
    let provider = PROVIDERS
        .iter()
        .copied()
        .find(|provider| provider.id == "rust-analyzer")
        .unwrap();
    let pool = SemanticSessionPool::default();
    let handle = pool.handle(&workspace, provider, &executable).unwrap();
    std::fs::write(&executable, "second-binary").unwrap();
    let error = pool
        .handle(&workspace, provider, &executable)
        .err()
        .unwrap();
    assert!(error.to_string().contains("busy"));
    drop(handle);
    assert!(pool.handle(&workspace, provider, &executable).is_ok());
}

#[test]
fn text_document_sync_profile_honors_full_incremental_and_none() {
    assert_eq!(
        text_document_sync_profile(&json!({"textDocumentSync": 1})),
        TextDocumentSyncProfile {
            open_close: true,
            change: TextDocumentSyncMode::Full,
        }
    );
    assert_eq!(
        text_document_sync_profile(&json!({"textDocumentSync": 2})),
        TextDocumentSyncProfile {
            open_close: true,
            change: TextDocumentSyncMode::Incremental,
        }
    );
    assert_eq!(
        text_document_sync_profile(&json!({
            "textDocumentSync":{"openClose":true,"change":2}
        })),
        TextDocumentSyncProfile {
            open_close: true,
            change: TextDocumentSyncMode::Incremental,
        }
    );
    assert_eq!(
        text_document_sync_profile(&json!({"textDocumentSync":{"change":0}})),
        TextDocumentSyncProfile {
            open_close: false,
            change: TextDocumentSyncMode::None,
        }
    );
}

#[test]
fn incremental_replacement_range_uses_negotiated_position_encoding() {
    let source = "fn café() {}\nlet x = 🦀;";
    assert_eq!(
        document_end_position(source, "utf-8"),
        json!({"line":1,"character":13})
    );
    assert_eq!(
        document_end_position(source, "utf-16"),
        json!({"line":1,"character":11})
    );
    assert_eq!(
        document_end_position(source, "utf-32"),
        json!({"line":1,"character":10})
    );
}
