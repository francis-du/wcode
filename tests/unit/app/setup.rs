use super::*;
use std::fs;
use std::io::Cursor;

#[test]
fn setup_scope_retries_invalid_choices_and_eof_never_means_consent() {
    for (input, expected) in [
        ("\n", SetupScope::Global),
        ("2\n", SetupScope::Project),
        ("mistyped\n2\n", SetupScope::Project),
    ] {
        let mut output = Vec::new();
        assert_eq!(
            choose_scope_with_io(&mut Cursor::new(input), &mut output).unwrap(),
            expected
        );
        if input.starts_with("mistyped") {
            assert!(String::from_utf8(output)
                .unwrap()
                .contains("Choose 1, 2 or 3"));
        }
    }
    for input in ["", "mistyped\n", "3\n", "q\n", "Q\n"] {
        assert!(choose_scope_with_io(&mut Cursor::new(input), &mut Vec::new()).is_err());
    }
}

#[test]
fn setup_github_enrollment_is_explicit_bounded_and_idempotent() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join(".wcode")).unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let input = GitHubEnrollmentInput {
        repository: Some("Owner/Repo"),
        repository_id: Some(123),
        app_id: Some(456),
        check_name: None,
    };

    let preview = configure_github_enrollment(&workspace, true, input).unwrap();
    assert_eq!(preview.status, "planned");
    assert!(!preview.credential_stored);
    assert!(!preview.remote_changes);
    assert!(!root.path().join(GITHUB_ENROLLMENT_FILE).exists());

    let enrolled = configure_github_enrollment(&workspace, false, input).unwrap();
    assert_eq!(enrolled.status, "enrolled");
    let existing =
        configure_github_enrollment(&workspace, false, GitHubEnrollmentInput::default()).unwrap();
    assert_eq!(existing.status, "existing");
    assert_eq!(existing.enrollment.unwrap().repository, "owner/repo");

    let text = fs::read_to_string(root.path().join(GITHUB_ENROLLMENT_FILE)).unwrap();
    assert!(!text.contains("token"));
    assert!(!text.contains("credential"));
}

#[test]
fn setup_github_enrollment_rejects_partial_or_conflicting_identity_and_readonly_writes() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join(".wcode")).unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    assert!(configure_github_enrollment(
        &workspace,
        false,
        GitHubEnrollmentInput {
            repository: Some("owner/repo"),
            repository_id: None,
            app_id: Some(2),
            check_name: None,
        },
    )
    .is_err());

    configure_github_enrollment(
        &workspace,
        false,
        GitHubEnrollmentInput {
            repository: Some("owner/repo"),
            repository_id: Some(1),
            app_id: Some(2),
            check_name: None,
        },
    )
    .unwrap();
    assert!(configure_github_enrollment(
        &workspace,
        false,
        GitHubEnrollmentInput {
            repository: Some("owner/other"),
            repository_id: Some(3),
            app_id: Some(4),
            check_name: None,
        },
    )
    .is_err());

    let readonly_root = tempfile::tempdir().unwrap();
    fs::create_dir_all(readonly_root.path().join(".wcode")).unwrap();
    let readonly = Workspace::new(readonly_root.path(), false, false).unwrap();
    let blocked = configure_github_enrollment(
        &readonly,
        false,
        GitHubEnrollmentInput {
            repository: Some("owner/repo"),
            repository_id: Some(1),
            app_id: Some(2),
            check_name: None,
        },
    )
    .unwrap();
    assert_eq!(blocked.status, "blocked");
    assert!(!readonly_root.path().join(GITHUB_ENROLLMENT_FILE).exists());
}
