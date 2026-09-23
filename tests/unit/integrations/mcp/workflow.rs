use super::*;
use std::{fs, path::Path, process::Command};

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

async fn read_sha(state: &AppState, path: &str) -> String {
    let read = call_tool(state, json!({"name":"read_file","arguments":{"path":path}}))
        .await
        .unwrap();
    assert_eq!(read["isError"], false);
    read["structuredContent"]["sha256"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn developer_workflow_fitness_reaches_verified_edit_and_recovers_from_failures() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::create_dir_all(root.path().join("tests")).unwrap();
    fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname = \"developer-workflow-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(
        root.path().join("src/lib.rs"),
        "pub fn answer() -> u32 {\n    1\n}\n",
    )
    .unwrap();
    fs::write(
        root.path().join("tests/answer.rs"),
        "use developer_workflow_fixture::answer;\n\n#[test]\nfn answer_is_two() {\n    assert_eq!(answer(), 2);\n}\n",
    )
    .unwrap();

    git(root.path(), &["init", "--quiet"]);
    git(root.path(), &["add", "."]);
    git(
        root.path(),
        &[
            "-c",
            "user.name=WCode",
            "-c",
            "user.email=wcode@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            "baseline",
        ],
    );

    let workspaces = Workspaces::new([root.path()], true, true).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    let state = AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
        workspaces,
        harness: ToolHarness::new(4).unwrap(),
        monitor: TaskMonitor::new([workspace_id]),
        tasks: TaskRuntime::default(),
    };

    let context = call_tool(
        &state,
        json!({
            "name":"agent_context",
            "arguments":{
                "query":"Fix the answer function so the library returns the value required by its regression test",
                "budget":1000
            }
        }),
    )
    .await
    .unwrap();
    assert_eq!(context["isError"], false);
    let pack = &context["structuredContent"];
    let source = pack["hot_source"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["path"] == "src/lib.rs")
        .unwrap_or_else(|| panic!("agent_context omitted the edit target: {pack}"));
    assert_eq!(source["body"]["truncated"], false);
    assert!(source["body"]["content"]
        .as_str()
        .is_some_and(|body| body.contains("pub fn answer()") && body.contains("    1")));
    let original_sha = source["sha256"].as_str().unwrap().to_owned();

    let edited = call_tool(
        &state,
        json!({
            "name":"apply_edits",
            "arguments":{
                "path":"src/lib.rs",
                "expected_sha256":original_sha,
                "edits":[{
                    "old_text":"    1",
                    "new_text":"    2",
                    "start_line":2,
                    "end_line":2
                }]
            }
        }),
    )
    .await
    .unwrap();
    assert_eq!(edited["isError"], false);

    let stale = call_tool(
        &state,
        json!({
            "name":"apply_edits",
            "arguments":{
                "path":"src/lib.rs",
                "expected_sha256":original_sha,
                "edits":[{
                    "old_text":"    2",
                    "new_text":"    3",
                    "start_line":2,
                    "end_line":2
                }]
            }
        }),
    )
    .await
    .unwrap();
    assert_eq!(
        stale["isError"], true,
        "stale SHA must never overwrite a newer edit"
    );

    let review = call_tool(
        &state,
        json!({"name":"review_changes","arguments":{"adversarial":true}}),
    )
    .await
    .unwrap();
    assert_eq!(review["isError"], false);
    assert_eq!(review["structuredContent"]["clean"], false);
    assert_eq!(review["structuredContent"]["source_changed"], true);
    assert!(review["structuredContent"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|file| file["path"] == "src/lib.rs"));
    assert_eq!(
        review["structuredContent"]["adversarial"]["policy"],
        "challenge-packet-not-evidence"
    );

    let drifted = call_tool(
        &state,
        json!({"name":"verify_project","arguments":{"level":"full","timeout_seconds":60}}),
    )
    .await
    .unwrap();
    assert_eq!(drifted["isError"], true, "{drifted}");
    assert_eq!(
        drifted["structuredContent"]["error_code"],
        "VERIFICATION_REVISION_DRIFT"
    );
    assert_eq!(drifted["structuredContent"]["retryable"], true);
    assert_eq!(
        drifted["structuredContent"]["next_actions"],
        json!(["review_changes", "verify_project"])
    );

    let drift_review = call_tool(&state, json!({"name":"review_changes","arguments":{}}))
        .await
        .unwrap();
    assert_eq!(drift_review["isError"], false);
    assert!(drift_review["structuredContent"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|file| file["path"] == "Cargo.lock"));

    let verified = call_tool(
        &state,
        json!({"name":"verify_project","arguments":{"level":"full","timeout_seconds":60}}),
    )
    .await
    .unwrap();
    assert_eq!(verified["isError"], false, "{verified}");
    assert_eq!(verified["structuredContent"]["passed"], true, "{verified}");
    assert!(verified["structuredContent"]["checks_run"]
        .as_u64()
        .is_some_and(|count| count >= 3));

    let current_sha = read_sha(&state, "src/lib.rs").await;
    let broken = call_tool(
        &state,
        json!({
            "name":"apply_edits",
            "arguments":{
                "path":"src/lib.rs",
                "expected_sha256":current_sha,
                "edits":[{
                    "old_text":"    2",
                    "new_text":"    this is not valid Rust",
                    "start_line":2,
                    "end_line":2
                }]
            }
        }),
    )
    .await
    .unwrap();
    assert_eq!(broken["isError"], false);

    let failed = call_tool(
        &state,
        json!({"name":"verify_project","arguments":{"level":"quick","timeout_seconds":60}}),
    )
    .await
    .unwrap();
    assert_eq!(failed["isError"], true);
    assert_eq!(failed["structuredContent"]["passed"], false);
    assert!(failed["structuredContent"]["checks_failed"]
        .as_u64()
        .is_some_and(|count| count > 0));
    assert!(failed["structuredContent"]["failure_locations"]
        .as_array()
        .is_some_and(|locations| locations.iter().any(|location| {
            location["path"] == "src/lib.rs"
                && location["line"].as_u64().is_some_and(|line| line >= 2)
                && location["column"]
                    .as_u64()
                    .is_some_and(|column| column >= 1)
                && location["precision"] == "diagnostic_text"
        })));
    let failure_context = failed["structuredContent"]["failure_context"]
        .as_array()
        .and_then(|contexts| {
            contexts
                .iter()
                .find(|context| context["path"] == "src/lib.rs")
        })
        .unwrap();
    assert_eq!(failure_context["editable"], true);
    assert_eq!(
        failure_context["precision"],
        "diagnostic_text+source_window"
    );
    assert!(failure_context["content"]
        .as_str()
        .is_some_and(|content| content.contains("this is not valid Rust")));
    let broken_sha = failure_context["sha256"].as_str().unwrap().to_owned();
    let repaired = call_tool(
        &state,
        json!({
            "name":"apply_edits",
            "arguments":{
                "path":"src/lib.rs",
                "expected_sha256":broken_sha,
                "edits":[{
                    "old_text":"    this is not valid Rust",
                    "new_text":"    2",
                    "start_line":2,
                    "end_line":2
                }]
            }
        }),
    )
    .await
    .unwrap();
    assert_eq!(repaired["isError"], false);

    let recovered = call_tool(
        &state,
        json!({"name":"verify_project","arguments":{"level":"quick","timeout_seconds":60}}),
    )
    .await
    .unwrap();
    assert_eq!(recovered["isError"], false, "{recovered}");
    assert_eq!(
        recovered["structuredContent"]["passed"], true,
        "{recovered}"
    );
}

