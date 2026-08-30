use super::*;
use crate::workspace::Workspace;
use std::fs;

fn fixture_workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".wcode/design/requirements")).unwrap();
    fs::create_dir_all(dir.path().join(".wcode/design/components")).unwrap();
    fs::create_dir_all(dir.path().join(".wcode/design/constraints")).unwrap();
    fs::create_dir_all(dir.path().join(".wcode/design/acceptance")).unwrap();
    fs::write(
        dir.path().join(".wcode/project.yaml"),
        "name: demo\ndescription: test project\n",
    )
    .unwrap();
    fs::write(
            dir.path().join(".wcode/design/requirements/REQ-SEC-001.yaml"),
            "id: REQ-SEC-001\ntitle: Root isolation\nintent: Keep reads inside the workspace.\npriority: critical\nimplemented_by:\n  - component:workspace-security\nacceptance:\n  - AC-SEC-001\nconstraints:\n  - CONSTRAINT-ROOT\nrisk:\n  security: high\n",
        )
        .unwrap();
    fs::write(
            dir.path().join(".wcode/design/components/workspace-security.yaml"),
            "id: component:workspace-security\nname: Workspace Security\nresponsibilities:\n  - isolate repository roots\nconstraints:\n  - CONSTRAINT-ROOT\nimplementation:\n  - kind: symbol\n    path: src/workspace.rs\n    symbol: Workspace::existing_path\n",
        )
        .unwrap();
    fs::write(
            dir.path().join(".wcode/design/constraints/root.yaml"),
            "id: CONSTRAINT-ROOT\ntitle: Root boundary\nstatement: Paths must remain inside the selected workspace.\napplies_to:\n  - component:workspace-security\n",
        )
        .unwrap();
    fs::write(
            dir.path().join(".wcode/design/acceptance/AC-SEC-001.yaml"),
            "id: AC-SEC-001\ntitle: Traversal is rejected\nstatement: Parent traversal cannot escape the root.\nverification:\n  - kind: test\n    path: src/workspace.rs\n    symbol: blocks_path_traversal_and_stale_writes\n",
        )
        .unwrap();
    dir
}

#[test]
fn loads_structured_design_state_and_validates_references() {
    let dir = fixture_workspace();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/workspace.rs"), "fn placeholder() {}\n").unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let load = load_design(&workspace).unwrap();

    assert!(load.initialized);
    assert_eq!(load.files_loaded, 5);
    assert_eq!(load.state.requirements.len(), 1);
    assert_eq!(load.state.components.len(), 1);
    assert_eq!(load.state.acceptance.len(), 1);
    assert_eq!(load.error_count(), 0, "{:?}", load.diagnostics);
}

#[test]
fn reports_missing_and_unsafe_references_without_panicking() {
    let dir = fixture_workspace();
    fs::write(
            dir.path().join(".wcode/design/components/workspace-security.yaml"),
            "id: component:workspace-security\nname: Workspace Security\nimplementation:\n  - kind: file\n    path: ../outside.rs\n",
        )
        .unwrap();
    fs::write(
            dir.path().join(".wcode/design/requirements/REQ-SEC-001.yaml"),
            "id: REQ-SEC-001\ntitle: Root isolation\nintent: Keep reads inside the workspace.\nimplemented_by:\n  - component:missing\n",
        )
        .unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let load = load_design(&workspace).unwrap();

    assert!(load.error_count() >= 2);
    assert!(load
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "missing-design-reference"));
    assert!(load
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "invalid-code-reference"));
}

#[test]
fn absent_design_state_is_a_valid_uninitialized_project() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let load = load_design(&workspace).unwrap();
    assert!(!load.initialized);
    assert_eq!(load.state.node_count(), 0);
    assert_eq!(load.error_count(), 0);
}
