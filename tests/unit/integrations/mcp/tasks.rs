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

fn snapshot_live_command_stream(bytes: &[u8]) -> (String, usize, bool) {
    let mut stream = LiveCommandStream::default();
    stream.push(bytes, false);
    stream.snapshot()
}

#[test]
fn live_command_window_is_utf8_safe_bounded_and_redacts_before_tailing() {
    let input = format!(
        "{}\nmarker=尾部\n",
        "x".repeat(MAX_LIVE_COMMAND_STREAM_BYTES + 128)
    );
    let (tail, dropped, redacted) = snapshot_live_command_stream(input.as_bytes());
    assert!(tail.len() <= MAX_LIVE_COMMAND_STREAM_BYTES);
    assert!(dropped > 0);
    assert!(tail.trim_end().ends_with("marker=尾部"));
    assert!(!redacted);

    let secret = format!(
        "api_key={}\n",
        "s".repeat(MAX_LIVE_COMMAND_STREAM_BYTES + 128)
    );
    let (tail, dropped, redacted) = snapshot_live_command_stream(secret.as_bytes());
    let tail = tail.trim_end();
    assert_eq!(tail, "api_key= [REDACTED]");
    assert_eq!(dropped, 0);
    assert!(redacted);
    assert!(!tail.contains(&"s".repeat(128)));
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
