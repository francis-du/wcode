use super::*;

fn git(root: &std::path::Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .current_dir(root)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn design_fixture(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    for directory in [
        ".wcode/design/requirements",
        ".wcode/design/components",
        ".wcode/design/acceptance",
    ] {
        std::fs::create_dir_all(root.join(directory)).unwrap();
    }
    std::fs::write(
        root.join("src/lib.rs"),
        "fn secure(path: &str) -> bool { !path.contains(\"..\") }\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn blocks_escape() { assert!(!super::secure(\"../secret\")); }\n}\n",
    )
    .unwrap();
    for index in 0..3 {
        std::fs::write(
            root.join(format!(".wcode/design/requirements/REQ-SEC-{index:03}.yaml")),
            format!(
                "id: REQ-SEC-{index:03}\ntitle: Workspace isolation {index}\nintent: Paths must stay inside the workspace.\npriority: critical\nimplemented_by:\n  - component:workspace-security\nacceptance:\n  - AC-SEC-{index:03}\n"
            ),
        )
        .unwrap();
        std::fs::write(
            root.join(format!(".wcode/design/acceptance/AC-SEC-{index:03}.yaml")),
            format!(
                "id: AC-SEC-{index:03}\ntitle: Escape is blocked {index}\nstatement: Parent traversal is rejected.\nverification:\n  - kind: test\n    path: src/lib.rs\n    symbol: blocks_escape\n"
            ),
        )
        .unwrap();
    }
    std::fs::write(
        root.join(".wcode/design/components/workspace-security.yaml"),
        "id: component:workspace-security\nname: Workspace Security\nimplementation:\n  - kind: symbol\n    path: src/lib.rs\n    symbol: secure\n",
    )
    .unwrap();
}

fn exec_state(root: &std::path::Path) -> AppState {
    let workspaces = Workspaces::new([root], true, true).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
        workspaces,
        harness: ToolHarness::new(4).unwrap(),
        monitor: TaskMonitor::new([workspace_id]),
        tasks: TaskRuntime::default(),
    }
}

async fn traceability_status(state: &AppState, arguments: Value) -> Result<Value, String> {
    call_tool(
        state,
        json!({"name": "traceability_status", "arguments": arguments}),
    )
    .await
}

#[tokio::test]
async fn traceability_status_defaults_to_summary_and_restores_full_references() {
    let root = tempfile::tempdir().unwrap();
    design_fixture(root.path());
    let state = batch_test_state(root.path());

    let response = traceability_status(&state, json!({})).await.unwrap();
    assert_eq!(response["isError"], false, "{response}");
    let summary = &response["structuredContent"];
    assert_eq!(summary["detail"], "summary");
    let requirements = summary["requirements"].as_array().unwrap();
    assert!(requirements.len() >= 3, "{summary}");
    for requirement in requirements {
        for key in ["implementation", "verification"] {
            let total = requirement[format!("{key}_total")].as_u64().unwrap();
            let resolved = requirement[format!("{key}_resolved")].as_u64().unwrap();
            let unresolved = requirement[format!("{key}_unresolved")].as_u64().unwrap();
            assert_eq!(resolved + unresolved, total, "{requirement}");
            let sample = requirement[key].as_array().unwrap();
            assert!(
                sample.len() as u64 <= total.min(4),
                "the summary keeps a bounded reference sample"
            );
            for reference in sample {
                if let Some(message) = reference["message"].as_str() {
                    assert!(
                        message.chars().count() <= 160,
                        "summary reference messages are bounded"
                    );
                }
            }
        }
    }

    let response = traceability_status(&state, json!({"detail": "full"}))
        .await
        .unwrap();
    assert_eq!(response["isError"], false, "{response}");
    let full = &response["structuredContent"];
    assert!(full.get("detail").is_none());
    for requirement in full["requirements"].as_array().unwrap() {
        assert!(requirement.get("implementation_total").is_none());
        assert!(requirement.get("verification_total").is_none());
    }

    let rejected = traceability_status(&state, json!({"detail": "everything"})).await;
    assert!(rejected.is_err(), "unknown detail must fail closed");
}

