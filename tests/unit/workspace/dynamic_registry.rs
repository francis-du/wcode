use super::*;

#[test]
fn dynamic_workspace_refresh_discovers_new_projects_without_restart() {
    let root = tempfile::tempdir().unwrap();
    let workspaces = Workspaces::new([root.path()], true, true).unwrap();
    let parent_id = workspaces.default_id().to_owned();
    let project = root.path().join("Rust/new-project");
    let project_id = format!("{parent_id}/Rust/new-project");
    assert!(workspaces.select(Some(&project_id)).is_err());

    fs::create_dir_all(&project).unwrap();
    fs::write(
        project.join("Cargo.toml"),
        "[package]\nname='new-project'\nversion='0.1.0'\n",
    )
    .unwrap();

    let (selected_id, selected) = workspaces.select(Some(&project_id)).unwrap();
    assert_eq!(selected_id, project_id);
    assert_eq!(selected.root(), project.canonicalize().unwrap());
    assert!(workspaces.capabilities()["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["id"] == selected_id));
}

#[test]
fn dynamic_workspace_refresh_tracks_marker_changes_and_rename() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("old-name");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("Cargo.toml"), "[workspace]\n").unwrap();
    let workspaces = Workspaces::new([root.path()], true, true).unwrap();
    let parent_id = workspaces.default_id().to_owned();
    let old_id = format!("{parent_id}/old-name");
    assert!(workspaces.select(Some(&old_id)).is_ok());

    fs::remove_file(project.join("Cargo.toml")).unwrap();
    assert!(workspaces.select(Some(&old_id)).is_err());

    fs::write(project.join("package.json"), "{\"name\":\"restored\"}\n").unwrap();
    assert!(workspaces.select(Some(&old_id)).is_ok());

    let renamed = root.path().join("new-name");
    fs::rename(&project, &renamed).unwrap();
    let new_id = format!("{parent_id}/new-name");
    assert!(workspaces.select(Some(&old_id)).is_err());
    assert_eq!(
        workspaces.select(Some(&new_id)).unwrap().1.root(),
        renamed.canonicalize().unwrap()
    );
}

#[test]
fn dynamic_workspace_refresh_revokes_removed_workspace_authorization() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("Cargo.toml"), "[workspace]\n").unwrap();
    let workspaces = Workspaces::new([root.path()], true, true).unwrap();
    let parent_id = workspaces.default_id().to_owned();
    let project_id = format!("{parent_id}/project");
    assert!(workspaces.select(Some(&project_id)).is_ok());

    workspaces
        .set_all_commands_authorized(Some(&project_id), true)
        .unwrap();
    assert!(workspaces
        .all_commands_authorized(Some(&project_id))
        .unwrap());

    fs::remove_file(project.join("Cargo.toml")).unwrap();
    assert!(workspaces.select(Some(&project_id)).is_err());

    fs::write(project.join("Cargo.toml"), "[workspace]\n").unwrap();
    assert!(workspaces.select(Some(&project_id)).is_ok());
    assert!(!workspaces
        .all_commands_authorized(Some(&project_id))
        .unwrap());
}

#[test]
fn dynamic_workspace_refresh_remains_bounded_and_does_not_promote_noise() {
    let root = tempfile::tempdir().unwrap();
    let workspaces = Workspaces::new([root.path()], false, false).unwrap();
    for index in 0..64 {
        let path = root.path().join(format!("noise/{index}/nested"));
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("README.txt"), "not a project\n").unwrap();
    }
    let project = root.path().join("projects/real");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("Cargo.toml"), "[workspace]\n").unwrap();

    let info = workspaces.capabilities();
    assert_eq!(info["subspace_discovery"]["dynamic_refresh_max_depth"], 3);
    let entries = info["workspaces"].as_array().unwrap();
    let project_root = project.canonicalize().unwrap();
    assert!(entries
        .iter()
        .any(|entry| { entry["root"].as_str() == Some(project_root.to_string_lossy().as_ref()) }));
    assert!(!entries.iter().any(|entry| {
        entry["root"]
            .as_str()
            .is_some_and(|root| root.contains("/noise/"))
    }));
}