#[tokio::test]
async fn developer_workflow_fitness_is_polyglot_for_go_deno_python_and_node() {
    struct Fixture {
        manifest_path: &'static str,
        manifest: &'static str,
        source_path: &'static str,
        source: &'static str,
        test_path: &'static str,
        test: &'static str,
        query: &'static str,
        old_text: &'static str,
        new_text: &'static str,
        start_line: u64,
        required_program: &'static str,
        expected_checks: &'static [&'static str],
    }

    let fixtures = [
        Fixture {
            manifest_path: "go.mod",
            manifest: "module developerworkflow\n\ngo 1.23.0\n",
            source_path: "answer.go",
            source: "package developerworkflow\n\nfunc Answer() int {\n    return 1\n}\n",
            test_path: "answer_test.go",
            test: "package developerworkflow\n\nimport \"testing\"\n\nfunc TestAnswer(t *testing.T) {\n    if Answer() != 2 {\n        t.Fatal(\"expected two\")\n    }\n}\n",
            query: "Fix the answer behavior so the Go package matches its regression test",
            old_text: "    return 1",
            new_text: "    return 2",
            start_line: 4,
            required_program: "go",
            expected_checks: &["go-vet", "go-tests"],
        },
        Fixture {
            manifest_path: "deno.json",
            manifest: "{ \"fmt\": { \"lineWidth\": 100 }, \"lint\": {} }\n",
            source_path: "answer.ts",
            source: "export function answer(): number {\n  return 1;\n}\n",
            test_path: "answer_test.ts",
            test: "import { answer } from \"./answer.ts\";\n\nDeno.test(\"answer\", () => {\n  if (answer() !== 2) {\n    throw new Error(\"expected two\");\n  }\n});\n",
            query: "Fix the answer behavior so the Deno module matches its regression test",
            old_text: "  return 1;",
            new_text: "  return 2;",
            start_line: 2,
            required_program: "deno",
            expected_checks: &["deno-format", "deno-lint", "deno-check", "deno-test"],
        },
        Fixture {
            manifest_path: "pyproject.toml",
            manifest: "[project]\nname='developerworkflow'\nversion='0.1.0'\n",
            source_path: "answer.py",
            source: "def answer():\n    return 1\n",
            test_path: "test_answer.py",
            test: "import unittest\nfrom answer import answer\n\nclass AnswerTest(unittest.TestCase):\n    def test_answer(self):\n        self.assertEqual(answer(), 2)\n",
            query: "Fix the answer behavior so the Python module matches its unittest regression test",
            old_text: "    return 1",
            new_text: "    return 2",
            start_line: 2,
            required_program: "python3",
            expected_checks: &["python-unittest"],
        },
        Fixture {
            manifest_path: "package.json",
            manifest: "{\"name\":\"developerworkflow\",\"version\":\"1.0.0\",\"type\":\"module\",\"scripts\":{\"test\":\"node --test\"}}\n",
            source_path: "answer.js",
            source: "export function answer() {\n  return 1;\n}\n",
            test_path: "answer.test.js",
            test: "import test from \"node:test\";\nimport assert from \"node:assert/strict\";\nimport { answer } from \"./answer.js\";\n\ntest(\"answer\", () => {\n  assert.equal(answer(), 2);\n});\n",
            query: "Fix the answer behavior so the Node module matches its regression test",
            old_text: "  return 1;",
            new_text: "  return 2;",
            start_line: 2,
            required_program: "npm",
            expected_checks: &["node-test"],
        },
    ];

    for fixture in fixtures {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join(fixture.manifest_path), fixture.manifest).unwrap();
        fs::write(root.path().join(fixture.source_path), fixture.source).unwrap();
        fs::write(root.path().join(fixture.test_path), fixture.test).unwrap();
        git(root.path(), &["init", "--quiet"]);
        git(root.path(), &["add", "."]);
        git(
            root.path(),
            &[
                "-c",
                "user.name=WCode",
                "-c",
                "user.email=wcode@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--quiet",
                "-m",
                "baseline",
            ],
        );

        let workspaces = Workspaces::new([root.path()], true, true).unwrap();
        let workspace_id = workspaces.default_id().to_owned();
        let state = AppState {
            auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
            workspaces,
            harness: ToolHarness::new(4).unwrap(),
            monitor: TaskMonitor::new([workspace_id]),
            tasks: TaskRuntime::default(),
        };

        let failed = call_tool(
            &state,
            json!({"name":"verify_project","arguments":{"level":"full","timeout_seconds":60}}),
        )
        .await
        .unwrap();
        assert_eq!(failed["isError"], true, "{failed}");
        let unavailable = format!(
            "verification executable `{}` is unavailable",
            fixture.required_program
        );
        let toolchain_unavailable = failed["structuredContent"]["checks"]
            .as_array()
            .is_some_and(|checks| {
                checks.iter().any(|check| {
                    check["stderr_tail"]
                        .as_str()
                        .is_some_and(|stderr| stderr.contains(&unavailable))
                })
            });
        if toolchain_unavailable {
            assert!(
                failed["structuredContent"]["checks"]
                    .as_array()
                    .is_some_and(|checks| checks.iter().any(|check| {
                        check["stderr_tail"].as_str().is_some_and(|stderr| {
                            stderr.contains("install the project toolchain")
                                && stderr.contains("retry verify_project")
                        })
                    })),
                "missing fail-closed toolchain recovery for {}: {failed}",
                fixture.required_program
            );
        } else {
            assert!(
                failed["structuredContent"]["failure_locations"]
                    .as_array()
                    .is_some_and(|locations| locations.iter().any(|location| {
                        location["path"] == fixture.test_path
                            && location["line"].as_u64().is_some_and(|line| line >= 1)
                            && location["precision"] == "diagnostic_text"
                    })),
                "missing runtime diagnostic location for {}: {failed}",
                fixture.test_path
            );
            assert!(
                failed["structuredContent"]["failure_context"]
                    .as_array()
                    .is_some_and(|contexts| contexts.iter().any(|context| {
                        context["path"] == fixture.test_path
                            && context["sha256"]
                                .as_str()
                                .is_some_and(|sha| sha.len() == 64)
                            && context["precision"] == "diagnostic_text+source_window"
                    })),
                "missing runtime diagnostic context for {}: {failed}",
                fixture.test_path
            );
        }
        if fixture.source_path == "answer.py" {
            assert!(
                !root.path().join("__pycache__").exists(),
                "verification must not mutate the Python workspace with bytecode caches"
            );
        }

        let context = call_tool(
            &state,
            json!({"name":"agent_context","arguments":{"query":fixture.query,"budget":1000}}),
        )
        .await
        .unwrap();
        assert_eq!(context["isError"], false, "{context}");
        let pack = &context["structuredContent"];
        let source = pack["hot_source"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["path"] == fixture.source_path)
            .unwrap_or_else(|| panic!("missing {} from context: {pack}", fixture.source_path));
        assert_eq!(source["body"]["truncated"], false);
        let source_sha = source["sha256"].as_str().unwrap();

        let edited = call_tool(
            &state,
            json!({
                "name":"apply_edits",
                "arguments":{
                    "path":fixture.source_path,
                    "expected_sha256":source_sha,
                    "edits":[{
                        "old_text":fixture.old_text,
                        "new_text":fixture.new_text,
                        "start_line":fixture.start_line,
                        "end_line":fixture.start_line
                    }]
                }
            }),
        )
        .await
        .unwrap();
        assert_eq!(edited["isError"], false, "{edited}");

        let review = call_tool(
            &state,
            json!({"name":"review_changes","arguments":{"adversarial":true}}),
        )
        .await
        .unwrap();
        assert_eq!(review["isError"], false, "{review}");
        assert!(review["structuredContent"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|file| file["path"] == fixture.source_path));

        let checks = pack["checks"].as_array().unwrap();
        for expected in fixture.expected_checks {
            assert!(
                checks.iter().any(|check| check["id"] == *expected),
                "missing {expected} for {}: {pack}",
                fixture.source_path
            );
        }

        let verified = call_tool(
            &state,
            json!({"name":"verify_project","arguments":{"level":"full","timeout_seconds":60}}),
        )
        .await
        .unwrap();
        if toolchain_unavailable {
            assert_eq!(verified["isError"], true, "{verified}");
            assert!(
                verified["structuredContent"]["checks"]
                    .as_array()
                    .is_some_and(|checks| checks.iter().any(|check| {
                        check["stderr_tail"]
                            .as_str()
                            .is_some_and(|stderr| stderr.contains(&unavailable))
                    })),
                "missing persistent toolchain gap for {} after edit: {verified}",
                fixture.required_program
            );
        } else {
            assert_eq!(verified["isError"], false, "{verified}");
            assert_eq!(verified["structuredContent"]["passed"], true, "{verified}");
        }
    }
}

