use super::*;

#[test]
fn source_scans_honor_ignore_files_and_allow_explicit_ignored_paths() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("generated/cache")).unwrap();
    fs::create_dir_all(dir.path().join("scratch/tmp")).unwrap();
    fs::create_dir_all(dir.path().join("target/debug")).unwrap();
    fs::write(dir.path().join(".gitignore"), "generated/\n").unwrap();
    fs::write(dir.path().join(".ignore"), "scratch/\n").unwrap();
    fs::write(dir.path().join("main.rs"), "fn visible_needle() {}\n").unwrap();
    fs::write(
        dir.path().join("generated/cache/hidden.rs"),
        "fn generated_hidden_needle() {}\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("scratch/tmp/hidden.rs"),
        "fn scratch_hidden_needle() {}\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("target/debug/hidden.rs"),
        "fn target_hidden_needle() {}\n",
    )
    .unwrap();

    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let (files, truncated) = workspace.source_files(".", 100).unwrap();
    assert!(!truncated);
    assert!(files.iter().any(|path| path == "main.rs"));
    assert!(!files.iter().any(|path| path.starts_with("generated/")));
    assert!(!files.iter().any(|path| path.starts_with("scratch/")));
    assert!(!files.iter().any(|path| path.starts_with("target/")));
    assert!(workspace
        .search("generated_hidden_needle", ".", 10)
        .unwrap()
        .is_empty());
    assert!(workspace
        .search("scratch_hidden_needle", ".", 10)
        .unwrap()
        .is_empty());
    assert!(workspace
        .search("target_hidden_needle", ".", 10)
        .unwrap()
        .is_empty());

    assert_eq!(
        workspace
            .search("generated_hidden_needle", "generated", 10)
            .unwrap()[0]["path"],
        "generated/cache/hidden.rs"
    );
    assert_eq!(
        workspace
            .search("target_hidden_needle", "target", 10)
            .unwrap()[0]["path"],
        "target/debug/hidden.rs"
    );
}

#[test]
fn list_files_prunes_ignored_roots_but_explicit_ignored_directories_remain_browsable() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("generated/nested")).unwrap();
    fs::create_dir_all(dir.path().join("target/debug")).unwrap();
    fs::write(dir.path().join(".gitignore"), "generated/\n").unwrap();
    fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();
    fs::write(
        dir.path().join("generated/nested/output.rs"),
        "fn generated() {}\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("target/debug/artifact.rs"),
        "fn artifact() {}\n",
    )
    .unwrap();

    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let root_files = workspace.list_files(".", 100).unwrap();
    assert!(root_files.contains(&"main.rs".to_owned()));
    assert!(!root_files.iter().any(|path| path.starts_with("generated/")));
    assert!(!root_files.iter().any(|path| path.starts_with("target/")));

    assert_eq!(
        workspace.list_files("generated", 100).unwrap(),
        vec!["generated/nested/output.rs"]
    );
    assert_eq!(
        workspace.list_files("target", 100).unwrap(),
        vec!["target/debug/artifact.rs"]
    );
}

#[test]
fn subspace_discovery_honors_gitignore_for_custom_generated_projects() {
    let root = tempfile::tempdir().unwrap();
    let visible = root.path().join("visible");
    fs::create_dir_all(&visible).unwrap();
    fs::write(visible.join("package.json"), "{\"name\":\"visible\"}\n").unwrap();

    fs::write(root.path().join(".gitignore"), "generated-projects/\n").unwrap();
    let ignored = root.path().join("generated-projects/noise");
    fs::create_dir_all(&ignored).unwrap();
    fs::write(
        ignored.join("package.json"),
        "{\"name\":\"ignored-noise\"}\n",
    )
    .unwrap();

    let workspaces = Workspaces::new([root.path()], false, false).unwrap();
    let parent_id = workspaces.default_id().to_owned();
    let entries = workspaces.capabilities()["workspaces"]
        .as_array()
        .unwrap()
        .clone();

    assert!(entries
        .iter()
        .any(|entry| entry["id"] == format!("{parent_id}/visible")));
    assert!(
        !entries.iter().any(|entry| entry["id"]
            .as_str()
            .is_some_and(|id| id.contains("generated-projects"))),
        "ignored subspace leaked into discovery: {entries:#?}"
    );
}

#[test]
fn authority_state_walkers_hide_state_and_keep_legitimate_wcode_projects() {
    let state =
        fs_safety::normalize_authority_root(&crate::evidence_store::state_root().unwrap()).unwrap();
    fs::create_dir_all(&state).unwrap();
    let hidden = tempfile::Builder::new()
        .prefix("hidden-fixture-")
        .tempdir_in(&state)
        .unwrap();
    let marker = format!("authority_hidden_{}", Uuid::new_v4().simple());
    fs::write(
        hidden.path().join("hidden.rs"),
        format!("fn {marker}() {{}}\n"),
    )
    .unwrap();
    let parent = state.parent().unwrap();
    let visible = tempfile::Builder::new()
        .prefix("visible-fixture-")
        .tempdir_in(parent)
        .unwrap();
    fs::create_dir(visible.path().join("wcode")).unwrap();
    fs::write(
        visible.path().join("wcode/main.rs"),
        "fn legitimate_project() {}\n",
    )
    .unwrap();
    let workspace = Workspace::new(parent, false, false).unwrap();
    let state_relative = portable_relative_path(state.strip_prefix(workspace.root()).unwrap());

    assert!(workspace.search(&marker, ".", 100).unwrap().is_empty());
    let files = workspace.list_files(".", 1000).unwrap();
    assert!(files
        .iter()
        .all(|path| !path.starts_with(&format!("{state_relative}/"))));
    let (sources, _) = workspace.source_files(".", 1000).unwrap();
    assert!(sources
        .iter()
        .all(|path| !path.starts_with(&format!("{state_relative}/"))));
    // The parent contains historical PID stores; its bounded results need not
    // reach this fixture. Check ordinary project visibility in its own subtree.
    let visible_relative =
        portable_relative_path(visible.path().strip_prefix(workspace.root()).unwrap());
    let expected = vec![format!("{visible_relative}/wcode/main.rs")];
    assert_eq!(
        workspace.list_files(&visible_relative, 10).unwrap(),
        expected
    );
    assert_eq!(
        workspace.source_files(&visible_relative, 10).unwrap().0,
        expected
    );
    assert_eq!(
        workspace
            .search("legitimate_project", &visible_relative, 10)
            .unwrap()[0]["path"],
        expected[0]
    );
    assert!(!workspace
        .bounded_directory_entries(".", 128)
        .unwrap()
        .contains(&state_relative));
    assert!(workspace.search(&marker, &state_relative, 100).is_err());
    assert!(workspace.list_files(&state_relative, 100).is_err());
}