#[tokio::test]
async fn software_context_summary_bounds_the_embedded_traceability() {
    let root = tempfile::tempdir().unwrap();
    design_fixture(root.path());
    let state = batch_test_state(root.path());

    let response = call_tool(
        &state,
        json!({"name": "software_context", "arguments": {"query": "workspace isolation"}}),
    )
    .await
    .unwrap();
    assert_eq!(response["isError"], false, "{response}");
    let summary = &response["structuredContent"];
    let coverage = &summary["coverage"];
    assert_eq!(coverage["detail"], "summary");
    let requirements = coverage["requirements"].as_array().unwrap();
    assert!(!requirements.is_empty(), "{coverage}");
    for requirement in requirements {
        for key in ["implementation", "verification"] {
            let total = requirement[format!("{key}_total")].as_u64().unwrap();
            let resolved = requirement[format!("{key}_resolved")].as_u64().unwrap();
            let unresolved = requirement[format!("{key}_unresolved")].as_u64().unwrap();
            assert_eq!(resolved + unresolved, total, "{requirement}");
            assert!(
                requirement[key].as_array().unwrap().len() as u64 <= total.min(4),
                "the embedded traceability keeps a bounded reference sample"
            );
        }
        if let Some(message) = requirement["verification"]
            .as_array()
            .unwrap()
            .first()
            .and_then(|reference| reference["message"].as_str())
        {
            assert!(message.chars().count() <= 160);
        }
    }
    // The surrounding context the caller asked for is untouched.
    assert!(summary["symbols"].is_array());
    assert!(!summary["design_items"].as_array().unwrap().is_empty());
    assert_eq!(summary["query"], "workspace isolation");

    let response = call_tool(
        &state,
        json!({"name": "software_context", "arguments": {"query": "workspace isolation", "detail": "full"}}),
    )
    .await
    .unwrap();
    assert_eq!(response["isError"], false, "{response}");
    let full = &response["structuredContent"];
    assert!(full["coverage"].get("detail").is_none());
    for requirement in full["coverage"]["requirements"].as_array().unwrap() {
        assert!(requirement.get("verification_total").is_none());
    }

    let rejected = call_tool(
        &state,
        json!({"name": "software_context", "arguments": {"query": "workspace isolation", "detail": "everything"}}),
    )
    .await;
    assert!(rejected.is_err(), "unknown detail must fail closed");
}

#[tokio::test]
async fn risk_status_summary_keeps_the_verdict_and_compacts_traceability() {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "-q"]);
    design_fixture(root.path());
    git(root.path(), &["add", "."]);
    git(
        root.path(),
        &[
            "-c",
            "user.name=fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgSign=false",
            "commit",
            "-qm",
            "baseline",
        ],
    );
    let state = exec_state(root.path());

    let response = call_tool(&state, json!({"name": "risk_status", "arguments": {}}))
        .await
        .unwrap();
    assert_eq!(response["isError"], false, "{response}");
    let summary = &response["structuredContent"];
    for key in ["level", "risks", "drift", "bug_patterns", "profile"] {
        assert!(
            summary.get(key).is_some(),
            "risk verdict field {key} must survive"
        );
    }
    let traceability = &summary["traceability"];
    assert_eq!(traceability["detail"], "summary");
    assert!(!traceability["requirements"].as_array().unwrap().is_empty());

    let response = call_tool(
        &state,
        json!({"name": "risk_status", "arguments": {"detail": "full"}}),
    )
    .await
    .unwrap();
    assert_eq!(response["isError"], false, "{response}");
    let full = &response["structuredContent"];
    assert!(full["traceability"].get("detail").is_none());
    for requirement in full["traceability"]["requirements"].as_array().unwrap() {
        assert!(requirement.get("verification_total").is_none());
    }
}
