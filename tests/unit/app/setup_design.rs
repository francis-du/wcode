use super::*;
use std::sync::{Arc, Barrier};

#[test]
fn setup_design_initializes_neutral_metadata_without_execution_or_approval() {
    let root = tempfile::tempdir().unwrap();
    let source = b"fn existing_business_logic() {}\n";
    fs::write(root.path().join("business.rs"), source).unwrap();
    let manifest = r#"{"scripts":{"test":"node -e \"require('fs').writeFileSync('executed-check','unexpected')\""}} "#;
    fs::write(root.path().join("package.json"), manifest).unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let report = initialize(&workspace, false).unwrap();
    assert_eq!(report.status, "initialized");
    assert!(!report.policy_activated);
    assert!(root.path().join(design::DESIGN_ROOT).is_dir());
    let load = design::load_design(&workspace).unwrap();
    assert!(load.initialized);
    assert_eq!(load.error_count(), 0);
    let metadata = load.state.project.unwrap();
    assert_eq!(
        metadata.name,
        root.path().file_name().unwrap().to_str().unwrap()
    );
    assert!(metadata.description.is_empty());
    assert!(metadata.acceptance_policy.is_none());
    assert!(load.state.product.is_some());
    assert_eq!(
        load.state.constraints.len(),
        design::baseline_constraints().len()
    );
    assert!(load.state.requirements.is_empty());
    assert!(load.state.components.is_empty());
    assert!(load.state.acceptance.is_empty());
    assert_eq!(fs::read(root.path().join("business.rs")).unwrap(), source);
    assert_eq!(
        fs::read_to_string(root.path().join("package.json")).unwrap(),
        manifest
    );
    for path in ["executed-check", "target"] {
        assert!(
            !root.path().join(path).exists(),
            "unexpected setup effect: {path}"
        );
    }
    assert!(root.path().join(".wcode/design/product.yaml").is_file());
    assert!(root.path().join(".wcode/design/constraints.yaml").is_file());
}

#[test]
fn setup_design_preview_and_existing_state_are_non_destructive() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let preview = initialize(&workspace, true).unwrap();
    assert_eq!(preview.status, "planned");
    assert_eq!(
        preview.planned_paths,
        [
            design::PROJECT_FILE,
            design::DESIGN_ROOT,
            ".wcode/design/product.yaml",
            ".wcode/design/constraints.yaml",
        ]
    );
    assert!(!preview.policy_activated);
    assert!(!root.path().join(".wcode").exists());
    initialize(&workspace, false).unwrap();
    let original = b"# preserve business intent\nschema_version: 1\nname: Established Project\ndescription: Existing ownership\n";
    fs::write(root.path().join(design::PROJECT_FILE), original).unwrap();
    let component = b"- schema_version: 1\n  id: component:billing\n  name: Billing\n  responsibilities: [Existing responsibility]\n";
    fs::write(root.path().join(".wcode/design/components.yaml"), component).unwrap();
    for dry_run in [true, false] {
        let report = initialize(&workspace, dry_run).unwrap();
        assert_eq!(report.status, "existing");
        assert!(!report.policy_activated);
        assert_eq!(
            fs::read(root.path().join(design::PROJECT_FILE)).unwrap(),
            original
        );
        assert_eq!(
            fs::read(root.path().join(".wcode/design/components.yaml")).unwrap(),
            component
        );
    }
}

