use super::*;
use std::sync::mpsc;

struct HeldJournal {
    _root: tempfile::TempDir,
    release: mpsc::Sender<()>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl HeldJournal {
    fn unrelated() -> Self {
        let root = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(root.path(), true, false).unwrap();
        let gate = crate::engineering_journal::journal_access(&workspace).unwrap();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (release, receiver) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let _guard = gate.lock().unwrap();
            ready_tx.send(()).unwrap();
            let _ = receiver.recv();
        });
        ready_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        Self {
            _root: root,
            release,
            worker: Some(worker),
        }
    }
}

impl Drop for HeldJournal {
    fn drop(&mut self) {
        let _ = self.release.send(());
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}

#[tokio::test]
async fn unrelated_workspace_journal_does_not_block_single_slot_read_fanout() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("one.txt"), "one").unwrap();
    std::fs::write(root.path().join("two.txt"), "two").unwrap();
    let state = state(root.path(), 1);
    let holder = HeldJournal::unrelated();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        call_tool(
            &state,
            json!({
                "name":"parallel_tools", "arguments":{"tasks":[
                    {"id":"one","tool":"read_file","arguments":{"path":"one.txt"}},
                    {"id":"missing","tool":"read_file","arguments":{"path":"missing.txt"}},
                    {"id":"two","tool":"read_file","arguments":{"path":"two.txt"}}
                ]}
            }),
        ),
    )
    .await;
    drop(holder); // Always release and reap the baseline global-lock worker.
    let result = result
        .expect("unrelated journal must not delay two-second fanout")
        .unwrap();
    assert_eq!(result["structuredContent"]["succeeded"], 2);
    assert_eq!(result["structuredContent"]["failed"], 1);
    assert_eq!(state.harness.admission_snapshot().slots_in_use, 0);
}

#[tokio::test]
async fn unrelated_workspace_journal_does_not_block_dependency_write_fanout() {
    for cap in [1, 4] {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("exists.txt"), "original").unwrap();
        let state = state(root.path(), cap);
        let holder = HeldJournal::unrelated();
        let result = tokio::time::timeout(std::time::Duration::from_secs(3), call_tool(&state, json!({
            "name":"parallel_tools", "arguments":{"tasks":[
                {"id":"fail","tool":"create_file","arguments":{"path":"exists.txt","content":"replacement"}},
                {"id":"dependent","tool":"move_path","arguments":{"source":"exists.txt","destination":"moved.txt"}},
                {"id":"transitive","tool":"read_file","arguments":{"path":"moved.txt"}},
                {"id":"independent","tool":"create_file","arguments":{"path":"independent.txt","content":"ok"}}
            ]}
        }))).await;
        drop(holder);
        let result = result
            .expect("unrelated journal must not delay three-second fanout")
            .unwrap();
        let data = &result["structuredContent"];
        assert_eq!(data["succeeded"], 1);
        assert_eq!(data["failed"], 3);
        for index in [1, 2] {
            assert!(data["items"][index]["error"]
                .as_str()
                .unwrap()
                .contains("dependency failed"));
        }
        assert_eq!(
            std::fs::read_to_string(root.path().join("exists.txt")).unwrap(),
            "original"
        );
        assert!(!root.path().join("moved.txt").exists());
        assert_eq!(
            std::fs::read_to_string(root.path().join("independent.txt")).unwrap(),
            "ok"
        );
        assert_eq!(state.harness.admission_snapshot().slots_in_use, 0);
    }
}

fn state(root: &std::path::Path, cap: usize) -> AppState {
    let workspaces = Workspaces::new([root], true, false).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
        workspaces,
        harness: ToolHarness::new(cap).unwrap(),
        monitor: TaskMonitor::new([workspace_id]),
        tasks: TaskRuntime::default(),
    }
}
