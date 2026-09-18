use super::*;
use serde_json::json;

#[path = "admission.rs"]
mod admission;
#[path = "preview.rs"]
mod preview;

fn batch_test_state(root: &std::path::Path) -> AppState {
    let workspaces = Workspaces::new([root], true, false).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
        workspaces,
        harness: ToolHarness::new(4).unwrap(),
        monitor: TaskMonitor::new([workspace_id]),
        tasks: TaskRuntime::default(),
    }
}

#[tokio::test]
async fn strict_command_rejects_invalid_argument_types() {
    let root = tempfile::tempdir().unwrap();
    let state = AppState {
        workspaces: Workspaces::new([root.path()], true, true).unwrap(),
        ..batch_test_state(root.path())
    };
    for args in [
        json!(["--version", null]),
        json!(["--version", 7]),
        json!(["--version", false]),
        json!(["--version", {}]),
        json!(["--version", []]),
        Value::Null,
        json!("--version"),
        json!({}),
    ] {
        let result = call_tool(
            &state,
            json!({"name": "run_command", "arguments": {"program": "git", "args": args}}),
        )
        .await;
        assert!(
            result.is_err(),
            "invalid args must be rejected before execution: {result:?}"
        );
        assert!(state.workspaces.authorization_requests(10).is_empty());
    }
}

#[test]
fn strict_workspace_selector_rejects_invalid_explicit_values() {
    let root = tempfile::tempdir().unwrap();
    let state = batch_test_state(root.path());
    for value in [Value::Null, json!(false), json!(7), json!([]), json!({})] {
        assert!(selected_workspace(&state, &json!({"workspace": value})).is_err());
    }
    for value in [Value::Null, json!([]), json!("workspace")] {
        assert!(selected_workspace(&state, &value).is_err());
    }
    assert_eq!(
        selected_workspace(&state, &json!({})).unwrap().0,
        state.workspaces.default_id()
    );
    assert!(selected_workspace(&state, &json!({"workspace": ""})).is_err());
}

#[test]
fn strict_command_arguments_preserve_omission_and_empty_strings() {
    assert!(leaf_workspace::command_arguments(&json!({}))
        .unwrap()
        .is_empty());
    assert!(leaf_workspace::command_arguments(&json!({"args": []}))
        .unwrap()
        .is_empty());
    let expected = vec!["", "two words", "中文", "--", " spaced "];
    assert_eq!(
        leaf_workspace::command_arguments(&json!({"args": expected})).unwrap(),
        expected
    );
}

#[tokio::test]
async fn strict_command_rejects_invalid_optional_settings() {
    let root = tempfile::tempdir().unwrap();
    let state = AppState {
        workspaces: Workspaces::new([root.path()], true, true).unwrap(),
        ..batch_test_state(root.path())
    };
    for (key, value) in [
        ("cwd", Value::Null),
        ("cwd", json!(7)),
        ("cwd", json!("")),
        ("timeout_seconds", Value::Null),
        ("timeout_seconds", json!("120")),
        ("timeout_seconds", json!(0)),
        ("timeout_seconds", json!(-1)),
        ("timeout_seconds", json!(1.5)),
        ("timeout_seconds", json!(1801)),
    ] {
        let mut arguments = json!({"program": "git", "args": ["--version"]});
        arguments[key] = value;
        assert!(call_tool(
            &state,
            json!({"name": "run_command", "arguments": arguments})
        )
        .await
        .is_err());
    }
    let accepted = leaf_workspace::call(
        &state,
        "run_command",
        &json!({"program": "git", "args": ["--version"], "timeout_seconds": 1800}),
    )
    .await;
    assert!(
        accepted.is_ok(),
        "1800-second command timeout must be accepted"
    );
    assert!(state.workspaces.authorization_requests(10).is_empty());
}

#[tokio::test]
async fn strict_tool_envelopes_and_workspace_types_never_write() {
    let root = tempfile::tempdir().unwrap();
    let state = batch_test_state(root.path());
    for value in [Value::Null, json!([]), json!(7)] {
        assert!(call_tool(
            &state,
            json!({"name": "workspace_info", "arguments": value})
        )
        .await
        .is_err());
    }
    for value in [Value::Null, json!([]), json!(7), json!(false)] {
        let arguments = json!({"workspace": value, "path": "never.txt", "content": "bad"});
        assert!(call_tool(
            &state,
            json!({"name": "create_file", "arguments": arguments})
        )
        .await
        .is_err());
        let arguments = json!({"workspace": value, "tasks": [
            {"tool": "create_file", "arguments": {"path": "never.txt", "content": "bad"}},
            {"tool": "path_info", "arguments": {"path": "."}}
        ]});
        assert!(call_tool(
            &state,
            json!({"name": "parallel_tools", "arguments": arguments})
        )
        .await
        .is_err());
    }
    assert!(!root.path().join("never.txt").exists());
}

