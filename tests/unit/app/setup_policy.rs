use super::*;
use std::fs;
use std::io::Cursor;

fn fixture(project: &str) -> (tempfile::TempDir, Workspace) {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join(".wcode/design")).unwrap();
    fs::write(root.path().join(".wcode/project.yaml"), project).unwrap();
    fs::write(root.path().join(".wcode/design/product.yaml"),
        "schema_version: 1\nid: product:setup\nname: Setup fixture\nvision: Suggest a draft without executing checks.\n").unwrap();
    fs::write(root.path().join("package.json"),
        r#"{"name":"setup-fixture","scripts":{"lint":"node --check NEVER-EXECUTE.js","test":"node -e \"require('fs').writeFileSync('EXECUTED', 'wrong')\""}} "#).unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    (root, workspace)
}

#[test]
fn setup_policy_metadata_suggestion_never_executes_activates_or_writes_on_preview() {
    let project = "schema_version: 1\nname: Setup fixture\ndescription: Preserve this project.\n";
    let (root, workspace) = fixture(project);
    let suggestion = suggest(&workspace).unwrap();
    assert_eq!(suggestion.status, "draft_available");
    assert_eq!(suggestion.authority, "repository_draft_only");
    assert!(suggestion.activation_required);
    assert_eq!(
        suggestion
            .checks
            .iter()
            .map(|check| check.id.as_str())
            .collect::<Vec<_>>(),
        ["node-lint", "node-test"]
    );
    let policy: AcceptancePolicy =
        serde_yaml::from_str(suggestion.policy_yaml.as_ref().unwrap()).unwrap();
    assert_eq!(policy.requirements.minimum_level, PolicyLevel::Full);
    assert!(policy.requirements.human_approval);
    assert_eq!(
        fs::read_to_string(root.path().join(design::PROJECT_FILE)).unwrap(),
        project
    );
    assert!(!root.path().join("EXECUTED").exists());
    assert!(
        crate::verification::policy_store::load(&workspace, "setup-project")
            .unwrap()
            .is_none()
    );
    let serialized = serde_json::to_string(&suggestion).unwrap();
    assert!(!serialized.contains("NEVER-EXECUTE"));
    assert!(!serialized.contains("writeFileSync"));
    assert!(!serialized.contains("original"));
}

#[test]
fn setup_policy_draft_confirmation_rejects_non_tty_eof_and_negative_input() {
    for (interactive, answer) in [(false, "yes\n"), (true, ""), (true, "\n"), (true, "no\n")] {
        let project = "schema_version: 1\nname: Setup fixture\n";
        let (root, workspace) = fixture(project);
        let suggestion = suggest(&workspace).unwrap();
        let result = review_with_io(
            &workspace,
            &suggestion,
            interactive,
            &mut Cursor::new(answer),
            &mut Vec::new(),
        );
        if interactive {
            assert!(!result.unwrap());
        } else {
            assert!(result.is_err());
        }
        assert_eq!(
            fs::read_to_string(root.path().join(design::PROJECT_FILE)).unwrap(),
            project
        );
    }
}

#[test]
fn setup_policy_confirmed_draft_preserves_bytes_and_remains_unactivated() {
    for project in [
        "# Keep this comment\nschema_version: 1\nname: Setup fixture\ndescription: Keep description.\n...",
        "# Keep this comment\r\nschema_version: 1\r\nname: Setup fixture\r\ndescription: Keep description.\r\n",
    ] {
        let (root, workspace) = fixture(project);
        let suggestion = suggest(&workspace).unwrap();
        let mut output = Vec::new();
        assert!(review_with_io(&workspace, &suggestion, true,
            &mut Cursor::new("yes\n"), &mut output).unwrap());
        let updated = fs::read_to_string(root.path().join(design::PROJECT_FILE)).unwrap();
        assert!(updated.contains("# Keep this comment"));
        let before: ProjectDesign = serde_yaml::from_str(project).unwrap();
        let after: ProjectDesign = serde_yaml::from_str(&updated).unwrap();
        assert_eq!(before.name, after.name);
        assert_eq!(before.description, after.description);
        assert!(after.acceptance_policy.is_some());
        if project.contains("\r\n") {
            assert_eq!(updated.matches("\r\n").count(), updated.matches('\n').count());
        }
        assert!(crate::verification::policy_store::load(&workspace, "setup-project").unwrap().is_none());
        assert!(!root.path().join("EXECUTED").exists());
        assert!(String::from_utf8(output).unwrap().contains("does not activate"));
    }
}

