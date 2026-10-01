use super::*;

#[test]
fn blocks_path_traversal_and_stale_writes() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("demo.txt"), "hello world\n").unwrap();
    let workspace = Workspace::new(dir.path(), true, false).unwrap();
    assert!(workspace.read_file("../secret", 1, None).is_err());
    assert!(workspace
        .replace_text("demo.txt", "hello", "hi", "bad-hash")
        .is_err());
    let view = workspace.read_file("demo.txt", 1, None).unwrap();
    workspace
        .replace_text("demo.txt", "hello", "hi", &view.sha256)
        .unwrap();
    assert_eq!(
        fs::read_to_string(dir.path().join("demo.txt")).unwrap(),
        "hi world\n"
    );
}

#[test]
fn source_decomposition_policy_is_enforced_by_workspace_writes() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src/generated")).unwrap();
    let workspace = Workspace::new(dir.path(), true, false).unwrap();
    let oversized = "// filler\n".repeat(crate::conventions::OVERSIZED_SOURCE_LINES + 1);

    let create_error = workspace
        .create_file("src/new.rs", &oversized)
        .unwrap_err()
        .to_string();
    assert!(create_error.contains("core policy"));
    assert!(create_error.contains("hard limit"));

    let bounded = format!(
        "{}fn tail() {{}}\n",
        "// filler\n".repeat(crate::conventions::OVERSIZED_SOURCE_LINES - 1)
    );
    let initial = workspace.create_file("src/bounded.rs", &bounded).unwrap();
    let crossing = workspace
        .apply_edits(
            "src/bounded.rs",
            &[TextEdit {
                old_text: "fn tail() {}".into(),
                new_text: "fn tail() {}\nfn extra() {}".into(),
                start_line: None,
                end_line: None,
            }],
            &initial.sha256_after,
        )
        .unwrap_err()
        .to_string();
    assert!(crossing.contains("core policy"));

    let legacy = format!(
        "{}fn legacy_tail() {{}}\n",
        "// legacy filler\n".repeat(crate::conventions::OVERSIZED_SOURCE_LINES + 1)
    );
    fs::write(dir.path().join("src/legacy.rs"), &legacy).unwrap();
    let legacy_view = workspace.read_file("src/legacy.rs", 1, Some(1)).unwrap();
    let growth = workspace
        .apply_edits(
            "src/legacy.rs",
            &[TextEdit {
                old_text: "fn legacy_tail() {}".into(),
                new_text: "fn legacy_tail() {}\nfn more_behavior() {}".into(),
                start_line: None,
                end_line: None,
            }],
            &legacy_view.sha256,
        )
        .unwrap_err()
        .to_string();
    assert!(growth.contains("already") || growth.contains("blocks growing"));

    let legacy_view = workspace.read_file("src/legacy.rs", 1, Some(1)).unwrap();
    workspace
        .apply_edits(
            "src/legacy.rs",
            &[TextEdit {
                old_text: "// legacy filler\nfn legacy_tail() {}".into(),
                new_text: "fn legacy_tail() {}".into(),
                start_line: None,
                end_line: None,
            }],
            &legacy_view.sha256,
        )
        .unwrap();

    workspace
        .create_file("src/generated/client.rs", &oversized)
        .unwrap();
}

#[test]
fn write_lock_registry_reuses_live_locks_and_prunes_periodically() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path(), true, false).unwrap();
    let first_path = dir.path().join("first.txt");

    let first = workspace.write_lock_for(&first_path).unwrap();
    let same = workspace.write_lock_for(&first_path).unwrap();
    assert!(Arc::ptr_eq(&first, &same));
    assert_eq!(workspace.write_locks.lock().unwrap().len(), 1);
    drop((first, same));

    let mut last_path = PathBuf::new();
    for index in 0..128 {
        last_path = dir.path().join(format!("inactive-{index}.txt"));
        let lock = workspace.write_lock_for(&last_path).unwrap();
        drop(lock);
    }
    let locks = workspace.write_locks.lock().unwrap();
    assert_eq!(
        locks.len(),
        1,
        "expired weak locks should be pruned in batches"
    );
    assert!(locks.contains_key(&last_path));
}

