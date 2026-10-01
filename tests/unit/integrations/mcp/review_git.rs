use super::*;
use serde_json::json;
use std::process::Command;

#[tokio::test]
async fn review_output_large_worktree_avoids_repeating_all_file_details() {
    let (root, app, _base) = fixture();
    for index in 0..128 {
        std::fs::write(
            root.path().join(format!("change-{index:03}.rs")),
            "// change\n",
        )
        .unwrap();
    }
    let summary = call_tool(&app, json!({"name":"review_changes","arguments":{}}))
        .await
        .unwrap();
    let full = call_tool(
        &app,
        json!({"name":"review_changes","arguments":{"detail":"full"}}),
    )
    .await
    .unwrap();
    assert_eq!(summary["isError"], false);
    assert_eq!(full["isError"], false);
    let summary = &summary["structuredContent"];
    let full = &full["structuredContent"];
    assert_eq!(summary["files"].as_array().unwrap().len(), 32);
    assert_eq!(full["files"].as_array().unwrap().len(), 128);
    assert_eq!(summary["file_details"]["omitted"], 96);
    assert_eq!(summary["file_details"]["complete"], false);
    assert_eq!(
        summary["file_details"]["retrieve"]["arguments"]["detail"],
        "full"
    );
    assert_eq!(
        summary["file_details"]["retrieve"]["arguments"]["workspace"],
        app.workspaces.default_id()
    );
    for key in [
        "clean",
        "files_changed",
        "risk_level",
        "recommended_checks",
        "recommended_verification",
        "findings",
        "truncated",
        "additions",
        "deletions",
        "source_changed",
        "tests_changed",
        "docs_only",
    ] {
        assert_eq!(summary[key], full[key], "compaction altered {key}");
    }
    assert_eq!(
        summary["truncated"], false,
        "file preview is not incomplete source discovery"
    );
    assert_eq!(
        summary["files"],
        json!(&full["files"].as_array().unwrap()[..32])
    );
    let summary_bytes = serde_json::to_vec(summary).unwrap().len();
    let full_bytes = serde_json::to_vec(full).unwrap().len();
    println!("review_output_bytes: summary={summary_bytes}, full={full_bytes}");
    assert!(summary_bytes * 2 < full_bytes);
}

#[tokio::test]
async fn review_output_rejects_invalid_detail_before_inspection() {
    let root = tempfile::tempdir().unwrap();
    let app = state(root.path());
    for detail in [
        json!(null),
        json!(true),
        json!(7),
        json!([]),
        json!({}),
        json!("all"),
    ] {
        let result = call_tool(
            &app,
            json!({"name":"review_changes","arguments":{"detail":detail}}),
        )
        .await;
        assert!(result.is_err(), "invalid detail was ignored: {result:?}");
    }
    assert!(app.workspaces.authorization_requests(10).is_empty());
}

fn git(root: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}

fn commit(root: &std::path::Path, message: &str) {
    git(
        root,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgSign=false",
            "commit",
            "--quiet",
            "--no-verify",
            "--allow-empty",
            "-m",
            message,
        ],
    );
}

fn state(root: &std::path::Path) -> AppState {
    let workspaces = Workspaces::new([root], false, true).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".into())),
        workspaces,
        harness: ToolHarness::new(4).unwrap(),
        monitor: TaskMonitor::new([workspace_id]),
        tasks: TaskRuntime::default(),
    }
}

fn fixture() -> (tempfile::TempDir, AppState, String) {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "--quiet"]);
    std::fs::write(root.path().join("README.md"), "# Fixture\n").unwrap();
    std::fs::write(root.path().join("main.rs"), "fn main() {}\n").unwrap();
    git(root.path(), &["add", "."]);
    commit(root.path(), "base");
    let base = git(root.path(), &["rev-parse", "HEAD"]);
    let app = state(root.path());
    (root, app, base)
}