#[test]
fn setup_policy_stale_review_and_existing_fields_are_not_overwritten() {
    let (root, workspace) = fixture("schema_version: 1\nname: Setup fixture\n");
    let suggestion = suggest(&workspace).unwrap();
    let changed = "schema_version: 1\nname: Independently changed\n";
    fs::write(root.path().join(design::PROJECT_FILE), changed).unwrap();
    assert!(review_with_io(
        &workspace,
        &suggestion,
        true,
        &mut Cursor::new("yes\n"),
        &mut Vec::new()
    )
    .is_err());
    assert_eq!(
        fs::read_to_string(root.path().join(design::PROJECT_FILE)).unwrap(),
        changed
    );
    let with_null = "schema_version: 1\nname: Setup fixture\nacceptance_policy: null\n";
    let (root, workspace) = fixture(with_null);
    let suggestion = suggest(&workspace).unwrap();
    assert_eq!(suggestion.status, "existing_policy_field");
    assert!(suggestion.policy_yaml.is_none());
    assert!(!review_with_io(
        &workspace,
        &suggestion,
        true,
        &mut Cursor::new("yes\n"),
        &mut Vec::new()
    )
    .unwrap());
    assert_eq!(
        fs::read_to_string(root.path().join(design::PROJECT_FILE)).unwrap(),
        with_null
    );
}

#[test]
fn setup_policy_missing_invalid_or_unappendable_design_is_advisory_only() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("package.json"),
        r#"{"scripts":{"test":"node --test"}}"#,
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let missing = suggest(&workspace).unwrap();
    assert_eq!(missing.status, "design_required");
    assert!(missing.edit.is_none());
    assert!(!root.path().join(".wcode").exists());
    for project in [
        "name: [invalid",
        "{schema_version: 1, name: Flow project}",
        "schema_version: 1\r\nname: Mixed\n",
    ] {
        let (_root, workspace) = fixture(project);
        let suggestion = suggest(&workspace).unwrap();
        assert!(suggestion.edit.is_none());
        assert!(suggestion.policy_yaml.is_none());
    }
}

#[test]
fn setup_policy_remote_hint_never_exposes_urls_credentials_or_reads_protected_git() {
    assert_eq!(
        classify_remote("https://FAKE-CREDENTIAL@github.com/owner/repo?token=FAKE-TOKEN"),
        "github"
    );
    assert_eq!(classify_remote("git@github.com:owner/repo.git"), "github");
    assert_eq!(
        classify_remote("https://github.com@evil.invalid/owner/repo"),
        "other"
    );
    let (root, workspace) = fixture("schema_version: 1\nname: Setup fixture\n");
    fs::create_dir(root.path().join(".git")).unwrap();
    fs::write(
        root.path().join(".git/config"),
        "[remote \"origin\"]\nurl = https://FAKE-CREDENTIAL@github.com/owner/repo\n",
    )
    .unwrap();
    assert_eq!(
        provider_hint(&workspace),
        "unknown",
        "no-exec Setup respects the protected Git config boundary"
    );
    let serialized = serde_json::to_string(&suggest(&workspace).unwrap()).unwrap();
    assert!(!serialized.contains("FAKE-CREDENTIAL"));
    assert!(!serialized.contains("https://"));
}

#[test]
fn setup_policy_public_guide_links_protected_ui_without_approval_or_tokens() {
    let page = crate::setup_web::render("fixture", 1, 2, "fixture-nonce");
    assert!(page.contains("wcode setup --project --dry-run --json"));
    assert!(page.contains("href=\"/intelligence\""));
    assert!(page.contains("This public guide cannot approve requests or activate Policy."));
    assert!(!page.contains("x-wcode-ui-token"));
    assert!(!page.contains("#token="));
    assert!(!page.contains("/intelligence/authorizations"));
}
