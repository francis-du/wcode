use super::*;
use std::fs;

#[test]
fn hot_context_skips_unused_related_bodies_and_ast_reconstruction() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("main.rs"),
        "fn entry() { helper(); }\nfn helper() {}\nfn caller() { entry(); }\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();
    let found = index
        .find_symbol("demo", &workspace, "entry", ".", None, 10)
        .unwrap();
    let id = found["results"][0]["id"].as_str().unwrap();
    index.state.lock().unwrap().ast_cache.clear();
    let hot = index
        .symbol_hot_context("demo", &workspace, id, 80)
        .unwrap();
    assert!(hot.get("same_file_related_context").is_none());
    assert!(
        index.state.lock().unwrap().ast_cache.is_empty(),
        "a discarded AST must not be rebuilt for a direct body read"
    );
    let full = index.symbol_context("demo", &workspace, id, 80).unwrap();
    for key in ["symbol", "sha256", "body", "syntax_calls"] {
        assert_eq!(
            hot[key], full[key],
            "direct and full context must share {key}"
        );
    }
    assert!(!full["same_file_related_context"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn syntax_call_navigation_exposes_bounded_cross_file_callers() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("target.rs"), "pub fn target() {}\n").unwrap();
    fs::write(
        root.path().join("caller.rs"),
        "pub fn caller() {\n    target();\n}\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();
    let found = index
        .find_symbol("demo", &workspace, "target", ".", None, 10)
        .unwrap();
    let id = found["results"][0]["id"].as_str().unwrap();
    let graph = index
        .software_graph("demo", &workspace, ".", 100, 100)
        .unwrap();
    let navigation = index.syntax_call_navigation(&graph, id, 10);
    assert_eq!(navigation["precision"], "syntax");
    assert!(navigation["incoming_calls"]
        .as_array()
        .unwrap()
        .iter()
        .any(|caller| caller["name"] == "caller"));
}

#[test]
fn hot_context_rejects_cross_workspace_ids_and_refreshes_equal_length_edits() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("main.rs");
    let before = "fn entry() { helper(); }\nfn helper() {}\nfn worker() {}\n";
    let after = "fn entry() { worker(); }\nfn helper() {}\nfn worker() {}\n";
    assert_eq!(before.len(), after.len());
    fs::write(&path, before).unwrap();
    let mtime = fs::metadata(&path).unwrap().modified().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();
    let found = index
        .find_symbol("demo", &workspace, "entry", ".", None, 10)
        .unwrap();
    let id = found["results"][0]["id"].as_str().unwrap();
    let original = index
        .symbol_hot_context("demo", &workspace, id, 80)
        .unwrap();
    fs::write(&path, after).unwrap();
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(mtime))
        .unwrap();
    let current = index
        .symbol_hot_context("demo", &workspace, id, 80)
        .unwrap();
    assert_ne!(original["sha256"], current["sha256"]);
    assert!(current["body"]["content"]
        .as_str()
        .unwrap()
        .contains("worker();"));
    assert!(current["syntax_calls"]
        .as_array()
        .unwrap()
        .iter()
        .any(|call| call["name"] == "worker"));
    assert!(!current["syntax_calls"]
        .as_array()
        .unwrap()
        .iter()
        .any(|call| call["name"] == "helper"));
    let other_root = tempfile::tempdir().unwrap();
    fs::write(other_root.path().join("main.rs"), before).unwrap();
    let other = Workspace::new(other_root.path(), false, false).unwrap();
    assert!(index.symbol_hot_context("other", &other, id, 80).is_err());
}
