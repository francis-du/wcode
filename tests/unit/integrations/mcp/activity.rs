use super::*;

#[tokio::test]
async fn observatory_activity_is_protected_scoped_bounded_and_has_no_arguments() {
    let (state, root) = origin_test_state();
    let workspace = state.workspaces.default_id().to_owned();
    let mut missing_token = ui_headers(&state, &workspace);
    missing_token.remove("x-wcode-ui-token");
    assert_eq!(
        intelligence_web_activity(State(state.clone()), missing_token)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let mut custom_host = ui_headers(&state, &workspace);
    custom_host.insert("host", "custom.example".parse().unwrap());
    assert_eq!(
        intelligence_web_activity(State(state.clone()), custom_host)
            .await
            .status(),
        StatusCode::OK,
        "a syntactically valid reverse-proxy Host stays usable when Origin and UI authorization remain valid"
    );

    let mut untrusted_origin = ui_headers(&state, &workspace);
    untrusted_origin.insert("origin", "https://untrusted.example".parse().unwrap());
    assert_eq!(
        intelligence_web_activity(State(state.clone()), untrusted_origin)
            .await
            .status(),
        StatusCode::FORBIDDEN
    );

    let mut duplicate_host = ui_headers(&state, &workspace);
    duplicate_host.append("host", "second.example".parse().unwrap());
    assert_eq!(
        intelligence_web_activity(State(state.clone()), duplicate_host)
            .await
            .status(),
        StatusCode::FORBIDDEN,
        "ambiguous Host headers must still fail closed"
    );
    for _ in 0..16 {
        state
            .monitor
            .queue(&workspace, "old_read", "PRIVATE-ARGUMENT", 1)
            .finish(true, 1);
    }
    let running = state
        .monitor
        .queue(&workspace, "run_command", "PRIVATE-ARGUMENT", 1);
    let job_id = format!("TASK-{}-{}", "1".repeat(20), "a".repeat(32));
    running.bind_command_job(&job_id);
    running.start();
    let queued = state
        .monitor
        .queue(&workspace, "find_symbol", "PRIVATE-ARGUMENT", 1);
    let foreign = state
        .monitor
        .queue("unrelated-project", "FOREIGN-TOOL", "PRIVATE-ARGUMENT", 1);
    let foreign_job_id = format!("TASK-{}-{}", "2".repeat(20), "b".repeat(32));
    foreign.bind_command_job(&foreign_job_id);
    foreign.start();
    let before = state.monitor.connection_status();
    let response =
        intelligence_web_activity(State(state.clone()), ui_headers(&state, &workspace)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let body = response_json(response).await;
    assert_eq!(body["workspace"], workspace);
    assert_eq!(body["resource_scope"], "whole_process");
    assert_eq!(body["activity"]["active"], 1);
    assert_eq!(body["activity"]["queued"], 1);
    assert_eq!(body["activity"]["recent"].as_array().unwrap().len(), 12);
    assert_eq!(body["activity"]["recent_truncated"], true);
    assert_eq!(body["activity"]["recent"][0]["status"], "running");
    assert_eq!(body["activity"]["recent"][0]["command_job_id"], job_id);
    assert_eq!(body["activity"]["recent"][1]["status"], "queued");
    assert!(body["activity"]["recent"][1]["command_job_id"].is_null());
    assert!(!body.to_string().contains("PRIVATE-ARGUMENT"));
    assert!(!body.to_string().contains("FOREIGN-TOOL"));
    assert!(!body.to_string().contains(&foreign_job_id));
    assert!(!body.to_string().contains(state.auth.ui_token()));
    assert_eq!(
        before.active_tasks,
        state.monitor.connection_status().active_tasks
    );
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    drop((running, queued, foreign));
}

#[tokio::test]
async fn observatory_activity_exposes_latest_jev_decision_without_secrets() {
    let (state, _root) = origin_test_state();
    let workspace = state.workspaces.default_id().to_owned();
    state.monitor.record_agent_context_decision(
        &workspace,
        &json!({
            "provider": "jev",
            "status": "active",
            "model": "jev-latest",
            "authority": "increase_only_assist",
            "question_set": {"id": "wcode.agent_context", "version": 3},
            "baseline_next_action": "edit_then_verify",
            "candidate_next_action": "edit_then_verify",
            "guidance": ["jev:next_action:edit_then_verify"],
            "comparison": {
                "shared_signals": 7,
                "choice_disagreements": 1,
                "safety_policy_violations": 0,
                "shape_mismatches": 0
            },
            "api_key": "PRIVATE-API-KEY",
            "state": "PRIVATE-RAW-CONTEXT"
        }),
    );

    let response =
        intelligence_web_activity(State(state.clone()), ui_headers(&state, &workspace)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    let jev = &body["activity"]["agent_context"]["decision_runtime"]["jev"];
    assert_eq!(jev["status"], "active");
    assert_eq!(jev["model"], "jev-latest");
    assert_eq!(jev["question_set"]["id"], "wcode.agent_context");
    assert_eq!(jev["baseline_next_action"], "edit_then_verify");
    assert_eq!(jev["candidate_next_action"], "edit_then_verify");
    assert_eq!(jev["calls"]["successful"], 1);
    assert_eq!(jev["comparison"]["safety_policy_violations"], 0);
    let serialized = body.to_string();
    assert!(!serialized.contains("PRIVATE-JEV-KEY"));
    assert!(!serialized.contains("PRIVATE-RAW-CONTEXT"));
    assert!(!serialized.contains(state.auth.ui_token()));
}
