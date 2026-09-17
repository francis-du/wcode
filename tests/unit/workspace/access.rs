use super::*;
use std::fs;

#[test]
fn full_access_capacity_failure_preserves_registry() {
    let root = tempfile::tempdir().unwrap();
    let mut paths = Vec::new();
    for index in 0..MAX_WORKSPACES {
        let path = root.path().join(format!("project-{index}"));
        fs::create_dir(&path).unwrap();
        paths.push(path);
    }
    let workspaces = Workspaces::new(&paths, false, false).unwrap();
    let before = workspaces.capabilities();
    let error = workspaces
        .grant_full_user_access_at(root.path())
        .unwrap_err();
    assert!(error.to_string().contains("at most"));
    assert_eq!(workspaces.capabilities(), before);
    assert!(!workspaces.full_access_enabled());
}

#[test]
fn full_access_elevates_existing_roots_and_keeps_hard_path_boundaries() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    fs::create_dir(&project).unwrap();
    let workspaces = Workspaces::new([&project], true, true).unwrap();

    let (home_id, home) = workspaces.grant_full_user_access_at(dir.path()).unwrap();
    assert_eq!(home, dir.path().canonicalize().unwrap());
    assert!(workspaces.full_access_enabled());
    assert!(workspaces.select(Some(&home_id)).is_ok());
    let capabilities = workspaces.capabilities();
    assert_eq!(capabilities["security"]["full_access"], true);
    assert_eq!(capabilities["security"]["unrestricted_commands"], true);
    assert_eq!(capabilities["security"]["user_home_workspace"], true);
    assert!(workspaces.all_commands_authorized(None).unwrap());
    assert_eq!(
        workspaces.workspace_access(None).unwrap()["all_commands_authorized"],
        true
    );
    assert_eq!(capabilities["security"]["broad_workspace_roots"], false);
    assert!(capabilities["security"]["full_access_scope"]
        .as_str()
        .unwrap()
        .contains("filesystem root"));
    assert!(workspaces
        .select(Some(&home_id))
        .unwrap()
        .1
        .read_file(".ssh/config", 1, None)
        .is_err());
}

#[test]
fn full_access_preparation_failures_leave_policy_unchanged() {
    for failure in ["missing-home", "file-home", "missing-workspace"] {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first");
        let second = root.path().join("second");
        fs::create_dir(&first).unwrap();
        fs::create_dir(&second).unwrap();
        let workspaces = Workspaces::new([&first, &second], false, false).unwrap();
        let before = workspaces.capabilities();
        let home = match failure {
            "missing-home" => root.path().join("absent"),
            "file-home" => {
                let file = root.path().join("not-a-directory");
                fs::write(&file, "untouched").unwrap();
                file
            }
            _ => {
                fs::remove_dir(&second).unwrap();
                root.path().to_path_buf()
            }
        };
        assert!(workspaces.grant_full_user_access_at(&home).is_err());
        assert_eq!(workspaces.capabilities(), before, "failure case: {failure}");
    }
}

#[test]
fn full_access_preserves_shared_locks_and_command_revocations() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    fs::create_dir(&project).unwrap();
    let workspaces = Workspaces::new([&project], false, false).unwrap();
    workspaces.revoke_command(None, "cargo").unwrap();
    let (_, before) = workspaces.select(None).unwrap();
    let (home_id, _) = workspaces.grant_full_user_access_at(root.path()).unwrap();
    let (_, after) = workspaces.select(None).unwrap();
    assert!(!after.command_allowed("cargo"));
    assert!(Arc::ptr_eq(&before.commands, &after.commands));
    assert!(Arc::ptr_eq(&before.write_locks, &after.write_locks));
    assert_eq!(before.root_identity, after.root_identity);
    assert!(!before.write_enabled());
    assert!(after.write_enabled());
    assert_eq!(
        workspaces.grant_full_user_access_at(root.path()).unwrap().0,
        home_id
    );
    assert_eq!(workspaces.roots().len(), 2);
    assert!(!workspaces.select(None).unwrap().1.command_allowed("cargo"));
}

#[test]
fn full_access_at_capacity_reuses_registered_home() {
    let root = tempfile::tempdir().unwrap();
    let mut paths = Vec::new();
    for index in 0..MAX_WORKSPACES {
        let path = root.path().join(format!("project-{index}"));
        fs::create_dir(&path).unwrap();
        paths.push(path);
    }
    let workspaces = Workspaces::new(&paths, false, false).unwrap();
    let id = workspaces.default_id().to_owned();
    assert_eq!(
        workspaces.grant_full_user_access_at(&paths[0]).unwrap().0,
        id
    );
    assert_eq!(workspaces.roots().len(), MAX_WORKSPACES);
    assert!(workspaces.full_access_enabled());
    for (id, _) in workspaces.roots() {
        let (_, workspace) = workspaces.select(Some(&id)).unwrap();
        assert!(workspace.write_enabled() && workspace.exec_enabled());
    }
}

#[test]
fn full_access_and_workspace_addition_publish_consistent_policy() {
    for _ in 0..8 {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first");
        let second = root.path().join("second");
        fs::create_dir(&first).unwrap();
        fs::create_dir(&second).unwrap();
        let workspaces = Workspaces::new([&first], false, false).unwrap();
        let barrier = Arc::new(std::sync::Barrier::new(2));
        std::thread::scope(|scope| {
            let workspaces = &workspaces;
            let barrier = &barrier;
            let home = root.path();
            let second = &second;
            let upgrade = scope.spawn(move || {
                barrier.wait();
                workspaces.grant_full_user_access_at(home).unwrap();
            });
            let add = scope.spawn(move || {
                barrier.wait();
                workspaces.add_workspace(second).unwrap();
            });
            upgrade.join().unwrap();
            add.join().unwrap();
        });
        assert!(workspaces.full_access_enabled());
        assert_eq!(workspaces.roots().len(), 3);
        for (id, _) in workspaces.roots() {
            let (_, workspace) = workspaces.select(Some(&id)).unwrap();
            assert!(workspace.write_enabled() && workspace.exec_enabled());
            assert!(workspace.risky_exec_enabled());
        }
    }
}

#[test]
fn model_reads_preserve_source_and_cap_each_request_at_one_thousand_lines() {
    let dir = tempfile::tempdir().unwrap();
    let source = (1..=1_200)
        .map(|line| format!("    line_{line}"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(dir.path().join("source.txt"), &source).unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();

    let default_view = workspace.read_file("source.txt", 1, None).unwrap();
    assert_eq!(default_view.start_line, 1);
    assert_eq!(default_view.end_line, 1_000);
    assert_eq!(default_view.content.lines().count(), 1_000);
    assert!(default_view.content.starts_with("    line_1\n    line_2"));

    let oversized = workspace.read_file("source.txt", 101, Some(1_200)).unwrap();
    assert_eq!(oversized.start_line, 101);
    assert_eq!(oversized.end_line, 1_100);
    assert_eq!(oversized.content.lines().count(), 1_000);
}
