use super::*;
use std::fs::{FileTimes, OpenOptions};
use std::io::Write;

#[test]
fn design_cache_detects_same_size_same_mtime_content_rewrite() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join(".wcode/design")).unwrap();
    std::fs::write(
        root.path().join(".wcode/project.yaml"),
        "schema_version: 1\nname: evolution-cache\n",
    )
    .unwrap();
    let product = root.path().join(".wcode/design/product.yaml");
    let initial = "schema_version: 1\nid: product:demo\nname: Demo\nvision: Alpha\n";
    let revised = "schema_version: 1\nid: product:demo\nname: Demo\nvision: Bravo\n";
    assert_eq!(initial.len(), revised.len());
    std::fs::write(&product, initial).unwrap();

    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let first = runtime.design_load(&workspace).unwrap();
    assert_eq!(
        first.state.product.as_ref().unwrap().vision,
        "Alpha",
        "fixture must warm the Design cache"
    );

    let original_modified = std::fs::metadata(&product).unwrap().modified().unwrap();
    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&product)
        .unwrap();
    file.write_all(revised.as_bytes()).unwrap();
    file.sync_all().unwrap();
    file.set_times(FileTimes::new().set_modified(original_modified))
        .unwrap();
    drop(file);
    let metadata = std::fs::metadata(&product).unwrap();
    assert_eq!(metadata.len(), revised.len() as u64);
    assert_eq!(metadata.modified().unwrap(), original_modified);

    let second = runtime.design_load(&workspace).unwrap();
    assert_eq!(
        second.state.product.as_ref().unwrap().vision,
        "Bravo",
        "Design cache must not trust only size+mtime across repository evolution"
    );
}

#[test]
fn traceability_cache_detects_same_size_same_mtime_symbol_rewrite() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("src")).unwrap();
    std::fs::create_dir_all(root.path().join(".wcode/design/requirements")).unwrap();
    std::fs::create_dir_all(root.path().join(".wcode/design/components")).unwrap();
    std::fs::create_dir_all(root.path().join(".wcode/design/acceptance")).unwrap();
    let source = root.path().join("src/lib.rs");
    let initial = "fn secure(path: &str) -> bool { !path.contains(\"..\") }\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn blocks_escape() { assert!(!super::secure(\"../secret\")); }\n}\n";
    let revised = initial
        .replace("blocks_escape", "blocks_escapx")
        .replace("secure", "secxre");
    assert_eq!(initial.len(), revised.len());
    std::fs::write(&source, initial).unwrap();
    std::fs::write(
        root.path().join(".wcode/design/requirements/REQ-SEC-001.yaml"),
        "id: REQ-SEC-001\ntitle: Workspace isolation\nintent: Paths must stay inside the workspace.\nimplemented_by: [component:workspace-security]\nacceptance: [AC-SEC-001]\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join(".wcode/design/components/workspace-security.yaml"),
        "id: component:workspace-security\nname: Workspace Security\nimplementation:\n  - kind: symbol\n    path: src/lib.rs\n    symbol: secure\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join(".wcode/design/acceptance/AC-SEC-001.yaml"),
        "id: AC-SEC-001\ntitle: Escape is blocked\nstatement: Parent traversal is rejected.\nverification:\n  - kind: test\n    path: src/lib.rs\n    symbol: blocks_escape\n",
    )
    .unwrap();

    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let first = runtime
        .traceability_status("demo", &workspace, &index, &HashSet::new())
        .unwrap();
    assert_eq!(first.design_to_implementation.percent, 100);
    assert_eq!(first.acceptance_to_verification.percent, 100);

    let original_modified = std::fs::metadata(&source).unwrap().modified().unwrap();
    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&source)
        .unwrap();
    file.write_all(revised.as_bytes()).unwrap();
    file.sync_all().unwrap();
    file.set_times(FileTimes::new().set_modified(original_modified))
        .unwrap();
    drop(file);
    assert_eq!(
        std::fs::metadata(&source).unwrap().len(),
        revised.len() as u64
    );
    assert_eq!(
        std::fs::metadata(&source).unwrap().modified().unwrap(),
        original_modified
    );

    let second = runtime
        .traceability_status("demo", &workspace, &index, &HashSet::new())
        .unwrap();
    assert_eq!(
        second.design_to_implementation.percent, 0,
        "implementation symbol rewrite must invalidate traceability even when metadata is replayed"
    );
    assert_eq!(
        second.acceptance_to_verification.percent, 0,
        "test symbol rewrite must invalidate traceability even when metadata is replayed"
    );
    assert_eq!(second.complete_requirements, 0);
}
