use super::*;

#[path = "resume.rs"]
mod resume;

#[test]
fn oversized_task_result_becomes_a_durable_failure() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let mut record = TaskRecord::working(
        "a".repeat(64),
        "demo".into(),
        "semantic_provider_refresh".into(),
        "runtime-a".into(),
    );
    task_store::persist(&workspace, &record).unwrap();
    record.complete(json!({"payload": "x".repeat(4 * 1024 * 1024)}));
    persist_task_result(&workspace, &mut record).unwrap();
    let loaded = task_store::load(&workspace, &record.task_id)
        .unwrap()
        .unwrap();
    assert_eq!(loaded.status, TaskStatus::Failed);
    assert!(loaded.result.is_none());
    assert!(loaded.error.unwrap()["message"]
        .as_str()
        .unwrap()
        .contains("could not be persisted"));
}

#[test]
fn successful_task_result_is_not_replaced_by_recovery() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let mut record = TaskRecord::working(
        "a".repeat(64),
        "demo".into(),
        "semantic_provider_refresh".into(),
        "runtime-a".into(),
    );
    let result = json!({"isError": false, "structuredContent": {"runs": []}});
    record.complete(result.clone());
    persist_task_result(&workspace, &mut record).unwrap();
    let loaded = task_store::load(&workspace, &record.task_id)
        .unwrap()
        .unwrap();
    assert_eq!(loaded.status, TaskStatus::Completed);
    assert_eq!(loaded.result, Some(result));
}

#[tokio::test]
async fn finished_task_handle_is_not_a_live_worker() {
    let runtime = TaskRuntime::default();
    let task = tokio::spawn(async {});
    let handle = task.abort_handle();
    task.await.unwrap();
    runtime.register("finished".into(), "demo".into(), handle);
    assert!(!runtime.running("finished"));
    runtime.remove("finished");
    assert!(!runtime.running("finished"));
}