#[tokio::test]
async fn review_git_base_inspection_reports_clean_head_rename_and_modes() {
    let (root, app, base) = fixture();
    git(root.path(), &["mv", "main.rs", "renamed 源.rs"]);
    git(root.path(), &["config", "core.fileMode", "false"]);
    git(root.path(), &["update-index", "--chmod=+x", "README.md"]);
    commit(root.path(), "rename and executable mode");
    let head = git(root.path(), &["rev-parse", "HEAD"]);
    for arguments in [
        json!({"base_revision": base}),
        json!({"base_revision": base, "target_revision": "HEAD"}),
        json!({"base_revision": base, "target_revision": head}),
    ] {
        let response = call_tool(
            &app,
            json!({"name":"review_changes", "arguments":arguments}),
        )
        .await
        .unwrap();
        assert_eq!(response["isError"], false, "{response}");
        let body = &response["structuredContent"];
        assert_eq!(body["workspace"], app.workspaces.default_id());
        assert_eq!(body["inspection_scope"], "base_change");
        let change = &body["git_change"];
        assert_eq!(change["complete"], true);
        assert_eq!(change["base_sha"], base);
        assert_eq!(change["target_sha"], head);
        assert_eq!(change["authority"], "metadata_only");
        assert_eq!(change["executable_changes"], true);
        assert_eq!(change["regular_file_changes"], true);
        assert!(change["unknown_reasons"].as_array().unwrap().is_empty());
        let rows = change["changes"].as_array().unwrap();
        let renamed = rows
            .iter()
            .find(|row| row["status"].as_str().unwrap().starts_with('R'))
            .unwrap();
        assert_eq!(renamed["old_path"], "main.rs");
        assert_eq!(renamed["new_path"], "renamed 源.rs");
        assert_eq!(renamed["old_mode"], "100644");
        assert_eq!(renamed["new_mode"], "100644");
        assert!(rows.iter().any(|row| row["new_path"] == "README.md"
            && row["old_mode"] == "100644"
            && row["new_mode"] == "100755"));
        assert!(change.get("trusted").is_none());
        assert!(body.get("ready").is_none());
    }
}

