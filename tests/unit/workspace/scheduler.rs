use super::*;
use serde_json::json;

fn model(tool: &str, arguments: Value) -> WorkloadResources {
    resource_model("demo", tool, &arguments).unwrap()
}

fn layers(workloads: Vec<WorkloadResources>) -> Vec<Vec<usize>> {
    let indexed = workloads.into_iter().enumerate().collect::<Vec<_>>();
    dependency_graph(&indexed, indexed.len())
        .layers(&(0..indexed.len()).collect())
        .unwrap()
}

#[test]
fn ready_successors_do_not_wait_for_an_unrelated_slow_branch() {
    let workloads = vec![
        (0, model("create_file", json!({"path":"fast.txt"}))),
        (1, model("create_file", json!({"path":"slow.txt"}))),
        (2, model("read_file", json!({"path":"fast.txt"}))),
        (3, model("read_file", json!({"path":"slow.txt"}))),
    ];
    let graph = dependency_graph(&workloads, 4);
    assert_eq!(
        graph.ready(&BTreeSet::from([0, 1, 2, 3]), &BTreeSet::new()),
        vec![0, 1]
    );
    assert_eq!(
        graph.ready(&BTreeSet::from([2, 3]), &BTreeSet::from([0])),
        vec![2]
    );
    assert_eq!(
        graph.ready(&BTreeSet::from([3]), &BTreeSet::from([0, 2])),
        Vec::<usize>::new()
    );
    assert_eq!(
        graph.ready(&BTreeSet::from([3]), &BTreeSet::from([0, 1, 2])),
        vec![3]
    );

    let mut schedule = graph
        .completion_schedule(&BTreeSet::from([0, 1, 2, 3]))
        .unwrap();
    assert_eq!(schedule.take_ready(), vec![0, 1]);
    schedule.complete(0);
    assert_eq!(schedule.take_ready(), vec![2]);
    schedule.complete(2);
    assert!(schedule.take_ready().is_empty());
    schedule.complete(1);
    assert_eq!(schedule.take_ready(), vec![3]);
}

#[test]
fn completion_schedule_matches_naive_readiness_across_200_graphs() {
    for round in 0..200usize {
        let task_count = 8 + (round % 17);
        let mut predecessors = vec![BTreeSet::new(); task_count];
        for (index, dependencies) in predecessors.iter_mut().enumerate().skip(1) {
            if (index + round) % 3 != 0 {
                dependencies.insert(index - 1);
            }
            if index >= 2 && (index * 7 + round) % 5 == 0 {
                dependencies.insert(index - 2);
            }
            if index >= 4 && (index + round * 3) % 7 == 0 {
                dependencies.insert(index - 4);
            }
        }
        let graph = DependencyGraph { predecessors };
        let mut pending = (0..task_count).collect::<BTreeSet<_>>();
        let mut completed = BTreeSet::new();
        let mut schedule = graph.completion_schedule(&pending).unwrap();
        while !pending.is_empty() {
            let expected = graph.ready(&pending, &completed);
            let actual = schedule.take_ready();
            assert_eq!(actual, expected, "round {round}");
            assert!(!actual.is_empty(), "round {round} stalled");
            for index in actual {
                pending.remove(&index);
                completed.insert(index);
                schedule.complete(index);
            }
        }
    }
}

#[test]
fn parent_and_subspace_aliases_share_resource_dependencies() {
    let parent =
        model("write_file", json!({"path":"project/src/lib.rs"})).in_root(Path::new("root"));
    let child = resource_model("child", "read_file", &json!({"path":"src/lib.rs"}))
        .unwrap()
        .in_root(Path::new("root/project"));
    assert_eq!(layers(vec![parent.clone(), child]), vec![vec![0], vec![1]]);
    let unrelated = model("read_file", json!({"path":"src/lib.rs"})).in_root(Path::new("other"));
    assert_eq!(layers(vec![parent, unrelated]), vec![vec![0, 1]]);
}

#[test]
fn coalescing_preserves_intervening_reads_and_independent_work() {
    let edit = |text: &str| {
        json!({"tool":"apply_edits","arguments":{
            "path":"shared.txt","expected_sha256":"same",
            "edits":[{"old_text":text,"new_text":"updated"}]
        }})
    };
    let items = vec![
        edit("first"),
        json!({"tool":"read_file","arguments":{"path":"shared.txt"}}),
        edit("last"),
    ];
    assert!(coalesce_apply_edits("demo", &items)
        .unwrap_err()
        .contains("intervening dependent"));
    let items = vec![
        edit("first"),
        json!({"tool":"read_file","arguments":{"path":"other.txt"}}),
        edit("last"),
    ];
    let (_, aliases, skipped) = coalesce_apply_edits("demo", &items).unwrap();
    assert_eq!(aliases[&0], vec![(2, "task-3".to_owned())]);
    assert_eq!(skipped, HashSet::from([2]));
}