#[tokio::test]
async fn verification_failure_context_redacts_secrets_and_never_claims_edit_ready() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname = \"failure-context-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(
        root.path().join("Cargo.lock"),
        "# This file is automatically @generated by Cargo.\nversion = 4\n\n[[package]]\nname = \"failure-context-fixture\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    let sentinel = "synthetic-private-fixture";
    fs::write(
        root.path().join("src/lib.rs"),
        format!(
            "pub fn broken() -> u32 {{\n    let password = \"{sentinel}\";\n    this is not valid Rust\n    1\n}}\n"
        ),
    )
    .unwrap();

    git(root.path(), &["init", "--quiet"]);
    git(root.path(), &["add", "."]);
    git(
        root.path(),
        &[
            "-c",
            "user.name=WCode",
            "-c",
            "user.email=wcode@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            "baseline",
        ],
    );

    let workspaces = Workspaces::new([root.path()], true, true).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    let state = AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
        workspaces,
        harness: ToolHarness::new(4).unwrap(),
        monitor: TaskMonitor::new([workspace_id]),
        tasks: TaskRuntime::default(),
    };

    let failed = call_tool(
        &state,
        json!({"name":"verify_project","arguments":{"level":"quick","timeout_seconds":60}}),
    )
    .await
    .unwrap();
    assert_eq!(failed["isError"], true, "{failed}");
    let context = failed["structuredContent"]["failure_context"]
        .as_array()
        .and_then(|contexts| {
            contexts
                .iter()
                .find(|context| context["path"] == "src/lib.rs")
        })
        .unwrap();
    assert_eq!(context["redacted"], true);
    assert_eq!(context["editable"], false);
    assert_eq!(context["precision"], "diagnostic_text+source_window");
    assert!(!serde_json::to_string(&failed).unwrap().contains(sentinel));
}
