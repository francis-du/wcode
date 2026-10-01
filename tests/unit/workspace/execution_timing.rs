//! Real queued child processes: waiting has its own bound, not the runtime budget.
use super::*;

fn budget_fixture() -> (tempfile::TempDir, Workspace, String) {
    let (root, workspace, program) = verification_count_fixture();
    // Authorize only this isolated synthetic Workspace, not the user's host.
    workspace
        .authorization
        .set_workspace_commands_granted(&workspace.authorization_workspace_id(), true);
    (root, workspace, program)
}

fn budget_test_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

async fn queued_runtime_budget(entry: &str) {
    // These tests deliberately occupy the shared host queue. Serialize only
    // the fixtures so parallel test runners cannot deadlock one another.
    let _serial = budget_test_lock().lock().await;
    let (root, workspace, program) = budget_fixture();
    let governor = crate::resource::global();
    let mut held = Vec::new();
    for _ in 0..crate::resource::limits().child_processes {
        held.push(
            governor
                .acquire_child_for_workspace(workspace.root())
                .await
                .unwrap(),
        );
    }
    let args = vec!["--filter".to_owned(), "queue-budget".to_owned()];
    let operation = async {
        match entry {
            "command" => workspace.run_command(&program, &args, ".", 2).await,
            "verification" => {
                workspace
                    .run_workspace_verification_executable(&program, &args, ".", 2)
                    .await
            }
            "runtime" => {
                workspace
                    .run_trusted_runtime_command(&program, &args, ".", 2)
                    .await
            }
            _ => unreachable!(),
        }
    };
    tokio::pin!(operation);
    assert!(
        tokio::time::timeout(Duration::from_millis(1600), operation.as_mut())
            .await
            .is_err(),
        "a queued command must not bypass the occupied process slots"
    );
    assert!(!root.path().join("queue-started.txt").exists());
    drop(held);
    let result = tokio::time::timeout(Duration::from_secs(4), operation)
        .await
        .expect("released queue must make progress")
        .expect("queue was released before its independent two-second bound");
    assert!(
        result.success && !result.timed_out,
        "{entry}: queue wait consumed the runtime budget: {result:?}"
    );
    assert!(result.process_queue_wait_ms >= 1500);
    assert!(result.stdout.contains("queue-budget-finished"));
    assert_eq!(
        std::fs::read_to_string(root.path().join("queue-finished.txt")).unwrap(),
        "finished"
    );
}

#[tokio::test]
async fn queued_command_retains_its_full_runtime_budget() {
    queued_runtime_budget("command").await;
}

#[tokio::test]
async fn queued_verification_retains_its_full_runtime_budget() {
    queued_runtime_budget("verification").await;
}

#[tokio::test]
async fn queued_runtime_executor_retains_its_full_runtime_budget() {
    queued_runtime_budget("runtime").await;
}

#[tokio::test]
async fn cancelled_queued_command_never_starts_and_releases_its_waiter() {
    let _serial = budget_test_lock().lock().await;
    let (root, workspace, program) = budget_fixture();
    let governor = crate::resource::global();
    let capacity = crate::resource::limits().child_processes;
    let mut held = Vec::new();
    for _ in 0..capacity {
        held.push(
            governor
                .acquire_child_for_workspace(workspace.root())
                .await
                .unwrap(),
        );
    }
    let args = vec!["--filter".to_owned(), "queue-budget".to_owned()];
    let mut operation = Box::pin(workspace.run_command(&program, &args, ".", 2));
    assert!(
        tokio::time::timeout(Duration::from_millis(40), operation.as_mut())
            .await
            .is_err()
    );
    drop(operation);
    drop(held);
    let mut reclaimed = Vec::new();
    for _ in 0..capacity {
        reclaimed.push(
            tokio::time::timeout(
                Duration::from_secs(2),
                governor.acquire_child_for_workspace(workspace.root()),
            )
            .await
            .expect("cancellation must remove the pending queue reservation")
            .unwrap(),
        );
    }
    assert!(!root.path().join("queue-started.txt").exists());
    assert!(!root.path().join("queue-finished.txt").exists());
}