#[test]
fn coalesced_transaction_limit_is_checked_before_execution() {
    let edits = (0..128)
        .map(|index| json!({"old_text":format!("line-{index}"),"new_text":"changed"}))
        .collect::<Vec<_>>();
    let items = vec![
        json!({"tool":"apply_edits","arguments":{"path":"shared.txt","expected_sha256":"same","edits":edits}}),
        json!({"tool":"apply_edits","arguments":{"path":"shared.txt","expected_sha256":"same","edits":[{"old_text":"extra","new_text":"changed"}]}}),
    ];
    assert!(coalesce_apply_edits("demo", &items)
        .unwrap_err()
        .contains("128-edit"));
}

#[test]
fn independent_reads_and_writes_fan_out() {
    assert_eq!(
        layers(vec![
            model("read_file", json!({"path":"a.rs"})),
            model("read_file", json!({"path":"b.rs"})),
        ]),
        vec![vec![0, 1]]
    );
    assert_eq!(
        layers(vec![
            model("create_file", json!({"path":"a.rs"})),
            model("create_file", json!({"path":"b.rs"})),
        ]),
        vec![vec![0, 1]]
    );
}

#[test]
fn same_path_read_write_and_parent_child_serialize() {
    assert_eq!(
        layers(vec![
            model("read_file", json!({"path":"src/lib.rs"})),
            model("write_file", json!({"path":"src/lib.rs"})),
        ]),
        vec![vec![0], vec![1]]
    );
    assert_eq!(
        layers(vec![
            model("write_file", json!({"path":"src"})),
            model("create_file", json!({"path":"src/domain/a.rs"})),
        ]),
        vec![vec![0], vec![1]]
    );
}

#[test]
fn move_delete_and_directory_creation_dependencies_are_ordered() {
    assert_eq!(
        layers(vec![
            model("move_path", json!({"source":"a.rs","destination":"b.rs"})),
            model("write_file", json!({"path":"b.rs"})),
        ]),
        vec![vec![0], vec![1]]
    );
    assert_eq!(
        layers(vec![
            model("read_file", json!({"path":"src/domain/a.rs"})),
            model("delete_path", json!({"path":"src/domain"})),
        ]),
        vec![vec![0], vec![1]]
    );
    assert_eq!(
        layers(vec![
            model("create_file", json!({"path":"src/domain/a.rs"})),
            model("create_directory", json!({"path":"src/domain"})),
        ]),
        vec![vec![1], vec![0]],
        "parent directory creation must precede the child create even when submitted later"
    );
}

#[test]
fn same_file_same_sha_coalesces_and_conflicts_are_rejected() {
    let items = vec![
        json!({"id":"first","tool":"apply_edits","arguments":{"path":"shared.txt","expected_sha256":"same","edits":[{"old_text":"same","new_text":"FIRST","start_line":1,"end_line":1}]}}),
        json!({"id":"last","tool":"apply_edits","arguments":{"path":"./shared.txt","expected_sha256":"same","edits":[{"old_text":"same","new_text":"LAST","start_line":3,"end_line":3}]}}),
    ];
    let (prepared, aliases, skipped) = coalesce_apply_edits("demo", &items).unwrap();
    assert_eq!(
        prepared[0]["arguments"]["edits"].as_array().unwrap().len(),
        2
    );
    assert_eq!(aliases.values().map(Vec::len).sum::<usize>(), 1);
    assert!(skipped.contains(&1));

    let different_sha = vec![
        json!({"tool":"apply_edits","arguments":{"path":"shared.txt","expected_sha256":"one","edits":[{"old_text":"a","new_text":"A","start_line":1,"end_line":1}]}}),
        json!({"tool":"apply_edits","arguments":{"path":"shared.txt","expected_sha256":"two","edits":[{"old_text":"b","new_text":"B","start_line":2,"end_line":2}]}}),
    ];
    assert!(coalesce_apply_edits("demo", &different_sha)
        .unwrap_err()
        .contains("different revisions"));

    let overlap = vec![
        json!({"tool":"apply_edits","arguments":{"path":"shared.txt","expected_sha256":"one","edits":[{"old_text":"a","new_text":"A","start_line":1,"end_line":3}]}}),
        json!({"tool":"apply_edits","arguments":{"path":"shared.txt","expected_sha256":"one","edits":[{"old_text":"b","new_text":"B","start_line":3,"end_line":4}]}}),
    ];
    assert!(coalesce_apply_edits("demo", &overlap)
        .unwrap_err()
        .contains("overlapping line ranges"));

    let ambiguous = vec![
        json!({"tool":"apply_edits","arguments":{"path":"shared.txt","expected_sha256":"one","edits":[{"old_text":"same","new_text":"A"}]}}),
        json!({"tool":"apply_edits","arguments":{"path":"shared.txt","expected_sha256":"one","edits":[{"old_text":"same","new_text":"B"}]}}),
    ];
    assert!(coalesce_apply_edits("demo", &ambiguous)
        .unwrap_err()
        .contains("ambiguous duplicate old_text"));
}
