use super::*;

fn ci_receipt_binding() -> crate::verification::receipt::ReceiptBinding {
    crate::verification::receipt::ReceiptBinding {
        repository: "francis-du/wcode".into(),
        repository_id: 1234,
        app_id: 7,
        check_name: "wcode/change-acceptance".into(),
    }
}

fn receipt_fields() -> crate::git_provider::CheckReceiptFields {
    crate::git_provider::CheckReceiptFields {
        schema_version: crate::git_provider::CHECK_RECEIPT_SCHEMA_VERSION,
        provider: "github".into(),
        repository: "francis-du/wcode".into(),
        repository_id: 1234,
        app_id: 7,
    }
}

fn ci_check_receipt(head_sha: &str) -> crate::git_provider::CheckReceipt {
    let check = crate::git_provider::CheckObservation {
        id: 9,
        head_sha: head_sha.to_owned(),
        name: "wcode/change-acceptance".into(),
        source_id: "github-app:7".into(),
        external_id: "wcode:test".into(),
        status: "completed".into(),
        conclusion: Some("success".into()),
    };
    let annotations = vec![crate::git_provider::CheckAnnotation {
        path: "src/lib.rs".into(),
        start_line: Some(1),
        end_line: Some(2),
        annotation_level: "warning".into(),
        title: "t".into(),
        message: "m".into(),
    }];
    let digest = crate::git_provider::receipt_digest::check_receipt_digest(
        &receipt_fields(),
        &check,
        &annotations,
    );
    crate::git_provider::CheckReceipt {
        schema_version: crate::git_provider::CHECK_RECEIPT_SCHEMA_VERSION,
        provider: "github".into(),
        repository: "francis-du/wcode".into(),
        repository_id: 1234,
        app_id: 7,
        check,
        annotations,
        digest,
    }
}

fn ci_git_binding() -> crate::verification::change::ExecutionGitBinding {
    crate::verification::change::ExecutionGitBinding {
        repository: format!("sha256:{}", "a".repeat(64)),
        head_sha: "b".repeat(40),
        tree_sha: "c".repeat(40),
        dirty: false,
        index_fingerprint: format!("sha256:{}", "d".repeat(64)),
    }
}

#[test]
fn ci_receipt_import_binds_to_the_current_revision_and_persists_observation() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let plan = runtime
        .create_plan_for_risk("demo", &workspace, RiskLevel::Low)
        .unwrap();
    let revision = plan.revision.clone().unwrap();
    let git = ci_git_binding();
    let receipt = ci_check_receipt(&git.head_sha);
    let evidence = runtime
        .verification_import_ci_receipt(
            "demo",
            &workspace,
            &plan.id,
            &receipt,
            &ci_receipt_binding(),
            Some(git),
        )
        .unwrap();
    assert_eq!(evidence.kind, EvidenceKind::CiReceipt);
    assert_eq!(evidence.authority, EvidenceAuthority::ExternalReceipt);
    assert_eq!(evidence.revision, revision);
    assert_eq!(evidence.subject, plan.subject);
    assert!(evidence.external_receipt.is_some());
    let loaded = crate::evidence_store::load(&workspace).unwrap();
    assert!(loaded.iter().any(|record| record.id == evidence.id));
}

#[test]
fn stale_ci_receipt_import_rejects_and_persists_nothing() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "pub fn value() -> u8 { 1 }\n").unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let plan = runtime
        .create_plan_for_risk("demo", &workspace, RiskLevel::Low)
        .unwrap();
    let git = ci_git_binding();
    let receipt = ci_check_receipt(&git.head_sha);
    fs::write(dir.path().join("lib.rs"), "pub fn value() -> u8 { 2 }\n").unwrap();
    assert!(runtime
        .verification_import_ci_receipt(
            "demo",
            &workspace,
            &plan.id,
            &receipt,
            &ci_receipt_binding(),
            Some(git),
        )
        .is_err());
    assert!(crate::evidence_store::load(&workspace).unwrap().is_empty());
}
