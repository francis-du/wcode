use super::*;
use std::collections::HashSet;

#[test]
fn internal_structured_results_do_not_duplicate_payload_as_text() {
    let payload = json!({"body": "x".repeat(32 * 1024), "count": 7});
    let external = tool_result(payload.clone(), false);
    let internal = structured_tool_result(payload.clone(), false);

    assert_eq!(internal["structuredContent"], payload);
    assert_eq!(internal["isError"], false);
    assert!(internal.get("content").is_none());
    assert!(
        serde_json::to_vec(&internal).unwrap().len() * 3
            < serde_json::to_vec(&external).unwrap().len() * 2,
        "structured-only child results should avoid roughly one full payload copy"
    );
}

#[tokio::test]
async fn cancelled_blocking_worker_retains_its_real_permit_until_finished() {
    use std::sync::Arc;
    use std::time::Duration;
    let slots = Arc::new(tokio::sync::Semaphore::new(1));
    let permit = Arc::new(slots.clone().acquire_owned().await.unwrap().into());
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let parent = tokio::spawn(BLOCKING_PERMIT.scope(
        permit,
        run_blocking(move || {
            let _ = started_tx.send(());
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            Ok(json!({"done": true}))
        }),
    ));
    tokio::time::timeout(Duration::from_secs(3), started_rx)
        .await
        .unwrap()
        .unwrap();
    parent.abort();
    assert!(parent.await.unwrap_err().is_cancelled());
    assert_eq!(
        slots.available_permits(),
        0,
        "running work still owns the slot"
    );
    release_tx.send(()).unwrap();
    let permit = tokio::time::timeout(Duration::from_secs(3), slots.clone().acquire_owned())
        .await
        .unwrap()
        .unwrap();
    drop(permit);
    assert_eq!(slots.available_permits(), 1);
}

#[test]
fn cancelled_queued_blocking_worker_never_starts() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use std::time::Duration;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .unwrap();
    runtime.block_on(async {
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let blocker = tokio::task::spawn_blocking(move || {
            let _ = started_tx.send(());
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        });
        started_rx.await.unwrap();
        let slots = Arc::new(tokio::sync::Semaphore::new(1));
        let permit = Arc::new(slots.clone().acquire_owned().await.unwrap().into());
        let executed = Arc::new(AtomicBool::new(false));
        let worker_executed = executed.clone();
        let (queued_tx, queued_rx) = tokio::sync::oneshot::channel();
        let parent = tokio::spawn(BLOCKING_PERMIT.scope(permit, async move {
            let mut work = Box::pin(run_blocking(move || {
                worker_executed.store(true, Ordering::SeqCst);
                Ok(())
            }));
            std::future::poll_fn(|cx| {
                assert!(std::future::Future::poll(work.as_mut(), cx).is_pending());
                std::task::Poll::Ready(())
            })
            .await;
            let _ = queued_tx.send(());
            work.await
        }));
        queued_rx.await.unwrap();
        parent.abort();
        assert!(parent.await.unwrap_err().is_cancelled());
        release_tx.send(()).unwrap();
        blocker.await.unwrap();
        let permit = tokio::time::timeout(Duration::from_secs(3), slots.acquire_owned())
            .await
            .unwrap()
            .unwrap();
        assert!(!executed.load(Ordering::SeqCst));
        drop(permit);
    });
}

#[tokio::test]
async fn panicking_blocking_worker_releases_its_permit() {
    use std::sync::Arc;
    let slots = Arc::new(tokio::sync::Semaphore::new(1));
    let permit = Arc::new(slots.clone().acquire_owned().await.unwrap().into());
    let result: AnyResult<Value> = BLOCKING_PERMIT
        .scope(permit, run_blocking(|| panic!("synthetic blocking panic")))
        .await;
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("blocking task failed"));
    assert_eq!(slots.available_permits(), 1);
}

#[tokio::test]
async fn cancelled_execution_worker_retains_both_admission_permits() {
    use std::sync::Arc;
    use std::time::Duration;
    let harness = crate::harness::ToolHarness::new(2).unwrap();
    let permit = Arc::new(harness.acquire_tool(true).await.unwrap());
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let parent = tokio::spawn(BLOCKING_PERMIT.scope(
        permit,
        run_blocking(move || {
            let _ = started_tx.send(());
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            Ok(())
        }),
    ));
    tokio::time::timeout(Duration::from_secs(3), started_rx)
        .await
        .unwrap()
        .unwrap();
    parent.abort();
    assert!(parent.await.unwrap_err().is_cancelled());
    assert!(
        tokio::time::timeout(Duration::from_millis(25), harness.acquire_tool(true))
            .await
            .is_err()
    );
    let read = harness.acquire_tool(false).await.unwrap();
    drop(read);
    release_tx.send(()).unwrap();
    let next = tokio::time::timeout(Duration::from_secs(3), harness.acquire_tool(true))
        .await
        .unwrap()
        .unwrap();
    drop(next);
    let first = harness.acquire().await.unwrap();
    let second = harness.acquire().await.unwrap();
    drop((first, second));
}

