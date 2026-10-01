use super::*;
use crate::engineering_journal::{persist, EngineeringMilestone};
use crate::harness::ToolHarness;
use std::fs;

fn attach(pack: &mut Value, workspace: &Workspace, query: &str, budget: usize) {
    super::attach(pack, workspace, query, budget, &HashSet::new());
}

fn observe(workspace: &Workspace, code: EngineeringFailureCode, path: &str) {
    let mut event =
        EngineeringMilestone::new("apply_edits", "change", "failed", 1, vec![path.into()]).unwrap();
    event.failure_codes = vec![code];
    persist(workspace, &event).unwrap();
}

#[test]
fn context_lessons_survive_new_harness_and_workspace_without_model_identity() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::write(
        root.path().join("src/lib.rs"),
        "pub fn value() -> u32 { 1 }\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    observe(
        &workspace,
        EngineeringFailureCode::ShaMismatch,
        "src/lib.rs",
    );
    observe(
        &workspace,
        EngineeringFailureCode::ShaMismatch,
        "src/lib.rs",
    );
    drop(workspace);
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(2).unwrap();
    let pack = harness
        .agent_context("demo", &workspace, "edit value", 4_000, &[])
        .unwrap();
    assert_eq!(pack["lessons"]["kind"], "historical_advisory");
    assert_eq!(pack["lessons"]["rules"][0]["code"], "sha_mismatch");
    assert_eq!(pack["lessons"]["rules"][0]["observations"], 2);
    assert_eq!(pack["lessons"]["rules"][0]["recurring"], true);
    assert_ne!(pack["readiness"]["verify"], "passed");
    // The memory never supplies an approver or a verification result.
    assert!(pack["lessons"].get("approval").is_none());
    assert!(pack["lessons"].get("passed").is_none());
}

#[test]
fn context_lessons_are_scoped_deduplicated_and_do_not_teach_success() {
    let first = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(first.path(), true, false).unwrap();
    let other_workspace = Workspace::new(other.path(), true, false).unwrap();
    let mut event =
        EngineeringMilestone::new("apply_edits", "change", "failed", 1, Vec::new()).unwrap();
    event.failure_codes = vec![EngineeringFailureCode::ShaMismatch];
    persist(&workspace, &event).unwrap();
    // A second valid record carrying the same event identity is not another mistake.
    event.timestamp_ms += 1;
    persist(&workspace, &event).unwrap();
    event.event_id = Some(uuid::Uuid::new_v4().to_string());
    event.outcome = "succeeded".into();
    event.failure_codes.clear();
    persist(&workspace, &event).unwrap();
    let mut pack = json!({"files":[]});
    attach(&mut pack, &workspace, "edit source", 4_000);
    assert_eq!(pack["lessons"]["rules"][0]["observations"], 1);
    assert_eq!(pack["lessons"]["rules"][0]["recurring"], false);
    let mut other_pack = json!({"files":[]});
    attach(&mut other_pack, &other_workspace, "edit source", 4_000);
    assert!(other_pack.get("lessons").is_none());
}

#[test]
fn context_lessons_filter_irrelevant_errors_and_removed_paths() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    observe(
        &workspace,
        EngineeringFailureCode::VerificationFailure,
        "removed.rs",
    );
    let mut pack = json!({"files":[]});
    attach(&mut pack, &workspace, "inspect logo", 4_000);
    assert!(pack.get("lessons").is_none());
    attach(&mut pack, &workspace, "run verification check", 4_000);
    assert_eq!(pack["lessons"]["rules"][0]["code"], "verification_failure");
    assert_eq!(pack["lessons"]["rules"][0]["paths"], json!([]));
}

