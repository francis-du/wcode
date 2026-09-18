use super::*;

#[path = "budget_perf.rs"]
mod budget_perf;

fn exact_ids(
    index: &CodeIndex,
    workspace: &Workspace,
    targets: &[(&str, &str)],
) -> HashSet<String> {
    targets
        .iter()
        .map(|(path, name)| {
            index
                .resolve_symbol(workspace, path, name)
                .unwrap()
                .expect("fixture symbol")
                .id
        })
        .collect()
}

#[test]
fn graph_budget_reserves_priority_across_files() {
    for (fillers, budget) in [(16, 3), (6_200, 5_000)] {
        let root = tempfile::tempdir().unwrap();
        let mut source = "pub fn first_target() -> usize { 1 }\n".to_owned();
        for index in 0..fillers {
            source.push_str(&format!("pub fn filler_{index:04}() {{}}\n"));
        }
        fs::write(root.path().join("first.rs"), source).unwrap();
        fs::write(
            root.path().join("second.rs"),
            "pub fn second_target() -> usize { 2 }\n",
        )
        .unwrap();
        fs::write(
            root.path().join("caller.rs"),
            "pub fn invokes_both() -> usize {\n    first_target() + second_target()\n}\n",
        )
        .unwrap();
        let workspace = Workspace::new(root.path(), false, false).unwrap();
        let index = CodeIndex::new().unwrap();
        let ids = exact_ids(
            &index,
            &workspace,
            &[("first.rs", "first_target"), ("second.rs", "second_target")],
        );
        let names = HashSet::from(["first_target".to_owned(), "second_target".to_owned()]);
        for paths in [
            ["first.rs", "second.rs", "caller.rs"],
            ["caller.rs", "first.rs", "second.rs"],
            ["second.rs", "caller.rs", "first.rs"],
        ] {
            let graph = index
                .software_graph_from_paths(
                    &workspace,
                    paths.map(str::to_owned).to_vec(),
                    false,
                    budget,
                    &ids,
                    &names,
                )
                .unwrap();
            for expected in ["first_target", "second_target", "invokes_both"] {
                assert!(
                    graph
                        .graph
                        .nodes
                        .values()
                        .any(|node| node.label == expected),
                    "missing {expected}: budget={budget}, order={paths:?}"
                );
            }
            assert_eq!(
                graph
                    .graph
                    .nodes
                    .values()
                    .filter(|node| node.kind != NodeKind::File)
                    .count(),
                budget
            );
            assert_eq!(
                graph
                    .graph
                    .edges
                    .iter()
                    .filter(|edge| edge.kind == EdgeKind::Calls)
                    .count(),
                2
            );
            assert!(graph.truncated);
        }
    }
}

#[test]
fn graph_budget_keeps_exact_target_ahead_of_many_callers() {
    let root = tempfile::tempdir().unwrap();
    let mut source = (0..10)
        .map(|n| format!("pub fn caller_{n}() {{\n    target_feature();\n}}\n"))
        .collect::<String>();
    source.push_str("pub fn target_feature() {}\n");
    fs::write(root.path().join("all.rs"), source).unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();
    let ids = exact_ids(&index, &workspace, &[("all.rs", "target_feature")]);
    let names = HashSet::from(["target_feature".to_owned()]);
    let graph = index
        .software_graph_from_paths(
            &workspace,
            vec!["all.rs".to_owned()],
            false,
            2,
            &ids,
            &names,
        )
        .unwrap();
    assert!(
        graph
            .graph
            .nodes
            .values()
            .any(|node| node.label == "target_feature"),
        "exact definition must precede callers even at the end of a file"
    );
    assert_eq!(
        graph
            .graph
            .nodes
            .values()
            .filter(|node| node.kind != NodeKind::File)
            .count(),
        2
    );
    assert_eq!(
        graph
            .graph
            .edges
            .iter()
            .filter(|edge| edge.kind == EdgeKind::Calls)
            .count(),
        1
    );
    assert!(graph.truncated);
}

#[test]
fn graph_budget_preserves_unprioritized_order_and_hard_limit() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("all.rs"),
        "pub fn first() {}\npub fn second() {}\npub fn third() {}\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();
    let ids = exact_ids(
        &index,
        &workspace,
        &[
            ("all.rs", "first"),
            ("all.rs", "second"),
            ("all.rs", "third"),
        ],
    );
    for priorities in [HashSet::new(), ids] {
        let graph = index
            .software_graph_from_paths(
                &workspace,
                vec!["all.rs".to_owned()],
                false,
                2,
                &priorities,
                &HashSet::new(),
            )
            .unwrap();
        assert!(graph.graph.nodes.values().any(|node| node.label == "first"));
        assert!(graph
            .graph
            .nodes
            .values()
            .any(|node| node.label == "second"));
        assert!(!graph.graph.nodes.values().any(|node| node.label == "third"));
        assert!(graph.truncated);
    }
}
