use super::*;
use crate::task_store;

fn command_is_serving(port: u16) -> bool {
    use std::io::Read;
    let Ok(mut stream) = std::net::TcpStream::connect(("127.0.0.1", port)) else {
        return false;
    };
    stream
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    let mut reply = [0; 2];
    stream.read_exact(&mut reply).is_ok() && reply == *b"ok"
}

fn assert_native_ui_interop(value: &Value) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let input = tempfile::NamedTempFile::new().unwrap();
    fs::write(input.path(), serde_json::to_vec(value).unwrap()).unwrap();
    let output = std::process::Command::new("node")
        .arg(root.join("tests/unit/ui/jobs.cjs"))
        .arg(root)
        .arg(input.path())
        .output()
        .expect("Node is required for actual HTTP serializer interoperability");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["passed"], true);
}

#[tokio::test]
async fn web_jobs_real_command_output_and_cancellation_preserve_owner_and_workspace() {
    // Required runtime: absence must fail this test, never look like a pass.
    assert!(std::process::Command::new("node")
        .arg("--version")
        .output()
        .unwrap()
        .status
        .success());
    for ui_owned in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        fs::write(root.path().join("job.js"),
            "const net=require('net');console.log('海'.repeat(12000));console.error('ghp_'+'x'.repeat(40));const server=net.createServer(s=>{s.on('error',()=>{});s.end('ok');});server.listen(0,'127.0.0.1',()=>console.log('job-ready:'+server.address().port));setInterval(()=>{},1000);\n").unwrap();
        let state = fixture(&[root.path(), other.path()], true);
        let id = state.workspaces.default_id();
        let other_id = state
            .workspaces
            .roots()
            .into_iter()
            .find(|(key, _)| key != id)
            .unwrap()
            .0;
        let owner = if ui_owned {
            crate::mcp_tasks::monitor_ui_owner(&state)
        } else {
            "a".repeat(64)
        };
        let record = crate::mcp_tasks::start_tool_task_bound(
            state.clone(),
            json!({
                "name":"run_command", "arguments":{"workspace":id,"program":"node",
                    "args":["job.js"],"task_mode":true,"timeout_seconds":30}
            }),
            owner.clone(),
            None,
        )
        .await
        .unwrap();
        let path = format!("/intelligence/jobs/{}", record.task_id);
        let (port, detail) = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let (code, detail) = http(&state, id, "GET", &path, None, true).await;
                assert_eq!(code, StatusCode::OK, "{detail}");
                let port = detail["stdout"]["text"]
                    .as_str()
                    .unwrap()
                    .lines()
                    .find_map(|line| {
                        line.strip_prefix("job-ready:")
                            .and_then(|port| port.parse::<u16>().ok())
                    });
                if let Some(port) = port {
                    break (port, detail);
                }
                assert_eq!(detail["status"], "working", "{detail}");
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert!(command_is_serving(port));
        assert_eq!(detail["origin"], if ui_owned { "ui" } else { "mcp" });
        assert_eq!(detail["can_cancel"], ui_owned);
        assert_eq!(detail["success"], Value::Null);
        assert_eq!(detail["completion_is_not_success"], true);
        assert_eq!(detail["acceptance_ready"], false);
        assert!(detail["stdout"]["text"].as_str().unwrap().len() <= 32768);
        assert_eq!(detail["stdout"]["truncated"], true);
        let (_, list) = http(&state, id, "GET", "/intelligence/jobs", None, true).await;
        assert_eq!(list["items"].as_array().unwrap().len(), 1);
        assert_eq!(list["items"][0]["task_id"], record.task_id);
        assert_eq!(list["items"][0]["can_cancel"], ui_owned);
        for value in [&list, &detail] {
            let text = value.to_string();
            assert!(!text.contains(&owner));
            assert!(!text.contains(state.auth.instance_id()));
            assert!(!text.contains("job.js"));
            assert!(!text.contains(&"x".repeat(40)));
            for field in ["owner", "args", "params", "runtime_instance_id"] {
                assert!(value.get(field).is_none());
            }
        }
        assert_native_ui_interop(&json!({"workspace":id,"jobs":list,"job":detail}));
        let cancel = format!("{path}/cancel");
        for (method, url) in [("GET", path.as_str()), ("POST", cancel.as_str())] {
            assert_eq!(
                http(&state, &other_id, method, url, None, true).await.0,
                StatusCode::NOT_FOUND
            );
            assert_eq!(
                http(&state, id, method, url, None, false).await.0,
                StatusCode::UNAUTHORIZED
            );
        }
        let (code, cancelled) = http(
            &state,
            id,
            "POST",
            &cancel,
            Some(json!({"owner":owner,"origin":"ui"})),
            true,
        )
        .await;
        if ui_owned {
            assert_eq!(code, StatusCode::OK, "{cancelled}");
            assert_eq!(cancelled["status"], "cancelled");
        } else {
            assert_eq!(code, StatusCode::FORBIDDEN);
            if !command_is_serving(port) {
                tokio::time::sleep(Duration::from_millis(250)).await;
                let (_, denied_detail) = http(&state, id, "GET", &path, None, true).await;
                panic!(
                    "denied cancellation must not stop MCP child: status={}, stderr={}",
                    denied_detail["status"], denied_detail["stderr"]["text"]
                );
            }
            crate::mcp_tasks::cancel_task(&state, &record.task_id, &owner).unwrap();
        }
        tokio::time::timeout(Duration::from_secs(10), async {
            while command_is_serving(port) {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("native cancellation must stop the supervised child");
        let (_, retained) = http(&state, id, "GET", &path, None, true).await;
        assert_eq!(retained["status"], "cancelled");
        assert_eq!(retained["can_cancel"], false);
    }
}

#[tokio::test]
async fn web_jobs_discovery_recovers_a_real_verification_receipt_without_rerunning() {
    let root = tempfile::tempdir().unwrap();
    repository(root.path());
    let other = tempfile::tempdir().unwrap();
    repository(other.path());
    let state = fixture(&[root.path(), other.path()], true);
    let id = state.workspaces.default_id();
    let created = launch(&state, id).await;
    let task_id = created["task_id"].as_str().unwrap();
    let (_, workspace) = state.workspaces.select(Some(id)).unwrap();
    let mut foreign = TaskRecord::working(
        "b".repeat(64),
        id.into(),
        "verify_project".into(),
        state.auth.instance_id().into(),
    );
    foreign.complete(json!({"PRIVATE":"FOREIGN_RECORD"}));
    task_store::persist(&workspace, &foreign).unwrap();
    // A client that lost the POST reply has no ID until this server discovery.
    let (code, list) = http(
        &state,
        id,
        "GET",
        "/intelligence/verification/tasks",
        None,
        true,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{list}");
    assert_eq!(list["kind"], "verification_task_list");
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
    let recovered = list["items"][0]["task_id"].as_str().unwrap();
    assert_eq!(recovered, task_id);
    assert_eq!(list["items"][0]["origin"], "ui");
    assert!(!list.to_string().contains("FOREIGN_RECORD"));
    assert!(!list.to_string().contains(&foreign.task_id));
    finished(&state, id, recovered).await;
    let (code, verification) = http(
        &state,
        id,
        "GET",
        &format!("/intelligence/verification/{recovered}/result"),
        None,
        true,
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert_native_ui_interop(&json!({"workspace":id,"recovery":list,"verification":verification}));
    let evidence_before = crate::evidence_store::load(&workspace).unwrap().len();
    for _ in 0..3 {
        let (code, list) = http(
            &state,
            id,
            "GET",
            "/intelligence/verification/tasks",
            None,
            true,
        )
        .await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(list["items"].as_array().unwrap().len(), 1);
        assert_eq!(list["items"][0]["task_id"], task_id);
    }
    assert_eq!(
        crate::evidence_store::load(&workspace).unwrap().len(),
        evidence_before
    );
    let other_id = state
        .workspaces
        .roots()
        .into_iter()
        .find(|(key, _)| key != id)
        .unwrap()
        .0;
    let (_, empty) = http(
        &state,
        &other_id,
        "GET",
        "/intelligence/verification/tasks",
        None,
        true,
    )
    .await;
    assert!(empty["items"].as_array().unwrap().is_empty());
    for route in ["/intelligence/jobs", "/intelligence/verification/tasks"] {
        assert_eq!(
            http(&state, id, "GET", route, None, false).await.0,
            StatusCode::UNAUTHORIZED
        );
    }
}

#[tokio::test]
async fn web_jobs_readonly_observation_unknown_and_damaged_store_do_not_launch_or_reuse_success() {
    let root = tempfile::tempdir().unwrap();
    let state = fixture(&[root.path()], false);
    let id = state.workspaces.default_id();
    let (_, workspace) = state.workspaces.select(Some(id)).unwrap();
    let (_, empty) = http(&state, id, "GET", "/intelligence/jobs", None, true).await;
    assert!(empty["items"].as_array().unwrap().is_empty());
    let mut record = TaskRecord::working(
        "a".repeat(64),
        id.into(),
        "run_command".into(),
        "old-runtime".into(),
    );
    task_store::persist(&workspace, &record).unwrap();
    let (_, list) = http(&state, id, "GET", "/intelligence/jobs", None, true).await;
    assert_eq!(list["items"][0]["status"], "unknown");
    assert_eq!(list["items"][0]["can_cancel"], false);
    record.complete(json!({"success":true,"stdout":"old success"}));
    task_store::persist(&workspace, &record).unwrap();
    let directory = crate::evidence_store::workspace_state_directory(&workspace)
        .unwrap()
        .join("mcp-tasks")
        .join(&record.task_id);
    fs::write(
        directory.join(format!("{}-{}.json", "9".repeat(20), "a".repeat(24))),
        "{damaged",
    )
    .unwrap();
    let (_, list) = http(&state, id, "GET", "/intelligence/jobs", None, true).await;
    assert!(list["items"].as_array().unwrap().is_empty());
    assert_eq!(list["truncated"], true);
    let (code, detail) = http(
        &state,
        id,
        "GET",
        &format!("/intelligence/jobs/{}", record.task_id),
        None,
        true,
    )
    .await;
    assert_eq!(code, StatusCode::NOT_FOUND);
    assert!(!detail.to_string().contains("old success"));
    assert!(crate::evidence_store::load(&workspace).unwrap().is_empty());
}
