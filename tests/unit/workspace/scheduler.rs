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