fn assert_required_fields_exist(value: &Value, tool_name: &str) {
    match value {
        Value::Object(object) => {
            if let (Some(properties), Some(required)) = (
                object.get("properties").and_then(Value::as_object),
                object.get("required").and_then(Value::as_array),
            ) {
                for key in required.iter().filter_map(Value::as_str) {
                    assert!(
                        properties.contains_key(key),
                        "tool {tool_name} requires schema field {key} after it was removed from properties"
                    );
                }
            }
            for child in object.values() {
                assert_required_fields_exist(child, tool_name);
            }
        }
        Value::Array(items) => {
            for child in items {
                assert_required_fields_exist(child, tool_name);
            }
        }
        _ => {}
    }
}

fn assert_no_model_tuning_args(value: &Value, tool_name: &str) {
    match value {
        Value::Object(object) => {
            if let Some(properties) = object.get("properties").and_then(Value::as_object) {
                for key in MODEL_HIDDEN_TUNING_ARGS {
                    assert!(
                        !properties.contains_key(*key),
                        "tool {tool_name} exposes model tuning argument {key}"
                    );
                }
            }
            for child in object.values() {
                assert_no_model_tuning_args(child, tool_name);
            }
        }
        Value::Array(items) => {
            for child in items {
                assert_no_model_tuning_args(child, tool_name);
            }
        }
        _ => {}
    }
}

#[test]
fn core_tool_routing_survives_compact_description_limits() {
    let catalog = tools();
    for name in [
        "agent_context",
        "project_context",
        "parallel_tools",
        "file_outline",
        "find_symbol",
        "symbol_context",
        "verify_project",
        "read_files",
    ] {
        let tool = catalog.iter().find(|tool| tool["name"] == name).unwrap();
        assert!(
            !tool["description"].as_str().unwrap().ends_with('…'),
            "{name} routing was truncated"
        );
    }
    let project = catalog
        .iter()
        .find(|tool| tool["name"] == "project_context")
        .unwrap();
    assert!(project["description"]
        .as_str()
        .unwrap()
        .contains("Not a second mandatory"));
}

#[test]
fn tool_catalog_exposes_model_neutral_agent_hints() {
    let catalog = tools();
    let core = [
        "agent_context",
        "search_many",
        "scan_patterns",
        "search_syntax",
        "read_files",
        "apply_file_edits",
        "review_changes",
        "verify_project",
    ];
    for tool in catalog {
        let name = tool["name"].as_str().unwrap();
        if core.contains(&name) {
            assert_eq!(tool["_meta"]["dev.wcode/preloadRecommended"], true);
        } else {
            assert!(tool["_meta"].get("dev.wcode/preloadRecommended").is_none());
        }
    }
}

#[test]
fn tool_catalog_is_deterministic_compact_and_unique() {
    let first = tools();
    let second = tools();
    assert_eq!(first, second);
    for round in 0..200 {
        let current = tools();
        assert!(
            std::ptr::eq(first.as_ptr(), current.as_ptr()),
            "tool catalog storage was rebuilt in round {round}"
        );
        assert_eq!(first.len(), current.len());
    }

    let bytes = serde_json::to_vec(first).unwrap().len();
    assert!(bytes <= 60_000, "tool catalog is {bytes} bytes");

    let mut names = HashSet::new();
    for tool in first {
        let name = tool["name"].as_str().unwrap();
        assert!(names.insert(name), "duplicate tool name: {name}");
        let description = tool["description"].as_str().unwrap();
        assert!(
            description.chars().count() <= MAX_TOOL_DESCRIPTION_CHARS,
            "tool {name} description is too long"
        );
        if let Some(workspace) = tool["inputSchema"]["properties"].get("workspace") {
            assert_eq!(
                workspace["description"],
                "Only pass when switching away from the default Workspace."
            );
        }
        assert!(
            !serde_json::to_string(&tool["inputSchema"])
                .unwrap()
                .contains("\"default\":"),
            "tool {name} must not advertise model-visible default arguments"
        );
        assert_no_model_tuning_args(&tool["inputSchema"], name);
        assert_required_fields_exist(&tool["inputSchema"], name);
    }
    let agent_context = first
        .iter()
        .find(|tool| tool["name"] == "agent_context")
        .unwrap();
    assert!(agent_context["inputSchema"]["properties"]
        .get("budget")
        .is_none());
    let run_command = first
        .iter()
        .find(|tool| tool["name"] == "run_command")
        .unwrap();
    assert!(run_command["inputSchema"]["properties"]["program"]
        .get("maxLength")
        .is_none());
    assert!(names.contains("agent_context"));
    assert!(names.contains("semantic_navigation"));
    assert!(names.contains("verify_project"));
    assert!(names.contains("apply_file_edits"));
    let review = first
        .iter()
        .find(|tool| tool["name"] == "review_changes")
        .unwrap();
    assert!(review["inputSchema"]["properties"]
        .get("adversarial")
        .is_some());
}