#[tokio::test]
async fn strict_fanout_workspace_validation_precedes_coalescing() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("source.txt"), "first second").unwrap();
    let state = batch_test_state(root.path());
    let (_, workspace) = state.workspaces.select(None).unwrap();
    let sha = workspace.read_file("source.txt", 1, None).unwrap().sha256;
    for value in [Value::Null, json!(7), json!({})] {
        let result = call_tool(&state, json!({"name": "parallel_tools", "arguments": {
            "tasks": [
                {"tool": "create_file", "arguments": {"path": "never.txt", "content": "bad"}},
                {"tool": "apply_edits", "arguments": {"path": "source.txt", "expected_sha256": sha,
                    "edits": [{"old_text": "first", "new_text": "changed"}]}},
                {"tool": "apply_edits", "arguments": {"workspace": value, "path": "source.txt", "expected_sha256": sha,
                    "edits": [{"old_text": "second", "new_text": "changed"}]}}
            ]
        }})).await;
        assert!(result.unwrap_err().contains("no tasks executed"));
        assert_eq!(
            std::fs::read_to_string(root.path().join("source.txt")).unwrap(),
            "first second"
        );
        assert!(!root.path().join("never.txt").exists());
    }
}

#[tokio::test]
async fn invalid_fanout_preflight_never_executes_any_mutation() {
    let invalid_tasks = [
        Value::Null,
        json!({"tool": "run_command", "arguments": {"program": "git"}}),
        json!({"tool": "create_file", "arguments": []}),
        json!({"tool": "create_files", "arguments": {"files": [
            {"path": "exists.txt", "content": "replacement"},
            {"content": "missing path"}
        ]}}),
        json!({"tool": "move_path", "arguments": {"source": "exists.txt"}}),
        json!({"tool": "read_files", "arguments": {"paths": ["exists.txt", 7]}}),
        json!({"tool": "create_file", "arguments": {
            "workspace": "absent-workspace", "path": "exists.txt", "content": "bad"
        }}),
        json!({"tool": "create_file", "arguments": {
            "path": "../outside.txt", "content": "bad"
        }}),
    ];
    for cap in [1, 4] {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("exists.txt"), "original").unwrap();
        let mut state = batch_test_state(root.path());
        state.harness = ToolHarness::new(cap).unwrap();
        for invalid in &invalid_tasks {
            let result = call_tool(
                &state,
                json!({
                    "name": "parallel_tools", "arguments": {"tasks": [
                        {"tool": "create_file", "arguments": {
                            "path": "independent.txt", "content": "must not run"
                        }},
                        invalid,
                        {"tool": "move_path", "arguments": {
                            "source": "exists.txt", "destination": "moved.txt"
                        }}
                    ]}
                }),
            )
            .await;
            let error = result.unwrap_err();
            assert!(error.contains("no tasks executed"), "{error}");
            assert_eq!(
                std::fs::read_to_string(root.path().join("exists.txt")).unwrap(),
                "original"
            );
            assert!(!root.path().join("moved.txt").exists());
            assert!(!root.path().join("independent.txt").exists());
        }
    }
}

#[tokio::test]
async fn command_and_verification_failures_keep_reports_and_error_flags() {
    let root = tempfile::tempdir().unwrap();
    let note = root.path().join("note.txt");
    std::fs::write(&note, "clean\n").unwrap();
    for args in [vec!["init", "--quiet"], vec!["add", "note.txt"]] {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(root.path())
            .output()
            .unwrap();
        assert!(output.status.success(), "{:?}", output);
    }
    let state = AppState {
        workspaces: Workspaces::new([root.path()], true, true).unwrap(),
        ..batch_test_state(root.path())
    };
    for failed in [false, true] {
        if failed {
            std::fs::write(&note, "trailing whitespace \n").unwrap();
        }
        let command = call_tool(
            &state,
            json!({
                "name": "run_command", "arguments": {
                    "program": "git", "args": ["diff", "--check"]
                }
            }),
        )
        .await
        .unwrap();
        assert_eq!(command["isError"], failed);
        assert_eq!(command["structuredContent"]["success"], !failed);
        assert!(command["structuredContent"]["exit_code"].is_number());
        let report = call_tool(
            &state,
            json!({
                "name": "verify_project", "arguments": {"level": "quick"}
            }),
        )
        .await
        .unwrap();
        assert_eq!(report["isError"], failed);
        assert_eq!(report["structuredContent"]["passed"], !failed);
        assert_eq!(report["structuredContent"]["checks_run"], 1);
        assert_eq!(
            report["structuredContent"]["checks_failed"],
            usize::from(failed)
        );
        assert_eq!(report["structuredContent"]["checks"][0]["success"], !failed);
    }
}

