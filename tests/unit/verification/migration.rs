use super::*;

fn revision() -> Revision {
    Revision {
        code: "sha256:fixture".into(),
        design: Some("sha256:design".into()),
    }
}

#[test]
fn migration_audit_detects_legacy_mixed_state_and_missing_replacement() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join(".wcode")).unwrap();
    std::fs::create_dir_all(root.path().join("src")).unwrap();
    std::fs::write(
        root.path().join(".wcode/migration.yaml"),
        r#"schema_version: 1
id: client-v2
scopes: [src]
required_paths: [src/client.rs]
transitions:
  - id: client-api
    legacy: "OldClient::new"
    replacement: "NewClient::new"
    min_replacement_matches: 2
"#,
    )
    .unwrap();
    std::fs::write(
        root.path().join("src/client.rs"),
        "fn first() { OldClient::new(); }\nfn second() { NewClient::new(); }\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();

    let report = audit(&workspace, &revision()).unwrap().unwrap();
    assert!(!report.passed);
    assert!(!report.truncated);
    assert_eq!(report.transitions.len(), 1);
    assert_eq!(report.transitions[0].legacy_matches, 1);
    assert_eq!(report.transitions[0].replacement_matches, 1);
    assert!(report.transitions[0].mixed_state);
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.code == "legacy_remaining"));
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.code == "replacement_missing"));
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.code == "mixed_state"));
    assert_eq!(report.findings[0].locations[0].path, "src/client.rs");
    assert_eq!(report.findings[0].locations[0].line, 1);

    std::fs::write(
        root.path().join("src/client.rs"),
        "fn first() { NewClient::new(); }\nfn second() { NewClient::new(); }\n",
    )
    .unwrap();
    let report = audit(&workspace, &revision()).unwrap().unwrap();
    assert!(report.passed, "{}", report.summary);
    assert!(report.findings.is_empty());
    assert_eq!(report.transitions[0].legacy_matches, 0);
    assert_eq!(report.transitions[0].replacement_matches, 2);
}

#[cfg(unix)]
#[test]
fn migration_policy_symlink_cannot_disable_the_gate() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join(".wcode")).unwrap();
    std::os::unix::fs::symlink(
        root.path().join("missing-policy.yaml"),
        root.path().join(".wcode/migration.yaml"),
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();

    let report = audit(&workspace, &revision()).unwrap().unwrap();
    assert!(!report.passed);
    assert_eq!(report.findings[0].code, "invalid_config");
    let context = context_summary(&workspace).unwrap();
    assert_eq!(context["configured"], true);
    assert_eq!(context["valid"], false);
}

#[test]
fn invalid_migration_policy_fails_closed_and_context_surfaces_it() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join(".wcode")).unwrap();
    std::fs::write(
        root.path().join(".wcode/migration.yaml"),
        "schema_version: 1\nid: empty-policy\nscopes: [src]\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let report = audit(&workspace, &revision()).unwrap().unwrap();
    assert!(!report.passed);
    assert_eq!(report.findings[0].code, "invalid_config");
    let context = context_summary(&workspace).unwrap();
    assert_eq!(context["configured"], true);
    assert_eq!(context["valid"], false);
    assert_eq!(context["precision"], "deterministic");
}