#[test]
fn context_lessons_keep_tight_budget_and_current_source_sha() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::write(
        root.path().join("src/lib.rs"),
        "pub fn value() -> u32 { 1 }\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    for code in [
        EngineeringFailureCode::ShaMismatch,
        EngineeringFailureCode::ProtectedPath,
        EngineeringFailureCode::SourceLimit,
        EngineeringFailureCode::VerificationFailure,
    ] {
        observe(&workspace, code, "src/lib.rs");
    }
    let source = workspace.load_source("src/lib.rs").unwrap();
    let pack = ToolHarness::new(2)
        .unwrap()
        .agent_context("demo", &workspace, "edit value", 1_000, &[])
        .unwrap();
    assert!(serde_json::to_vec(&pack).unwrap().len() <= 4_000);
    assert_eq!(pack["lessons"]["rules"].as_array().unwrap().len(), 1);
    assert_eq!(pack["lessons"]["omitted"], 3);
    assert!(pack["files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|file| file["sha256"] == source.sha256));
}

#[test]
fn context_lessons_tight_budget_retains_the_same_complete_target_body() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    let mut source = String::from("pub fn value() -> u32 {\n");
    for index in 0..20 {
        source.push_str(&format!("    let _v{index} = {index};\n"));
    }
    source.push_str("    1\n}\n");
    fs::write(root.path().join("src/lib.rs"), source).unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(2).unwrap();
    let before = harness
        .agent_context("demo", &workspace, "edit value", 1_000, &[])
        .unwrap();
    assert_eq!(before["hot_source"][0]["body"]["truncated"], false);
    for code in [
        EngineeringFailureCode::ShaMismatch,
        EngineeringFailureCode::ProtectedPath,
        EngineeringFailureCode::SourceLimit,
        EngineeringFailureCode::VerificationFailure,
    ] {
        observe(&workspace, code, "src/lib.rs");
    }
    let after = harness
        .agent_context("demo", &workspace, "edit value", 1_000, &[])
        .unwrap();
    assert_eq!(
        after["hot_source"][0]["body"],
        before["hot_source"][0]["body"]
    );
    assert_eq!(
        after["hot_source"][0]["sha256"],
        before["hot_source"][0]["sha256"]
    );
    assert!(serde_json::to_vec(&after).unwrap().len() <= 4_000);
}

#[test]
fn context_lessons_compaction_yields_before_current_source_or_checks() {
    let source =
        json!({"path":"src/lib.rs","sha256":"current-sha","body":{"content":"current source"}});
    let check = json!({"id":"required-check"});
    let mut pack = json!({"hot_source":[source.clone()],"checks":[check.clone()],
        "lessons":{"rules":[{"code":"sha_mismatch","paths":["src/lib.rs"]},
            {"code":"timeout","paths":[]}],"omitted":0}});
    while compact(&mut pack) {}
    assert!(pack.get("lessons").is_none());
    assert_eq!(pack["hot_source"], json!([source]));
    assert_eq!(pack["checks"], json!([check]));
}

#[test]
fn context_lessons_corrupt_history_is_reported_without_changing_readiness() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let state = crate::evidence_store::workspace_state_directory(&workspace).unwrap();
    fs::create_dir_all(&state).unwrap();
    fs::write(state.join("engineering-journal"), "not a directory").unwrap();
    let mut pack = json!({"readiness":{"verify":"not_run"},"files":[]});
    attach(&mut pack, &workspace, "run tests", 4_000);
    assert_eq!(pack["lessons"]["available"], false);
    assert_eq!(pack["readiness"]["verify"], "not_run");
}

#[test]
fn context_lessons_all_invalid_history_is_explicit_without_changing_readiness() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let directory = crate::evidence_store::workspace_state_directory(&workspace)
        .unwrap()
        .join("engineering-journal");
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("invalid.json"), "invalid record").unwrap();
    let mut pack = json!({"readiness":{"verify":"not_run","edit":"ready"},"files":[]});
    let readiness = pack["readiness"].clone();
    attach(&mut pack, &workspace, "edit source", 4_000);
    assert_eq!(pack["lessons"]["kind"], "historical_advisory");
    assert_eq!(pack["lessons"]["available"], true);
    assert_eq!(pack["lessons"]["coverage"], "partial");
    assert_eq!(pack["lessons"]["rules"], json!([]));
    assert_eq!(pack["readiness"], readiness);
}

use crate::native_acceptance_fixture as native_memory_fixture;

