//! Real queued child processes: waiting has its own bound, not the runtime budget.
use super::*;

fn budget_fixture() -> (tempfile::TempDir, Workspace, String) {
    let (root, workspace, program) = verification_count_fixture();
    // Use phpunit's existing no-argument verification shape. A fabricated
    // --filter argument enters the broad-command sandbox instead and tests
    // platform sandbox availability, not process queue timing.
    std::fs::write(root.path().join("queue-budget-fixture.txt"), "queue-budget").unwrap();
    assert!(workspace.workspace_program_available(&program));
    assert!(workspace.verification_command_shape_allowed(&program, &[]));
    // Authorize only this isolated synthetic Workspace, not the user's host.
    workspace
        .authorization
        .set_workspace_commands_granted(&workspace.authorization_workspace_id(), true);
    (root, workspace, program)
}

async fn run_isolated(test: &str) -> bool {
    const MARKER: &str = "WCODE_TEST_QUEUE_PROCESS";
    if std::env::var(MARKER).as_deref() == Ok(test) {
        return false;
    }
    // Occupying every global process slot in the main test process can delay
    // unrelated tests. An exact-filtered child has its own production governor;
    // the two-second queue/runtime bounds below remain unchanged.
    let module = module_path!().split_once("::").unwrap().1;
    let name = format!("{module}::{test}");
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([&name, "--exact", "--nocapture", "--test-threads=1"])
        .env(MARKER, test)
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(60), command.output())
        .await
        .expect("isolated queue test exceeded its deadline")
        .expect("isolated queue test could not start");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{name}: {stdout}\n{stderr}");
    assert!(
        stdout.contains("test result: ok. 1 passed; 0 failed; 0 ignored;"),
        "the isolated process must execute exactly one real test: {stdout}"
    );
    true
}

async fn queued_runtime_budget(entry: &str) {
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
    let args: Vec<String> = Vec::new();
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
    if run_isolated("queued_command_retains_its_full_runtime_budget").await {
        return;
    }
    queued_runtime_budget("command").await;
}

#[tokio::test]
async fn queued_verification_retains_its_full_runtime_budget() {
    if run_isolated("queued_verification_retains_its_full_runtime_budget").await {
        return;
    }
    queued_runtime_budget("verification").await;
}

#[tokio::test]
async fn queued_runtime_executor_retains_its_full_runtime_budget() {
    if run_isolated("queued_runtime_executor_retains_its_full_runtime_budget").await {
        return;
    }
    queued_runtime_budget("runtime").await;
}

#[tokio::test]
async fn cancelled_queued_command_never_starts_and_releases_its_waiter() {
    if run_isolated("cancelled_queued_command_never_starts_and_releases_its_waiter").await {
        return;
    }
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
    let args: Vec<String> = Vec::new();
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
