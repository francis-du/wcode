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
