use super::*;
use std::fs;

#[test]
fn maintainability_review_ignores_generated_localization_sources() {
    let root = tempfile::tempdir().unwrap();
    let generated = root
        .path()
        .join("apps/demo/lib/l10n/app_localizations.dart");
    fs::create_dir_all(generated.parent().unwrap()).unwrap();
    fs::write(&generated, "// generated localization\n".repeat(1_500)).unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let mut findings = Vec::new();

    append_maintainability_findings(
        &workspace,
        &[ChangedFileReview {
            path: "apps/demo/lib/l10n/app_localizations.dart".into(),
            status: "modified".into(),
            staged: false,
            unstaged: true,
            untracked: false,
            category: "source".into(),
            additions: Some(900),
            deletions: Some(0),
            binary: false,
            risk_reasons: vec![],
        }],
        &mut findings,
    );

    assert!(findings.is_empty());
}
