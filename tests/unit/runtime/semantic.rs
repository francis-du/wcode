use super::*;

#[test]
fn retry_backoff_is_bounded() {
    assert_eq!(
        doubled_retry(Duration::from_secs(10)),
        Duration::from_secs(20)
    );
    assert_eq!(doubled_retry(Duration::from_secs(200)), MAX_RETRY);
    assert_eq!(doubled_retry(MAX_RETRY), MAX_RETRY);
}

#[test]
fn semantic_execution_is_default_on_and_can_be_disabled() {
    assert!(crate::workspace::WorkspaceSecurity::default().allow_semantic_exec);
    let dir = tempfile::tempdir().unwrap();
    let workspaces = Workspaces::new_with_security(
        [dir.path()],
        false,
        true,
        crate::workspace::WorkspaceSecurity {
            allow_semantic_exec: false,
            ..crate::workspace::WorkspaceSecurity::default()
        },
    )
    .unwrap();
    assert!(workspaces.semantic_workspaces().is_empty());
}

#[test]
fn semantic_workers_skip_broad_parent_workspaces() {
    let root = tempfile::tempdir().unwrap();
    for (path, marker) in [("Rust/app", "Cargo.toml"), ("Web/app", "package.json")] {
        let project = root.path().join(path);
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join(marker), "fixture\n").unwrap();
    }
    let workspaces = Workspaces::new([root.path()], false, true).unwrap();
    let semantic = workspaces.semantic_workspaces();
    assert_eq!(semantic.len(), 2);
    assert!(semantic
        .iter()
        .all(|(_, workspace)| workspace.root() != root.path()));
}