#[tokio::test]
async fn review_git_dirty_commit_is_error_and_worktree_target_is_explicit() {
    let (root, app, base) = fixture();
    std::fs::write(root.path().join("README.md"), "# Dirty\n").unwrap();
    let dirty = call_tool(
        &app,
        json!({"name":"review_changes",
        "arguments":{"base_revision":base}}),
    )
    .await
    .unwrap();
    assert_eq!(dirty["isError"], true);
    let change = &dirty["structuredContent"]["git_change"];
    assert_eq!(change["complete"], false);
    assert_eq!(change["binding"]["dirty"], true);
    assert!(change["unknown_reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reason| reason == "dirty_commit_candidate"));
    let working = call_tool(
        &app,
        json!({"name":"review_changes",
        "arguments":{"base_revision":base,"target_revision":"worktree"}}),
    )
    .await
    .unwrap();
    assert_eq!(working["isError"], false, "{working}");
    let change = &working["structuredContent"]["git_change"];
    assert_eq!(change["complete"], true);
    assert_eq!(change["target"]["kind"], "worktree");
    assert_eq!(change["binding"]["dirty"], true);
    assert!(change["changes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["new_path"] == "README.md"));
}

#[tokio::test]
async fn review_git_non_git_unborn_and_nonhead_remain_incomplete() {
    let root = tempfile::tempdir().unwrap();
    let app = state(root.path());
    let non_git = call_tool(
        &app,
        json!({"name":"review_changes",
        "arguments":{"base_revision":"HEAD"}}),
    )
    .await
    .unwrap();
    assert_eq!(non_git["isError"], true);
    assert_eq!(
        non_git["structuredContent"]["git_change"]["complete"],
        false
    );
    assert!(non_git["structuredContent"]["git_change"]["binding"].is_null());
    assert_eq!(
        non_git["structuredContent"]["git_change"]["unknown_reasons"],
        json!(["not_git"])
    );
    git(root.path(), &["init", "--quiet"]);
    let unborn = call_tool(
        &app,
        json!({"name":"review_changes",
        "arguments":{"base_revision":"HEAD"}}),
    )
    .await
    .unwrap();
    assert_eq!(unborn["isError"], true);
    assert_eq!(unborn["structuredContent"]["git_change"]["complete"], false);
    assert!(
        !unborn["structuredContent"]["git_change"]["unknown_reasons"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let (root, app, base) = fixture();
    commit(root.path(), "same bytes new head");
    let nonhead = call_tool(
        &app,
        json!({"name":"review_changes",
        "arguments":{"base_revision":base,"target_revision":base}}),
    )
    .await
    .unwrap();
    assert_eq!(nonhead["isError"], true);
    assert_eq!(
        nonhead["structuredContent"]["git_change"]["complete"],
        false
    );
    assert!(
        nonhead["structuredContent"]["git_change"]["unknown_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "target_not_current_head")
    );
}

#[tokio::test]
async fn review_git_rejects_invalid_revision_inputs_and_adversarial_scope() {
    let (_root, app, base) = fixture();
    let invalid = [
        json!({"target_revision":"HEAD"}),
        json!({"target_revision":"worktree"}),
        json!({"target_revision":null}),
        json!({"base_revision":null}),
        json!({"base_revision":false}),
        json!({"base_revision":7}),
        json!({"base_revision":[]}),
        json!({"base_revision":{}}),
        json!({"base_revision":""}),
        json!({"base_revision":"abc123"}),
        json!({"base_revision":"HEAD~1"}),
        json!({"base_revision":"HEAD^{commit}"}),
        json!({"base_revision":"--all"}),
        json!({"base_revision":"HEAD\n--output=/tmp/review-git-injection"}),
        json!({"base_revision":"refs/heads/main"}),
        json!({"base_revision":"worktree"}),
        json!({"base_revision":base,"target_revision":null}),
        json!({"base_revision":base,"target_revision":7}),
        json!({"base_revision":base,"target_revision":false}),
        json!({"base_revision":base,"target_revision":[]}),
        json!({"base_revision":base,"target_revision":{}}),
        json!({"base_revision":base,"target_revision":""}),
        json!({"base_revision":base,"target_revision":"abc123"}),
        json!({"base_revision":base,"target_revision":"HEAD~1"}),
        json!({"base_revision":base,"target_revision":"--all"}),
        json!({"base_revision":base,"target_revision":"HEAD\n--output=/tmp/review-git-injection"}),
        json!({"base_revision":base,"adversarial":true}),
    ];
    for arguments in invalid {
        let response = call_tool(
            &app,
            json!({"name":"review_changes","arguments":arguments.clone()}),
        )
        .await;
        assert!(
            response.is_err(),
            "invalid input {arguments} must be rejected: {response:?}"
        );
    }
    assert!(app.workspaces.authorization_requests(10).is_empty());
}

#[tokio::test]
async fn review_git_default_review_keeps_legacy_worktree_report_without_git_metadata() {
    let (root, app, _base) = fixture();
    std::fs::write(root.path().join("README.md"), "# Committed change\n").unwrap();
    git(root.path(), &["add", "README.md"]);
    commit(root.path(), "already committed");
    let response = call_tool(&app, json!({"name":"review_changes","arguments":{}}))
        .await
        .unwrap();
    let report = &response["structuredContent"];
    assert_eq!(report["workspace"], app.workspaces.default_id());
    assert_eq!(report["clean"], true);
    assert_eq!(report["files_changed"], 0);
    assert!(report["files"].as_array().unwrap().is_empty());
    assert!(report.get("git_change").is_none());
    assert!(report.get("inspection_scope").is_none());
    std::fs::write(root.path().join("README.md"), "# Actual worktree change\n").unwrap();
    let response = call_tool(&app, json!({"name":"review_changes","arguments":{}}))
        .await
        .unwrap();
    let report = &response["structuredContent"];
    assert_eq!(report["clean"], false);
    assert_eq!(report["files_changed"], 1);
    assert!(report.get("git_change").is_none());
    assert!(report.get("inspection_scope").is_none());
}