#[test]
fn authority_state_file_tools_remain_blocked_after_full_home_access() {
    let state =
        fs_safety::normalize_authority_root(&crate::evidence_store::state_root().unwrap()).unwrap();
    fs::create_dir_all(&state).unwrap();
    let fixture = tempfile::Builder::new()
        .prefix("authority-fixture-")
        .tempdir_in(&state)
        .unwrap();
    let home = state.parent().unwrap();
    let project = tempfile::Builder::new()
        .prefix("project-")
        .tempdir_in(home)
        .unwrap();
    let workspaces = Workspaces::new([project.path()], true, true).unwrap();
    let (home_id, _) = workspaces.grant_full_user_access_at(home).unwrap();
    let (_, workspace) = workspaces.select(Some(&home_id)).unwrap();
    assert!(workspaces.all_commands_authorized(Some(&home_id)).unwrap());
    assert!(workspace.write_enabled());

    for domain in [
        "evidence",
        "verification",
        "reconciliation-approval",
        "engineering-journal",
        "oauth",
        "worklist",
        "mcp-tasks",
    ] {
        let directory = fixture.path().join(domain);
        fs::create_dir(&directory).unwrap();
        let record = directory.join("record.json");
        fs::write(&record, "trusted").unwrap();
        let path = portable_relative_path(record.strip_prefix(workspace.root()).unwrap());
        let new_path = portable_relative_path(
            directory
                .join("forged.json")
                .strip_prefix(workspace.root())
                .unwrap(),
        );
        let digest = sha256(b"trusted");
        let errors = [
            workspace.read_file(&path, 1, None).unwrap_err().to_string(),
            workspace.path_info(&path).unwrap_err().to_string(),
            workspace.read_media(&path).unwrap_err().to_string(),
            workspace
                .write_file(&path, "forged", Some(&digest))
                .unwrap_err()
                .to_string(),
            workspace
                .replace_text(&path, "trusted", "forged", &digest)
                .unwrap_err()
                .to_string(),
            workspace
                .apply_edits(
                    &path,
                    &[TextEdit {
                        old_text: "trusted".into(),
                        new_text: "forged".into(),
                        start_line: None,
                        end_line: None,
                    }],
                    &digest,
                )
                .unwrap_err()
                .to_string(),
            workspace
                .create_file(&new_path, "forged")
                .unwrap_err()
                .to_string(),
            workspace
                .create_directory(&format!("{new_path}/nested"))
                .unwrap_err()
                .to_string(),
            workspace
                .delete_path(&path, Some(&digest))
                .unwrap_err()
                .to_string(),
            workspace
                .move_path(&path, "moved.json")
                .unwrap_err()
                .to_string(),
        ];
        assert!(
            errors.iter().all(|error| error.contains("authority state")),
            "{domain}: {errors:?}"
        );
        assert_eq!(fs::read_to_string(record).unwrap(), "trusted");
        assert!(!directory.join("forged.json").exists());
    }
}

#[test]
fn authority_state_directory_moves_reject_ancestors_before_traversal() {
    let state =
        fs_safety::normalize_authority_root(&crate::evidence_store::state_root().unwrap()).unwrap();
    fs::create_dir_all(&state).unwrap();
    let parent = state.parent().unwrap();
    let workspace = Workspace::new(parent.parent().unwrap(), true, false).unwrap();
    let source = portable_relative_path(parent.strip_prefix(workspace.root()).unwrap());
    // A descendant destination also prevents any rename if the authority check
    // regresses: this test must never move the shared test-state parent.
    let destination = format!("{source}/move-attempt-{}", Uuid::new_v4());
    let error = workspace
        .move_path(&source, &destination)
        .unwrap_err()
        .to_string();
    assert!(error.contains("authority state"), "{error}");
    assert!(state.is_dir());
}
