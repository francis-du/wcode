use super::*;

fn passing_report() -> VerificationReport {
    VerificationReport {
        workspace: "demo".into(),
        level: "quick".into(),
        execution: "fixture".into(),
        phases_run: 1,
        passed: true,
        checks_run: 1,
        checks_failed: 0,
        elapsed_ms: 1,
        summary: "fixture passed".into(),
        checks: vec![crate::harness::VerificationCheck {
            id: "rust-check".into(),
            phase: 0,
            command: "cargo check --locked".into(),
            reason: "fixture".into(),
            success: true,
            exit_code: Some(0),
            elapsed_ms: 1,
            stdout_tail: String::new(),
            stderr_tail: String::new(),
            output_truncated: false,
        }],
    }
}

#[test]
fn verification_rejects_source_changes_before_recording_any_evidence() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("lib.rs"), "pub fn before() {}\n").unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let before = runtime.current_revision(&workspace).unwrap();
    fs::write(root.path().join("lib.rs"), "pub fn after() {}\n").unwrap();
    let error = runtime
        .record_verification_report("demo", &workspace, &before, &passing_report())
        .unwrap_err();
    assert!(error.to_string().contains("revision changed"));
    assert!(crate::evidence_store::load(&workspace).unwrap().is_empty());
}

#[test]
fn verification_rejects_design_only_changes_before_recording_evidence() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join(".wcode/design")).unwrap();
    let path = root.path().join(".wcode/design/product.yaml");
    fs::write(
        &path,
        "schema_version: 1\nid: product:demo\nname: Demo\nvision: First\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let before = runtime.current_revision(&workspace).unwrap();
    fs::write(
        &path,
        "schema_version: 1\nid: product:demo\nname: Demo\nvision: Revised\n",
    )
    .unwrap();
    let after = runtime.current_revision(&workspace).unwrap();
    assert_eq!(before.code, after.code);
    assert_ne!(before.design, after.design);
    let error = runtime
        .record_verification_report("demo", &workspace, &before, &passing_report())
        .unwrap_err();
    assert!(error.to_string().contains("revision changed"));
    assert!(crate::evidence_store::load(&workspace).unwrap().is_empty());
}

#[test]
fn incomplete_revisions_cannot_produce_verification_evidence() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let complete = runtime.current_revision(&workspace).unwrap();
    for design_only in [false, true] {
        let mut partial = complete.clone();
        if design_only {
            partial.design = Some(format!("{}:partial", complete.code));
        } else {
            partial.code.push_str(":partial");
        }
        let error = runtime
            .record_verification_report("demo", &workspace, &partial, &passing_report())
            .unwrap_err();
        assert!(error.to_string().contains("revision is incomplete"));
        assert!(crate::evidence_store::load(&workspace).unwrap().is_empty());
    }
}

#[test]
fn stable_verification_keeps_the_captured_revision() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("lib.rs"), "pub fn unchanged() {}\n").unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let before = runtime.current_revision(&workspace).unwrap();
    let evidence = runtime
        .record_verification_report("demo", &workspace, &before, &passing_report())
        .unwrap();
    assert!(!evidence.is_empty());
    assert!(evidence.iter().all(|item| {
        item.revision.code == before.code && item.revision.design == before.design
    }));
    assert_eq!(
        crate::evidence_store::load(&workspace).unwrap().len(),
        evidence.len()
    );
}
