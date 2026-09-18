use super::*;

#[test]
fn graph_budget_ambiguity_cross_file_is_not_erased_by_truncation() {
    let dir = tempfile::tempdir().unwrap();
    for (path, source) in [
        ("a_target.rs", "pub fn shared_feature() {}\n"),
        ("b_caller.rs", "pub fn invoke() { shared_feature(); }\n"),
        ("z_shadow.rs", "pub fn shared_feature() {}\n"),
    ] {
        fs::write(dir.path().join(path), source).unwrap();
    }
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();
    for budget in [3, 2] {
        let snapshot = index
            .software_graph_from_paths(
                &workspace,
                vec![
                    "a_target.rs".into(),
                    "b_caller.rs".into(),
                    "z_shadow.rs".into(),
                ],
                false,
                budget,
                &HashSet::new(),
                &HashSet::new(),
            )
            .unwrap();
        assert_eq!(snapshot.truncated, budget < 3);
        assert!(
            !snapshot
                .graph
                .edges
                .iter()
                .any(|edge| edge.kind == EdgeKind::Calls),
            "budget={budget}: an omitted homonym cannot manufacture a unique call target"
        );
    }
}

#[test]
fn graph_budget_ambiguity_same_file_is_not_erased_by_truncation() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("main.rs"),
        "fn helper() {}\nfn invoke() { hidden::helper(); }\nmod hidden { pub fn helper() {} }\n",
    )
    .unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();
    for budget in [100, 2] {
        let snapshot = index
            .software_graph("demo", &workspace, ".", 10, budget)
            .unwrap();
        assert!(
            !snapshot
                .graph
                .edges
                .iter()
                .any(|edge| edge.kind == EdgeKind::Calls),
            "budget={budget}: a truncated nested homonym must not redirect a call"
        );
    }
}

#[test]
fn graph_budget_ambiguity_omitted_inner_caller_is_not_attributed_to_parent() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("main.rs"),
        "fn wrapper() {\n    fn inner() { helper(); }\n}\nfn helper() {}\n",
    )
    .unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();
    let mut priorities = HashSet::new();
    for name in ["wrapper", "helper"] {
        let found = index
            .find_symbol("demo", &workspace, name, ".", None, 10)
            .unwrap();
        priorities.insert(found["results"][0]["id"].as_str().unwrap().to_owned());
    }
    let snapshot = index
        .software_graph_from_paths(
            &workspace,
            vec!["main.rs".into()],
            false,
            2,
            &priorities,
            &HashSet::new(),
        )
        .unwrap();
    assert!(snapshot.truncated);
    assert!(
        !snapshot
            .graph
            .edges
            .iter()
            .any(|edge| edge.kind == EdgeKind::Calls),
        "the omitted inner function owns the call, not the retained wrapper"
    );
}

#[test]
fn graph_budget_ambiguity_preserves_explicit_cross_file_target() {
    let dir = tempfile::tempdir().unwrap();
    for (path, source) in [
        ("a_target.rs", "pub fn shared_feature() {}\n"),
        (
            "b_caller.rs",
            "pub fn invoke() { a_target::shared_feature(); }\n",
        ),
        ("z_shadow.rs", "pub fn shared_feature() {}\n"),
    ] {
        fs::write(dir.path().join(path), source).unwrap();
    }
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();
    for budget in [3, 2] {
        let snapshot = index
            .software_graph_from_paths(
                &workspace,
                vec![
                    "a_target.rs".into(),
                    "b_caller.rs".into(),
                    "z_shadow.rs".into(),
                ],
                false,
                budget,
                &HashSet::new(),
                &HashSet::new(),
            )
            .unwrap();
        let calls = snapshot
            .graph
            .edges
            .iter()
            .filter(|edge| edge.kind == EdgeKind::Calls)
            .collect::<Vec<_>>();
        assert_eq!(calls.len(), 1, "budget={budget}: keep the qualified target");
        assert_eq!(
            snapshot.graph.nodes[&calls[0].to].attributes["path"],
            "a_target.rs"
        );
        assert_eq!(calls[0].provenance.precision, GraphPrecision::Syntax);
    }
}

#[test]
fn software_graph_reuses_indexed_symbols_and_marks_syntax_precision() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("engine.rs"),
        "fn helper() -> u8 { 1 }\nfn compute() -> u8 { helper() }\n",
    )
    .unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();

    let snapshot = index
        .software_graph("demo", &workspace, ".", 100, 100)
        .unwrap();
    assert_eq!(snapshot.provider, "tree-sitter");
    assert_eq!(snapshot.precision, GraphPrecision::Syntax);
    assert_eq!(snapshot.files_indexed, 1);
    assert!(!snapshot.truncated);

    let file = snapshot
        .graph
        .nodes
        .values()
        .find(|node| node.kind == NodeKind::File)
        .unwrap();
    let helper = snapshot
        .graph
        .nodes
        .values()
        .find(|node| node.label == "helper")
        .unwrap();
    let compute = snapshot
        .graph
        .nodes
        .values()
        .find(|node| node.label == "compute")
        .unwrap();
    assert_eq!(helper.provenance.precision, GraphPrecision::Syntax);
    assert!(snapshot.graph.edges.iter().any(|edge| {
        edge.kind == EdgeKind::Defines && edge.from == file.id && edge.to == helper.id
    }));
    assert!(snapshot.graph.edges.iter().any(|edge| {
        edge.kind == EdgeKind::Calls && edge.from == compute.id && edge.to == helper.id
    }));
}

#[test]
fn software_graph_resolves_unique_cross_file_calls_at_syntax_precision() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("helper.rs"),
        "pub fn helper() -> u8 { 1 }\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("main.rs"),
        "fn compute() -> u8 { helper() }\n",
    )
    .unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();

    let snapshot = index
        .software_graph("demo", &workspace, ".", 100, 100)
        .unwrap();
    let helper = snapshot
        .graph
        .nodes
        .values()
        .find(|node| node.label == "helper")
        .unwrap();
    let compute = snapshot
        .graph
        .nodes
        .values()
        .find(|node| node.label == "compute")
        .unwrap();
    let edge = snapshot
        .graph
        .edges
        .iter()
        .find(|edge| {
            edge.kind == EdgeKind::Calls && edge.from == compute.id && edge.to == helper.id
        })
        .expect("unique cross-file call edge");
    assert_eq!(edge.provenance.precision, GraphPrecision::Syntax);
    assert_eq!(
        edge.provenance.provider,
        "tree-sitter/global-name-resolution"
    );
}

#[test]
fn software_graph_does_not_guess_untyped_receiver_method_calls() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("main.rs"),
        "struct Worker;\nimpl Worker { fn flush(&self) {} }\nstruct Cache;\nfn run(cache: &Cache) { cache.flush(); }\n",
    )
    .unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();

    let snapshot = index
        .software_graph("demo", &workspace, ".", 100, 100)
        .unwrap();
    let flush = snapshot
        .graph
        .nodes
        .values()
        .find(|node| node.label == "Worker::flush")
        .or_else(|| {
            snapshot
                .graph
                .nodes
                .values()
                .find(|node| node.label == "flush")
        })
        .unwrap();
    let run = snapshot
        .graph
        .nodes
        .values()
        .find(|node| node.label == "run")
        .unwrap();
    assert!(
        !snapshot.graph.edges.iter().any(|edge| {
            edge.kind == EdgeKind::Calls && edge.from == run.id && edge.to == flush.id
        }),
        "syntax-only graph must not infer receiver type from a unique method name"
    );
}