#[tokio::test]
async fn context_failure_memory_next_model_recalls_real_native_recovery_without_promoting_self_reports(
) {
    use crate::evidence::{EvidenceAuthority, EvidenceKind, EvidenceResult};
    use crate::monitor::TaskMonitor;
    let fixture = native_memory_fixture::NativeFixture::new();
    assert_eq!(
        fixture.root.path().canonicalize().unwrap(),
        fixture.workspace.root()
    );
    fs::write(
        fixture.root.path().join(".wcode/failure-rules.yaml"),
        r#"schema_version: 1
rules:
  - id: node-test-advice
    check: node-test
    guidance: Inspect the actual test and rerun the complete required checks.
  - id: unrelated-rule
    check: unrelated-check
    guidance: This rule must not be injected for the value test.
"#,
    )
    .unwrap();
    fs::write(
        fixture.root.path().join("src/value.js"),
        "exports.value = function value() { return 2; };\n",
    )
    .unwrap();
    native_memory_fixture::git(fixture.root.path(), &["add", "."]);
    native_memory_fixture::commit(fixture.root.path(), "failing native value");
    let monitor = TaskMonitor::new([native_memory_fixture::ID.to_owned()]);
    let failed = fixture
        .harness
        .verify_project_mode(
            native_memory_fixture::ID,
            &fixture.workspace,
            ("full", false),
            60,
            &monitor,
        )
        .await
        .unwrap();
    assert!(!failed.passed);
    assert!(failed
        .checks
        .iter()
        .any(|check| check.id == "node-test" && !check.success && check.exit_code.is_some()));
    let proofs = crate::evidence_store::load(&fixture.workspace).unwrap();
    let actual_failure = proofs
        .iter()
        .filter(|record| {
            record.kind == EvidenceKind::Verification && record.result == EvidenceResult::Fail
        })
        .max_by_key(|record| record.timestamp_ms)
        .unwrap();
    assert_eq!(
        actual_failure.authority,
        EvidenceAuthority::NativeVerification
    );
    let mut claimed_pass = actual_failure.clone();
    claimed_pass.id = "self-reported-untrusted-pass".into();
    claimed_pass.producer = "self-reported:mcp:fixture".into();
    claimed_pass.authority = EvidenceAuthority::SelfReported;
    claimed_pass.result = EvidenceResult::Pass;
    claimed_pass.timestamp_ms += 1;
    for check in &mut claimed_pass.execution_receipt.as_mut().unwrap().checks {
        check.result = EvidenceResult::Pass;
    }
    let attempted = engineering_journal::failure_memory::observe_native_verification(
        &fixture.workspace,
        &actual_failure.revision,
        actual_failure.execution_git_binding.as_ref(),
        actual_failure.execution_policy_binding.as_deref(),
        &[claimed_pass],
        &[],
    )
    .unwrap();
    assert_eq!(attempted.recoveries_written, 0);
    let candidate = ToolHarness::new(2)
        .unwrap()
        .agent_context(
            native_memory_fixture::ID,
            &fixture.workspace,
            "inspect node-test src/value.js",
            6_000,
            &[],
        )
        .unwrap();
    assert!(candidate["failure_memory"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|item| item["status"] == "unverified"));
    assert_eq!(candidate["failure_memory"]["can_authorize"], false);
    assert_eq!(
        candidate["failure_memory"]["proves_current_verification"],
        false
    );

    fs::write(
        fixture.root.path().join("src/value.js"),
        "exports.value = function value() { return 1; };\n",
    )
    .unwrap();
    native_memory_fixture::git(fixture.root.path(), &["add", "src/value.js"]);
    native_memory_fixture::commit(fixture.root.path(), "repaired native value");
    let passed = fixture
        .harness
        .verify_project_mode(
            native_memory_fixture::ID,
            &fixture.workspace,
            ("full", false),
            60,
            &monitor,
        )
        .await
        .unwrap();
    assert!(passed.passed, "{passed:?}");
    assert_ne!(failed.execution_git_binding, passed.execution_git_binding);
    let new_workspace = Workspace::new(fixture.root.path(), true, false).unwrap();
    let fresh_harness = ToolHarness::new(2).unwrap();
    let mut pack = fresh_harness
        .agent_context(
            native_memory_fixture::ID,
            &new_workspace,
            "inspect node-test src/value.js",
            6_000,
            &[],
        )
        .unwrap();
    let memory = &pack["failure_memory"];
    assert_eq!(memory["kind"], "historical_advisory");
    assert_eq!(memory["can_authorize"], false);
    assert_eq!(memory["proves_current_verification"], false);
    let items = memory["items"].as_array().unwrap();
    assert!(items.len() <= 2);
    let recovery = items
        .iter()
        .find(|item| item["status"] == "verified_same_check_recovery")
        .unwrap();
    assert_eq!(recovery["cause_proven"], false);
    let actual = crate::evidence_store::load(&new_workspace).unwrap();
    assert!(actual
        .iter()
        .any(|record| Some(record.id.as_str()) == recovery["failure_evidence_id"].as_str()));
    assert!(actual
        .iter()
        .any(|record| Some(record.id.as_str()) == recovery["pass_evidence_id"].as_str()));
    assert!(items.iter().any(|item| item["id"] == "node-test-advice"
        && item["source"] == "repository_advisory"
        && item["status"] == "unverified"));
    assert!(!serde_json::to_string(memory)
        .unwrap()
        .contains("unrelated-rule"));
    assert_ne!(pack["readiness"]["verify"], "passed");
    let source = new_workspace.load_source("src/value.js").unwrap();
    assert!(pack["files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|file| file["sha256"] == source.sha256));
    assert!(pack["hot_source"]
        .as_array()
        .unwrap()
        .iter()
        .any(|body| body["path"] == "src/value.js"
            && body["sha256"] == source.sha256
            && body["body"]["content"]
                .as_str()
                .is_some_and(|content| content.contains("return 1;"))));
    let current = [
        "files",
        "hot_source",
        "checks",
        "readiness",
        "core_constraints",
    ]
    .map(|key| (key, pack[key].clone()));
    while compact(&mut pack) {}
    assert!(pack.get("failure_memory").is_none());
    for (key, value) in current {
        assert_eq!(pack[key], value);
    }
}

#[tokio::test]
async fn failure_memory_generic_context_recalls_native_checks_after_restart_without_paths() {
    use crate::monitor::TaskMonitor;
    let fixture = native_memory_fixture::NativeFixture::new();
    fs::write(
        fixture.root.path().join("src/value.js"),
        "exports.value = function value() { return 2; };\n",
    )
    .unwrap();
    native_memory_fixture::git(fixture.root.path(), &["add", "src/value.js"]);
    native_memory_fixture::commit(fixture.root.path(), "committed failing value");
    let monitor = TaskMonitor::new([native_memory_fixture::ID.to_owned()]);
    let failed = fixture
        .harness
        .verify_project_mode(
            native_memory_fixture::ID,
            &fixture.workspace,
            ("full", false),
            60,
            &monitor,
        )
        .await
        .unwrap();
    assert!(!failed.passed);
    assert!(failed
        .checks
        .iter()
        .any(|check| check.id == "node-test" && !check.success));
    let records = crate::evidence_store::load(&fixture.workspace).unwrap();
    let failure = records
        .iter()
        .find(|record| {
            record.kind == crate::evidence::EvidenceKind::Verification
                && record.result == crate::evidence::EvidenceResult::Fail
        })
        .unwrap();
    // This reproduces clean-commit failures with no affected path metadata.
    engineering_journal::failure_memory::observe_native_verification(
        &fixture.workspace,
        &failure.revision,
        failure.execution_git_binding.as_ref(),
        failure.execution_policy_binding.as_deref(),
        std::slice::from_ref(failure),
        &[],
    )
    .unwrap();
    let workspace = Workspace::new(fixture.root.path(), true, false).unwrap();
    let harness = ToolHarness::new(2).unwrap();
    let pack = harness
        .agent_context(
            native_memory_fixture::ID,
            &workspace,
            "inspect value",
            6_000,
            &[],
        )
        .unwrap();
    let memory = &pack["failure_memory"];
    assert_eq!(memory["kind"], "historical_advisory");
    assert_eq!(memory["can_authorize"], false);
    assert_eq!(memory["proves_current_verification"], false);
    assert!(memory["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| { item["check"]["id"] == "node-test" && item["status"] == "unverified" }));
    assert_ne!(pack["readiness"]["verify"], "passed");
    let isolated = native_memory_fixture::NativeFixture::new();
    let other = ToolHarness::new(2)
        .unwrap()
        .agent_context(
            native_memory_fixture::ID,
            &isolated.workspace,
            "inspect value",
            6_000,
            &[],
        )
        .unwrap();
    assert!(other.get("failure_memory").is_none());
}

#[test]
fn context_failure_memory_literal_rule_is_advisory_and_corrupt_store_does_not_change_readiness() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join(".wcode")).unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/value.rs"), "pub fn value() {}\n").unwrap();
    fs::write(
        root.path().join(".wcode/failure-rules.yaml"),
        r#"schema_version: 1
rules:
  - id: module-rule
    literal: module bound
    path: src
    guidance: Split the responsibility and run current native checks.
  - id: unrelated-rule
    literal: another problem
    guidance: Never select unrelated advice.
"#,
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let event = EngineeringMilestone::new(
        "apply_edits",
        "change",
        "failed",
        1,
        vec!["src/value.rs".into()],
    )
    .unwrap();
    engineering_journal::failure_memory::observe_tool_failure(
        &workspace,
        &event,
        Some("module bound PRIVATE_TRANSIENT_ERROR_BODY"),
    )
    .unwrap();
    let mut pack =
        json!({"files":[{"path":"src/value.rs"}],"readiness":{"verify":"not_run","edit":"ready"}});
    let readiness = pack["readiness"].clone();
    attach(&mut pack, &workspace, "inspect source", 1_000);
    let memory = &pack["failure_memory"];
    assert_eq!(memory["items"].as_array().unwrap().len(), 1);
    assert_eq!(memory["items"][0]["source"], "repository_advisory");
    assert_eq!(memory["items"][0]["status"], "unverified");
    let encoded = serde_json::to_string(memory).unwrap();
    assert!(
        !encoded.contains("PRIVATE_TRANSIENT_ERROR_BODY") && !encoded.contains("unrelated-rule")
    );
    assert_eq!(pack["readiness"], readiness);
    let directory = crate::evidence_store::workspace_state_directory(&workspace)
        .unwrap()
        .join("failure-memory");
    fs::write(directory.join("corrupt.json"), "{broken").unwrap();
    attach(&mut pack, &workspace, "inspect source", 4_000);
    assert_eq!(pack["failure_memory"]["available"], false);
    assert_eq!(pack["readiness"], readiness);
}

#[cfg(unix)]
#[test]
fn context_lessons_isolate_real_non_utf8_roots_with_identical_lossy_names() {
    use std::os::unix::ffi::OsStringExt;
    let parent = tempfile::tempdir().unwrap();
    let mut workspaces = Vec::new();
    for byte in [0xff, 0xfe] {
        let path = parent
            .path()
            .join(std::ffi::OsString::from_vec(vec![b'r', b'-', byte]));
        match fs::create_dir(&path) {
            Ok(()) => {}
            #[cfg(target_os = "macos")]
            Err(error) if error.raw_os_error() == Some(libc::EILSEQ) => {
                eprintln!(
                    "not exercised: this macOS filesystem rejects non-UTF-8 directories (EILSEQ)"
                );
                return;
            }
            Err(error) => panic!("cannot create non-UTF-8 Workspace fixture: {error}"),
        }
        fs::create_dir(path.join("src")).unwrap();
        fs::write(path.join("src/lib.rs"), "pub fn value() {}\n").unwrap();
        workspaces.push(Workspace::new(&path, true, false).unwrap());
    }
    let second = workspaces.pop().unwrap();
    let first = workspaces.pop().unwrap();
    assert_ne!(first.root(), second.root());
    assert!(first.root().to_str().is_none());
    assert_eq!(
        first.root().to_string_lossy(),
        second.root().to_string_lossy()
    );

    observe(&first, EngineeringFailureCode::ShaMismatch, "src/lib.rs");
    let mut first_pack = json!({"readiness":{"verify":"not_run"},"files":[{"path":"src/lib.rs"}]});
    attach(&mut first_pack, &first, "edit value", 4_000);
    assert_eq!(first_pack["lessons"]["rules"][0]["code"], "sha_mismatch");
    let mut second_pack = json!({"readiness":{"verify":"not_run"},"files":[{"path":"src/lib.rs"}]});
    attach(&mut second_pack, &second, "edit value", 4_000);
    assert!(second_pack.get("lessons").is_none());

    observe(&second, EngineeringFailureCode::ProtectedPath, "src/lib.rs");
    attach(&mut second_pack, &second, "edit value", 4_000);
    assert_eq!(second_pack["lessons"]["rules"].as_array().unwrap().len(), 1);
    assert_eq!(second_pack["lessons"]["rules"][0]["code"], "protected_path");
    assert_eq!(second_pack["lessons"]["rules"][0]["observations"], 1);
    let mut first_reloaded = json!({"files":[{"path":"src/lib.rs"}]});
    attach(&mut first_reloaded, &first, "edit value", 4_000);
    assert_eq!(
        first_reloaded["lessons"]["rules"].as_array().unwrap().len(),
        1
    );
    assert_eq!(
        first_reloaded["lessons"]["rules"][0]["code"],
        "sha_mismatch"
    );
    assert_eq!(first_pack["readiness"]["verify"], "not_run");
    assert_eq!(second_pack["readiness"]["verify"], "not_run");
}
