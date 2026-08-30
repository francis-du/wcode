use super::*;

#[test]
fn session_pool_is_bounded_and_empty_by_default() {
    let pool = SemanticSessionPool::default();
    let status = pool.status();
    assert_eq!(status.sessions, 0);
    assert_eq!(status.documents, 0);
    assert_eq!(status.requests, 0);
    assert_eq!(status.max_sessions, MAX_SESSIONS);
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