#[tokio::test]
async fn verification_without_inferred_checks_is_not_success() {
    let root = tempfile::tempdir().unwrap();
    let state = AppState {
        workspaces: Workspaces::new([root.path()], true, true).unwrap(),
        ..batch_test_state(root.path())
    };
    let report = call_tool(
        &state,
        json!({
            "name": "verify_project", "arguments": {"level": "quick"}
        }),
    )
    .await
    .unwrap();
    assert_eq!(report["isError"], true);
    assert_eq!(report["structuredContent"]["passed"], false);
    assert_eq!(report["structuredContent"]["checks_run"], 0);
}

#[test]
fn batch_outcomes_preserve_partial_results_for_every_bulk_primitive() {
    for (name, key) in [
        ("read_files", "files"),
        ("create_files", "results"),
        ("apply_file_edits", "results"),
        ("move_paths", "results"),
    ] {
        let mut partial = json!({(key): [{"ok": true}, {"ok": false, "error": "fixture"}]});
        assert!(!leaf_workspace::batch_succeeded(name, &mut partial));
        assert_eq!(partial["status"], "partial");
        assert_eq!(partial["succeeded"], 1);
        assert_eq!(partial["failed"], 1);
        assert_eq!(partial[key][0]["ok"], true);
        let mut failed = json!({(key): [{"ok": false}]});
        assert!(!leaf_workspace::batch_succeeded(name, &mut failed));
        assert_eq!(failed["status"], "failed");
        let mut complete = json!({(key): [{"ok": true}]});
        assert!(leaf_workspace::batch_succeeded(name, &mut complete));
        assert_eq!(complete["status"], "complete");
    }
}

#[tokio::test]
async fn partial_bulk_read_is_an_error_with_usable_successes() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("exists.txt"), "original").unwrap();
    let state = batch_test_state(root.path());
    let response = call_tool(
        &state,
        json!({
            "name": "read_files", "arguments": {"paths": ["exists.txt", "missing.txt"]}
        }),
    )
    .await
    .unwrap();
    assert_eq!(response["isError"], true);
    let result = &response["structuredContent"];
    assert_eq!(result["status"], "partial");
    assert_eq!(result["files"][0]["file"]["content"], "original");
    assert_eq!(result["files"][1]["ok"], false);
}

#[tokio::test]
async fn partial_bulk_write_blocks_successors_but_preserves_independent_work() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("exists.txt"), "original").unwrap();
    let state = batch_test_state(root.path());
    let response = call_tool(&state, json!({
        "name":"parallel_tools", "arguments":{"tasks":[
            {"tool":"create_files", "arguments":{"files":[
                {"path":"exists.txt", "content":"replacement"},
                {"path":"created.txt", "content":"created"}
            ]}},
            {"tool":"move_path", "arguments":{"source":"exists.txt", "destination":"moved.txt"}},
            {"tool":"read_file", "arguments":{"path":"moved.txt"}},
            {"tool":"create_file", "arguments":{"path":"independent.txt", "content":"independent"}}
        ]}
    })).await.unwrap();
    assert_eq!(response["isError"], true);
    let result = &response["structuredContent"];
    assert_eq!(result["failed"], 3);
    assert_eq!(result["succeeded"], 1);
    assert_eq!(result["items"][0]["result"]["status"], "partial");
    for index in [1, 2] {
        assert!(result["items"][index]["error"]
            .as_str()
            .unwrap()
            .contains("dependency failed"));
    }
    assert_eq!(
        std::fs::read_to_string(root.path().join("exists.txt")).unwrap(),
        "original"
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("created.txt")).unwrap(),
        "created"
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("independent.txt")).unwrap(),
        "independent"
    );
    assert!(!root.path().join("moved.txt").exists());
}

