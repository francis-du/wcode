use super::*;
use crate::native_acceptance_fixture as native_fixture;
use crate::verification::acceptance::AcceptanceState;
use native_fixture::{assert_ready, commit, git, operator_receipt, NativeFixture, ID};

#[tokio::test]
async fn native_acceptance_retains_real_check_timings_without_counting_recapture_as_execution() {
    let fixture = NativeFixture::new();
    fixture.activate();
    let ready = fixture.verify_and_review().await;
    assert_ready(ready.record());
    let record = serde_json::to_value(ready.record()).unwrap();
    let evidence = crate::evidence_store::load(&fixture.workspace).unwrap();
    let measured = record["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item.get("execution_timing").is_some())
        .collect::<Vec<_>>();
    assert!(
        !measured.is_empty(),
        "real native check durations were lost in the Acceptance projection"
    );
    for item in measured {
        let source = evidence
            .iter()
            .find(|source| source.id == item["id"].as_str().unwrap())
            .unwrap();
        let timing = &item["execution_timing"];
        assert_eq!(
            source.summary.as_deref(),
            Some(
                format!(
                    "verification-metrics-v1;elapsed_ms={};phase={}",
                    timing["execution_ms"], timing["phase"]
                )
                .as_str()
            )
        );
        assert_eq!(timing["source"], "native_check_execution/v1");
        assert_eq!(item["freshness"], "current");
    }
    fixture
        .harness
        .record_acceptance(ID, &fixture.workspace, &fixture.base, fixture.target())
        .await
        .unwrap();
    let before = fixture
        .harness
        .acceptance_metrics(ID, &fixture.workspace)
        .unwrap();
    assert_eq!(before["verification_duration"]["available"], true);
    assert!(
        before["verification_duration"]["sample_count"]
            .as_u64()
            .unwrap()
            > 0
    );
    let restarted = ToolHarness::new(2).unwrap();
    restarted
        .record_acceptance(ID, &fixture.workspace, &fixture.base, fixture.target())
        .await
        .unwrap();
    let after = restarted
        .acceptance_metrics(ID, &fixture.workspace)
        .unwrap();
    assert_eq!(
        before["verification_duration"],
        after["verification_duration"]
    );
}

#[tokio::test]
async fn native_acceptance_real_policy_plan_checks_and_reviews_are_required_before_ready() {
    let fixture = NativeFixture::new();
    let missing = fixture.capture().await;
    assert_ne!(missing.record().state, AcceptanceState::Ready);
    assert!(missing
        .record()
        .reasons
        .iter()
        .any(|reason| reason.code == "policy_inactive"));
    fixture.activate();
    let plan = fixture
        .harness
        .acceptance_plan(ID, &fixture.workspace, &fixture.base, fixture.target())
        .await
        .unwrap();
    let pending = fixture.capture().await;
    assert_ne!(pending.record().state, AcceptanceState::Ready);
    assert_eq!(pending.record().plan.as_ref().unwrap().id, plan.id);
    assert!(pending
        .record()
        .checks
        .iter()
        .any(|check| check.evidence_ids.is_empty()));
    fixture.submit_fixture_reviews(&plan);
    let ready = fixture.verify_and_review().await;
    assert_ready(ready.record());
    assert!(ready
        .record()
        .checks
        .iter()
        .any(|check| check.id == "node-lint"));
    assert!(ready
        .record()
        .checks
        .iter()
        .any(|check| check.id == "node-test"));
}

#[tokio::test]
async fn native_acceptance_ready_survives_restart_as_fresh_capture_not_history_authority() {
    let fixture = NativeFixture::new();
    fixture.activate();
    let ready = fixture.verify_and_review().await;
    fixture
        .harness
        .record_acceptance(ID, &fixture.workspace, &fixture.base, fixture.target())
        .await
        .unwrap();
    let restarted = ToolHarness::new(2).unwrap();
    let current = restarted
        .capture_acceptance(ID, &fixture.workspace, &fixture.base, fixture.target())
        .await
        .unwrap();
    assert_ready(current.record());
    assert_eq!(current.record().digest(), ready.record().digest());
    let exported = restarted
        .acceptance_history(ID, &fixture.workspace)
        .unwrap();
    assert_eq!(exported["authority"], "historical_only");
    assert_eq!(exported["current_acceptance"], false);
    assert!(
        exported["retained"].as_u64().unwrap() >= 2,
        "pending and ready observations retained"
    );
}

#[tokio::test]
async fn native_acceptance_new_head_same_tree_and_dirty_source_do_not_reuse_old_proof() {
    let mut fixture = NativeFixture::new();
    fixture.activate();
    let ready = fixture.verify_and_review().await;
    let previous_revision = ready.record().revision.clone();
    std::fs::write(
        fixture.root.path().join("src/value.js"),
        "exports.value = () => 2;\n",
    )
    .unwrap();
    let dirty = fixture.capture().await;
    assert_ne!(dirty.record().state, AcceptanceState::Ready);
    assert!(!dirty.record().git.complete);
    std::fs::write(
        fixture.root.path().join("src/value.js"),
        "exports.value = function value() { return 1; };\n",
    )
    .unwrap();
    commit(fixture.root.path(), "new commit same tree");
    fixture.head = git(fixture.root.path(), &["rev-parse", "HEAD"]);
    let changed = fixture.capture().await;
    assert!(changed.record().git.complete);
    assert_eq!(
        changed.record().revision,
        previous_revision,
        "same bytes retain content Revision"
    );
    assert_ne!(
        changed.record().state,
        AcceptanceState::Ready,
        "old Git receipts cannot approve a new commit"
    );
}

#[tokio::test]
async fn native_acceptance_revoked_policy_and_changed_check_definition_fail_closed() {
    let fixture = NativeFixture::new();
    fixture.activate();
    fixture.verify_and_review().await;
    let revision = fixture
        .harness
        .current_revision(&fixture.workspace)
        .unwrap();
    fixture
        .harness
        .acceptance_policy_revoke_authorized(
            ID,
            &fixture.workspace,
            1,
            &revision,
            operator_receipt(),
        )
        .unwrap();
    let revoked = fixture.capture().await;
    assert_ne!(revoked.record().state, AcceptanceState::Ready);
    assert!(revoked
        .record()
        .reasons
        .iter()
        .any(|reason| reason.code == "policy_revoked"));

    let fixture = NativeFixture::new();
    fixture.activate();
    fixture.verify_and_review().await;
    let path = fixture.root.path().join("package.json");
    let mut package: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    package["scripts"]["lint"] = json!("node --check tests/value.test.js");
    std::fs::write(path, serde_json::to_vec(&package).unwrap()).unwrap();
    assert_eq!(
        fixture
            .harness
            .acceptance_policy_status(ID, &fixture.workspace)
            .unwrap()["status"],
        "stale_definition"
    );
    let changed = fixture.capture().await;
    assert_ne!(changed.record().state, AcceptanceState::Ready);
}

#[tokio::test]
async fn native_acceptance_corrupt_historical_record_is_rejected_and_never_imported() {
    let fixture = NativeFixture::new();
    fixture.activate();
    fixture.verify_and_review().await;
    fixture
        .harness
        .record_acceptance(ID, &fixture.workspace, &fixture.base, fixture.target())
        .await
        .unwrap();
    let history = crate::evidence_store::workspace_state_directory(&fixture.workspace)
        .unwrap()
        .join("acceptance-history");
    let path = std::fs::read_dir(history)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    std::fs::write(path, b"{\"state\":\"ready\"}").unwrap();
    let restarted = ToolHarness::new(2).unwrap();
    assert!(restarted
        .acceptance_history(ID, &fixture.workspace)
        .is_err());
    // Fresh native inputs remain independently valid; corrupt history is not read as proof.
    let current = restarted
        .capture_acceptance(ID, &fixture.workspace, &fixture.base, fixture.target())
        .await
        .unwrap();
    assert_ready(current.record());
}

#[tokio::test]
async fn native_acceptance_capture_rejects_input_drift() {
    let fixture = NativeFixture::new();
    fixture.activate();
    let ready = fixture.verify_and_review().await;
    let authority =
        harness_policy_select::policy_authority_fingerprint(&fixture.workspace, ID).unwrap();
    fixture
        .harness
        .guard_acceptance_inputs(
            ID,
            &fixture.workspace,
            &ready.record().revision,
            &authority,
            &ready.record().git,
        )
        .await
        .unwrap();

    std::fs::write(
        fixture.root.path().join("README.md"),
        "# Changed during capture\n",
    )
    .unwrap();
    let error = fixture
        .harness
        .guard_acceptance_inputs(
            ID,
            &fixture.workspace,
            &ready.record().revision,
            &authority,
            &ready.record().git,
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("revision changed"));
    std::fs::write(fixture.root.path().join("README.md"), "# Candidate\n").unwrap();
    fixture
        .harness
        .guard_acceptance_inputs(
            ID,
            &fixture.workspace,
            &ready.record().revision,
            &authority,
            &ready.record().git,
        )
        .await
        .unwrap();

    commit(fixture.root.path(), "same bytes changed Git during capture");
    let error = fixture
        .harness
        .guard_acceptance_inputs(
            ID,
            &fixture.workspace,
            &ready.record().revision,
            &authority,
            &ready.record().git,
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("Git candidate changed"));
    let head = git(fixture.root.path(), &["rev-parse", "HEAD"]);
    let current = fixture
        .harness
        .capture_acceptance(
            ID,
            &fixture.workspace,
            &fixture.base,
            GitChangeTarget::Commit { revision: head },
        )
        .await
        .unwrap();
    fixture
        .harness
        .acceptance_policy_revoke_authorized(
            ID,
            &fixture.workspace,
            1,
            &current.record().revision,
            operator_receipt(),
        )
        .unwrap();
    assert!(
        fixture
            .harness
            .guard_acceptance_inputs(
                ID,
                &fixture.workspace,
                &current.record().revision,
                &authority,
                &current.record().git,
            )
            .await
            .is_err(),
        "an observed Policy generation drift must reject the old capture"
    );
}