#[test]
fn setup_design_rejects_readonly_and_aliases() {
    let root = tempfile::tempdir().unwrap();
    let readonly = Workspace::new(root.path(), false, false).unwrap();
    assert_eq!(initialize(&readonly, false).unwrap().status, "blocked");
    assert!(!root.path().join(".wcode").exists());
    #[cfg(unix)]
    {
        let external = tempfile::tempdir().unwrap();
        let sentinel = external.path().join("unchanged");
        fs::write(&sentinel, b"external data").unwrap();
        std::os::unix::fs::symlink(external.path(), root.path().join(".wcode")).unwrap();
        let writable = Workspace::new(root.path(), true, false).unwrap();
        assert_eq!(initialize(&writable, false).unwrap().status, "blocked");
        assert_eq!(fs::read(&sentinel).unwrap(), b"external data");
        assert!(!external.path().join("project.yaml").exists());
        fs::remove_file(root.path().join(".wcode")).unwrap();
        fs::create_dir_all(root.path().join(design::DESIGN_ROOT)).unwrap();
        fs::write(&sentinel, b"schema_version: 1\nname: Aliased\n").unwrap();
        fs::hard_link(&sentinel, root.path().join(design::PROJECT_FILE)).unwrap();
        assert_eq!(initialize(&writable, false).unwrap().status, "blocked");
        assert_eq!(
            fs::read(&sentinel).unwrap(),
            b"schema_version: 1\nname: Aliased\n"
        );
        assert!(!external.path().join("design").exists());
    }
}

#[test]
fn setup_design_partial_and_invalid_state_require_review_without_repair() {
    for (project, design_directory) in [
        (None, true),
        (Some(b"name: [invalid".as_slice()), true),
        (
            Some(b"schema_version: 1\nname: Existing\n".as_slice()),
            false,
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join(".wcode")).unwrap();
        if design_directory {
            fs::create_dir(root.path().join(design::DESIGN_ROOT)).unwrap();
        }
        if let Some(bytes) = project {
            fs::write(root.path().join(design::PROJECT_FILE), bytes).unwrap();
        }
        let workspace = Workspace::new(root.path(), true, false).unwrap();
        for dry_run in [true, false] {
            assert_eq!(
                initialize(&workspace, dry_run).unwrap().status,
                "needs_repair"
            );
            match project {
                Some(bytes) => assert_eq!(
                    fs::read(root.path().join(design::PROJECT_FILE)).unwrap(),
                    bytes
                ),
                None => assert!(!root.path().join(design::PROJECT_FILE).exists()),
            }
            assert_eq!(
                root.path().join(design::DESIGN_ROOT).exists(),
                design_directory
            );
        }
    }
}

#[test]
fn setup_design_concurrent_initialization_never_overwrites_metadata() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Arc::new(Workspace::new(root.path(), true, false).unwrap());
    let barrier = Arc::new(Barrier::new(8));
    let workers = (0..8)
        .map(|_| {
            let workspace = workspace.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                initialize(&workspace, false).unwrap().status
            })
        })
        .collect::<Vec<_>>();
    let statuses = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        statuses
            .iter()
            .filter(|status| **status == "initialized")
            .count(),
        1
    );
    assert_eq!(
        statuses
            .iter()
            .filter(|status| **status == "existing")
            .count(),
        7
    );
    let original = fs::read(root.path().join(design::PROJECT_FILE)).unwrap();
    assert_eq!(initialize(&workspace, false).unwrap().status, "existing");
    assert_eq!(
        fs::read(root.path().join(design::PROJECT_FILE)).unwrap(),
        original
    );
}

#[test]
fn setup_design_preserves_other_wcode_configuration_and_readonly_directory() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(".wcode")).unwrap();
    let configuration = b"repository-owned executor configuration\n";
    fs::write(root.path().join(".wcode/executors.yaml"), configuration).unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let directory = root.path().join(".wcode");
    let original_permissions = fs::metadata(&directory).unwrap().permissions();
    let mut readonly = original_permissions.clone();
    readonly.set_readonly(true);
    fs::set_permissions(&directory, readonly).unwrap();
    let blocked = initialize(&workspace, false).unwrap();
    fs::set_permissions(&directory, original_permissions).unwrap();
    assert_eq!(blocked.status, "blocked");
    assert!(!root.path().join(design::PROJECT_FILE).exists());
    assert_eq!(initialize(&workspace, false).unwrap().status, "initialized");
    assert_eq!(
        fs::read(root.path().join(".wcode/executors.yaml")).unwrap(),
        configuration
    );
}