#[tokio::test]
async fn review_changes_adversarial_mode_attaches_non_evidence_questions() {
    let root = tempfile::tempdir().unwrap();
    let initialized = std::process::Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(root.path())
        .status()
        .unwrap();
    assert!(initialized.success());
    std::fs::create_dir_all(root.path().join("src")).unwrap();
    std::fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname='adversarial-fixture'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("src/lib.rs"),
        "pub fn target() -> bool { true }\n",
    )
    .unwrap();
    let committed = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=wcode-test",
            "-c",
            "user.email=wcode@example.invalid",
            "add",
            ".",
        ])
        .current_dir(root.path())
        .status()
        .unwrap();
    assert!(committed.success());
    let committed = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=wcode-test",
            "-c",
            "user.email=wcode@example.invalid",
            "-c",
            "commit.gpgSign=false",
            "commit",
            "--quiet",
            "-m",
            "fixture",
        ])
        .current_dir(root.path())
        .status()
        .unwrap();
    assert!(committed.success());
    std::fs::write(
        root.path().join("src/lib.rs"),
        "pub fn target() -> bool { false }\n",
    )
    .unwrap();
    let workspaces = Workspaces::new([root.path()], true, true).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    let state = AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
        workspaces,
        harness: ToolHarness::new(4).unwrap(),
        monitor: TaskMonitor::new([workspace_id]),
        tasks: TaskRuntime::default(),
    };

    let plain = call_tool(&state, json!({"name":"review_changes","arguments":{}}))
        .await
        .unwrap();
    assert_eq!(plain["isError"], false);
    assert!(plain["structuredContent"].get("adversarial").is_none());

    let challenged = call_tool(
        &state,
        json!({"name":"review_changes","arguments":{"adversarial":true}}),
    )
    .await
    .unwrap();
    assert_eq!(challenged["isError"], false);
    let packet = &challenged["structuredContent"]["adversarial"];
    assert_eq!(packet["policy"], "challenge-packet-not-evidence");
    assert_eq!(packet["reviewer_role"], "adversarial");
    let candidates = &packet["candidate_search"];
    assert_eq!(candidates["precision"], "syntax");
    assert_eq!(candidates["executed"], false);
    assert_eq!(candidates["oracle_required"], true);
    assert_eq!(candidates["files_scanned"], 1);
    assert_eq!(candidates["candidates"][0]["original"], "false");
    assert_eq!(candidates["candidates"][0]["replacement"], "true");
    assert_eq!(
        candidates["candidates"][0]["status"],
        "proposed-not-typechecked"
    );
    for invalid in [json!(null), json!("true"), json!(1), json!({}), json!([])] {
        let error = call_tool(
            &state,
            json!({"name":"review_changes","arguments":{"adversarial":invalid}}),
        )
        .await
        .unwrap_err();
        assert!(error.contains("adversarial must be a boolean"));
    }
    assert!(packet["questions"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
    assert!(packet["questions"]
        .as_array()
        .unwrap()
        .iter()
        .all(|question| {
            question["required_evidence"]
                .as_array()
                .is_some_and(|evidence| !evidence.is_empty())
                && question["counterexample_experiment"]["kind"]
                    .as_str()
                    .is_some_and(|kind| !kind.is_empty())
                && question["counterexample_experiment"]["closes_with"]
                    .as_array()
                    .is_some_and(|evidence| !evidence.is_empty())
        }));
}

#[cfg(test)]
mod agent_context_enrichment_tests {
    use super::*;

    #[test]
    fn worktree_status_warns_on_existing_changes_and_blocks_conflicts() {
        let mut context = json!({
            "targets": [{"path": "src/lib.rs"}],
            "readiness": {"edit": "ready", "next_actions": ["apply_edits", "review_changes", "verify_project"], "advisories": []}
        });
        merge_agent_worktree_status(
            &mut context,
            &json!({
                "available": true,
                "files": [{"path":"src/lib.rs","status":"modified","staged":false,"unstaged":true,"untracked":false}],
                "truncated": false
            }),
        );
        assert_eq!(context["worktree"]["targets"][0]["status"], "modified");
        assert_eq!(context["worktree"]["has_existing_changes"], true);
        assert!(context["readiness"]["advisories"]
            .as_array()
            .unwrap()
            .iter()
            .any(|advisory| advisory == "target_has_worktree_changes"));
        assert_eq!(context["readiness"]["edit"], "ready");

        merge_agent_worktree_status(
            &mut context,
            &json!({
                "available": true,
                "files": [{"path":"src/lib.rs","status":"unmerged","staged":true,"unstaged":true,"untracked":false}],
                "truncated": false
            }),
        );
        assert_eq!(context["readiness"]["edit"], "worktree_conflict");
        assert_eq!(
            context["readiness"]["next_actions"],
            json!(["review_changes"])
        );
    }
}

#[tokio::test]
async fn media_content_uses_standard_mcp_blocks_without_private_client_extension() {
    let root = tempfile::tempdir().unwrap();
    let png = STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=")
        .unwrap();
    std::fs::write(root.path().join("pixel.png"), png).unwrap();
    let state = batch_test_state(root.path());

    let result = call_tool(
        &state,
        json!({
            "name":"read_media",
            "arguments":{"path":"pixel.png","include_content":true}
        }),
    )
    .await
    .unwrap();

    assert_eq!(result["isError"], false);
    assert_eq!(result["structuredContent"]["content_returned"], true);
    assert_eq!(result["content"][1]["type"], "image");
    assert_eq!(result["content"][1]["mimeType"], "image/png");
    assert!(result["content"][1]["data"].as_str().unwrap().len() > 16);
}

#[tokio::test]
async fn media_protocol_matrix_is_standard_and_type_bounded() {
    let root = tempfile::tempdir().unwrap();
    let png = STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=")
        .unwrap();
    std::fs::write(root.path().join("pixel.png"), png).unwrap();
    std::fs::write(root.path().join("tone.mp3"), b"ID3test").unwrap();
    std::fs::write(
        root.path().join("clip.mp4"),
        [0, 0, 0, 12, b'f', b't', b'y', b'p', b'i', b's', b'o', b'm'],
    )
    .unwrap();
    let state = batch_test_state(root.path());

    for (path, expected_kind, expected_mime) in [
        ("pixel.png", "image", "image/png"),
        ("tone.mp3", "audio", "audio/mpeg"),
    ] {
        let metadata_only = call_tool(
            &state,
            json!({"name":"read_media","arguments":{"path":path}}),
        )
        .await
        .unwrap();
        assert_eq!(metadata_only["isError"], false, "{path}");
        assert_eq!(metadata_only["structuredContent"]["kind"], expected_kind);
        assert_eq!(
            metadata_only["structuredContent"]["mime_type"],
            expected_mime
        );
        assert_eq!(
            metadata_only["structuredContent"]["content_requested"],
            false
        );
        assert_eq!(
            metadata_only["structuredContent"]["content_returned"],
            false
        );
        assert!(metadata_only["content"].as_array().unwrap().len() == 1);

        let with_content = call_tool(
            &state,
            json!({
                "name":"read_media",
                "arguments":{"path":path,"include_content":true}
            }),
        )
        .await
        .unwrap();
        assert_eq!(with_content["isError"], false, "{path}");
        assert_eq!(with_content["structuredContent"]["content_requested"], true);
        assert_eq!(with_content["structuredContent"]["content_returned"], true);
        assert_eq!(with_content["content"][1]["type"], expected_kind);
        assert_eq!(with_content["content"][1]["mimeType"], expected_mime);
        assert!(!with_content["content"][1]["data"]
            .as_str()
            .unwrap()
            .is_empty());
    }

    let video_metadata = call_tool(
        &state,
        json!({"name":"read_media","arguments":{"path":"clip.mp4"}}),
    )
    .await
    .unwrap();
    assert_eq!(video_metadata["isError"], false);
    assert_eq!(video_metadata["structuredContent"]["kind"], "video");
    assert_eq!(
        video_metadata["structuredContent"]["content_available"],
        false
    );
    assert_eq!(
        video_metadata["structuredContent"]["content_returned"],
        false
    );

    let video_content = call_tool(
        &state,
        json!({
            "name":"read_media",
            "arguments":{"path":"clip.mp4","include_content":true}
        }),
    )
    .await
    .unwrap();
    assert_eq!(video_content["isError"], true);
    assert_eq!(
        video_content["structuredContent"]["error_code"],
        "media_content_type_not_supported"
    );
    assert_eq!(
        video_content["structuredContent"]["content_requested"],
        true
    );
    assert_eq!(
        video_content["structuredContent"]["content_returned"],
        false
    );
}
