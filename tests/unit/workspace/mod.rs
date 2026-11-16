use super::*;

mod access;
mod command_policy;
mod dynamic_registry;
mod files;
mod ignore;
mod launch_profiles;
pub(super) mod process_fixture;
mod search;
#[test]
fn list_files_and_search_skip_repository_noise_but_allow_protected_paths() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join(".idea")).unwrap();
    fs::write(dir.path().join(".idea/workspace.xml"), "private IDE state").unwrap();
    fs::create_dir(dir.path().join("target")).unwrap();
    fs::write(dir.path().join("target/cache.bin"), "generated").unwrap();
    fs::create_dir(dir.path().join("generated")).unwrap();
    fs::write(dir.path().join("generated/output.rs"), "ignored").unwrap();
    fs::write(dir.path().join(".gitignore"), "generated/\n").unwrap();
    fs::write(dir.path().join(".env"), "TOKEN=secret").unwrap();
    fs::write(dir.path().join("server.log"), "noise").unwrap();
    fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();

    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    assert_eq!(
        workspace.list_files(".", 100).unwrap(),
        vec![".env", ".gitignore", "main.rs"]
    );
    assert_eq!(workspace.search("secret", ".", 100).unwrap().len(), 1);
}
#[test]
fn model_facing_paths_use_forward_slashes() {
    let dir = tempfile::tempdir().unwrap();
    let nested = dir.path().join("nested").join("deeper");
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("main.rs"), "fn needle() {}\n").unwrap();

    let workspace = Workspace::new(dir.path(), true, false).unwrap();
    let expected = "nested/deeper/main.rs";

    assert_eq!(workspace.list_files(".", 100).unwrap(), vec![expected]);
    assert_eq!(
        workspace.read_file(expected, 1, None).unwrap().path,
        expected
    );
    assert_eq!(
        workspace.search("needle", ".", 10).unwrap()[0]["path"],
        expected
    );
    let (source_files, truncated) = workspace.source_files(".", 100).unwrap();
    assert!(!truncated);
    assert_eq!(source_files, vec![expected]);

    let created = workspace
        .create_file("nested/deeper/created.rs", "fn created() {}\n")
        .unwrap();
    assert_eq!(created.path, "nested/deeper/created.rs");
}
#[test]
fn search_many_handles_overlapping_patterns_without_duplicate_line_matches() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("main.rs"),
        "alpha alphabet\nbeta alpha alpha\n",
    )
    .unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let queries = vec![
        "alpha".to_owned(),
        "alphabet".to_owned(),
        "missing".to_owned(),
    ];

    let results = workspace.search_many(&queries, ".", 100).unwrap();
    let observed = results
        .iter()
        .map(|value| {
            (
                value["line"].as_u64().unwrap(),
                value["query"].as_str().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(observed, vec![(1, "alpha"), (1, "alphabet"), (2, "alpha")]);
}
#[test]
fn accepts_overlapping_workspaces_by_default() {
    let root = tempfile::tempdir().unwrap();
    let child = root.path().join("child");
    fs::create_dir(&child).unwrap();

    assert!(Workspaces::new([root.path(), child.as_path()], false, false).is_ok());

    let security = WorkspaceSecurity {
        allow_overlapping_workspaces: true,
        ..WorkspaceSecurity::default()
    };
    assert!(
        Workspaces::new_with_security([root.path(), child.as_path()], false, false, security,)
            .is_ok()
    );
}
#[test]
fn discovers_nested_project_subspaces_with_permissive_overlap_policy() {
    let root = tempfile::tempdir().unwrap();
    let rust_repo = root.path().join("Rust/wcode");
    fs::create_dir_all(rust_repo.join(".git")).unwrap();
    fs::write(
        rust_repo.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    let nested_crate = rust_repo.join("crates/core");
    fs::create_dir_all(&nested_crate).unwrap();
    fs::write(
        nested_crate.join("Cargo.toml"),
        "[package]\nname = \"core\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();

    let web = root.path().join("Web");
    fs::create_dir_all(&web).unwrap();
    fs::write(web.join("package.json"), "{\"name\":\"web\"}\n").unwrap();
    let ignored = root.path().join("node_modules/noise");
    fs::create_dir_all(&ignored).unwrap();
    fs::write(ignored.join("package.json"), "{\"name\":\"noise\"}\n").unwrap();

    let workspaces = Workspaces::new([root.path()], false, false).unwrap();
    let parent_id = workspaces.default_id().to_owned();
    let rust_id = format!("{parent_id}/Rust/wcode");
    let web_id = format!("{parent_id}/Web");
    let info = workspaces.capabilities();
    assert_eq!(info["security"]["overlapping_workspaces"], true);
    assert_eq!(info["subspace_discovery"]["enabled"], true);
    let entries = info["workspaces"].as_array().unwrap();
    let rust_entry = entries.iter().find(|entry| entry["id"] == rust_id).unwrap();
    assert_eq!(rust_entry["kind"], "subspace");
    assert_eq!(rust_entry["parent_workspace"], parent_id);
    assert!(rust_entry["markers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|marker| marker == "git"));
    assert!(rust_entry["markers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|marker| marker == "cargo"));
    assert!(entries.iter().any(|entry| entry["id"] == web_id));
    assert!(!entries
        .iter()
        .any(|entry| entry["id"] == format!("{rust_id}/crates/core")));
    assert!(!entries.iter().any(|entry| entry["id"]
        .as_str()
        .is_some_and(|id| id.contains("node_modules"))));

    let (selected_id, selected) = workspaces.select(Some(&rust_id)).unwrap();
    assert_eq!(selected_id, rust_id);
    assert_eq!(selected.root(), rust_repo.canonicalize().unwrap());
    assert_eq!(workspaces.select(Some("Rust/wcode")).unwrap().0, rust_id);
}
#[test]
fn explicit_nested_workspace_authorization_reuses_parent_security_boundary() {
    let root = tempfile::tempdir().unwrap();
    let nested = root.path().join("scratch/project");
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("notes.txt"), "demo\n").unwrap();

    let workspaces = Workspaces::new([root.path()], true, true).unwrap();
    let parent_id = workspaces.default_id().to_owned();
    let (id, canonical) = workspaces
        .add_workspace_from(Some(&parent_id), "scratch/project")
        .unwrap();
    assert_eq!(id, format!("{parent_id}/scratch/project"));
    assert_eq!(canonical, nested.canonicalize().unwrap());
    assert_eq!(
        workspaces
            .add_workspace_from(Some(&parent_id), nested.to_str().unwrap())
            .unwrap(),
        (id.clone(), canonical.clone())
    );

    let access = workspaces.workspace_access(Some(&id)).unwrap();
    assert_eq!(access["root"], canonical.to_string_lossy().as_ref());
    let info = workspaces.capabilities();
    let entry = info["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == id)
        .unwrap();
    assert_eq!(entry["kind"], "subspace");
    assert_eq!(entry["parent_workspace"], parent_id);
    assert!(entry["markers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|marker| marker == "authorized"));

    assert!(Workspaces::new([root.path(), nested.as_path()], false, false).is_ok());
}

#[cfg(unix)]
#[test]
fn webui_derived_workspace_authorization_blocks_symlink_children() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let real = root.path().join("real");
    fs::create_dir(&real).unwrap();
    symlink(&real, root.path().join("alias")).unwrap();
    let workspaces = Workspaces::new([root.path()], false, false).unwrap();
    let parent_id = workspaces.default_id().to_owned();

    let error = workspaces
        .add_workspace_from(Some(&parent_id), "alias")
        .unwrap_err()
        .to_string();
    assert!(error.contains("symlink workspace paths are blocked"));
}
#[test]
fn webui_external_workspace_uses_normal_overlap_policy() {
    let root = tempfile::tempdir().unwrap();
    let parent = root.path().join("parent");
    let child = parent.join("child");
    let external = root.path().join("external");
    fs::create_dir_all(&child).unwrap();
    fs::create_dir(&external).unwrap();

    let workspaces = Workspaces::new([&parent], false, false).unwrap();
    let parent_id = workspaces.default_id().to_owned();
    let (external_id, canonical) = workspaces
        .add_workspace_from(Some(&parent_id), external.to_str().unwrap())
        .unwrap();
    assert_eq!(canonical, external.canonicalize().unwrap());
    assert_eq!(
        workspaces.capabilities()["workspaces"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["id"] == external_id)
            .unwrap()["kind"],
        "configured"
    );

    let separate = Workspaces::new([&parent], false, false).unwrap();
    let (child_id, child_root) = separate.add_workspace(&child).unwrap();
    assert_eq!(child_root, child.canonicalize().unwrap());
    assert_eq!(
        separate.workspace_access(Some(&child_id)).unwrap()["root"],
        child_root.to_string_lossy().as_ref()
    );
}

#[test]
fn allows_protected_paths_and_destructive_replacements_by_default() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(".env"), "TOKEN=secret\n").unwrap();
    fs::write(dir.path().join(".env.example"), "TOKEN=example\n").unwrap();
    let original = "x".repeat(10_000);
    fs::write(dir.path().join("large.txt"), &original).unwrap();
    let workspace = Workspace::new(dir.path(), true, false).unwrap();

    let env_view = workspace.read_file(".env", 1, None).unwrap();
    assert!(env_view.redacted);
    assert!(workspace.read_file(".env.example", 1, None).is_ok());
    let view = workspace.read_file("large.txt", 1, None).unwrap();
    workspace
        .replace_text("large.txt", &original, "small", &view.sha256)
        .unwrap();
    assert_eq!(
        fs::read_to_string(dir.path().join("large.txt")).unwrap(),
        "small"
    );

    let hardened = WorkspaceSecurity {
        allow_destructive_writes: false,
        ..WorkspaceSecurity::default()
    };
    let strict = Workspace::new_with_security(dir.path(), true, false, hardened).unwrap();
    fs::write(dir.path().join("large.txt"), &original).unwrap();
    let view = strict.read_file("large.txt", 1, None).unwrap();
    assert!(strict
        .replace_text("large.txt", &original, "small", &view.sha256)
        .is_err());
}

#[test]
fn coding_primitives_create_write_edit_move_and_batch_safely() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path(), true, false).unwrap();

    let created = workspace.create_directory("src/domain/models").unwrap();
    assert!(created.created);
    let initial = workspace
        .write_file("src/domain/models/user.rs", "alpha beta gamma\n", None)
        .unwrap();
    let _edited = workspace
        .apply_edits(
            "src/domain/models/user.rs",
            &[
                TextEdit {
                    old_text: "alpha".into(),
                    new_text: "ALPHA".into(),
                    start_line: None,
                    end_line: None,
                },
                TextEdit {
                    old_text: "gamma".into(),
                    new_text: "GAMMA".into(),
                    start_line: Some(1),
                    end_line: Some(1),
                },
            ],
            &initial.sha256_after,
        )
        .unwrap();
    assert_eq!(
        fs::read_to_string(dir.path().join("src/domain/models/user.rs")).unwrap(),
        "ALPHA beta GAMMA\n"
    );

    let source_sha = workspace
        .path_info("src/domain/models/user.rs")
        .unwrap()
        .sha256
        .unwrap();
    workspace
        .move_path_checked(
            "src/domain/models/user.rs",
            "src/domain/user.rs",
            Some(&source_sha),
        )
        .unwrap();

    workspace
        .create_files(&[
            CreateFileRequest {
                path: "src/domain/a.rs".into(),
                content: "a\n".into(),
            },
            CreateFileRequest {
                path: "src/domain/b.rs".into(),
                content: "b\n".into(),
            },
        ])
        .unwrap();
    let a = workspace.read_file("src/domain/a.rs", 1, None).unwrap();
    let b = workspace.read_file("src/domain/b.rs", 1, None).unwrap();
    let edited = workspace
        .apply_file_edits(&[
            FileEditRequest {
                path: "src/domain/a.rs".into(),
                expected_sha256: a.sha256,
                edits: vec![TextEdit {
                    old_text: "a".into(),
                    new_text: "A".into(),
                    start_line: Some(1),
                    end_line: Some(1),
                }],
            },
            FileEditRequest {
                path: "src/domain/b.rs".into(),
                expected_sha256: b.sha256,
                edits: vec![TextEdit {
                    old_text: "b".into(),
                    new_text: "B".into(),
                    start_line: None,
                    end_line: None,
                }],
            },
        ])
        .unwrap();
    assert!(edited.iter().all(|item| item.ok));
    let moved = workspace
        .move_paths(&[
            MovePathRequest {
                source: "src/domain/a.rs".into(),
                destination: "src/domain/a_model.rs".into(),
                expected_source_sha256: None,
            },
            MovePathRequest {
                source: "src/domain/b.rs".into(),
                destination: "src/domain/b_model.rs".into(),
                expected_source_sha256: None,
            },
        ])
        .unwrap();
    assert!(moved.iter().all(|item| item.ok));
}

#[test]
fn apply_edits_pin_original_lines_and_reject_overlap_or_stale_revision() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("demo.txt"), "same\nmiddle\nsame\n").unwrap();
    let workspace = Workspace::new(dir.path(), true, false).unwrap();
    let view = workspace.read_file("demo.txt", 1, None).unwrap();
    workspace
        .apply_edits(
            "demo.txt",
            &[TextEdit {
                old_text: "same".into(),
                new_text: "FIRST".into(),
                start_line: Some(1),
                end_line: Some(1),
            }],
            &view.sha256,
        )
        .unwrap();
    assert_eq!(
        fs::read_to_string(dir.path().join("demo.txt")).unwrap(),
        "FIRST\nmiddle\nsame\n"
    );

    let view = workspace.read_file("demo.txt", 1, None).unwrap();
    assert!(workspace
        .apply_edits(
            "demo.txt",
            &[
                TextEdit {
                    old_text: "FIRST\nmiddle".into(),
                    new_text: "x".into(),
                    start_line: Some(1),
                    end_line: Some(2),
                },
                TextEdit {
                    old_text: "middle\nsame".into(),
                    new_text: "y".into(),
                    start_line: Some(2),
                    end_line: Some(3),
                },
            ],
            &view.sha256,
        )
        .is_err());
    assert!(workspace
        .apply_edits(
            "demo.txt",
            &[TextEdit {
                old_text: "FIRST".into(),
                new_text: "x".into(),
                start_line: Some(1),
                end_line: Some(1),
            }],
            "stale",
        )
        .is_err());
}

#[test]
fn delete_path_succeeds_without_authorization_by_default() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("obsolete.txt"), "remove me\n").unwrap();
    let workspace = Workspace::new(dir.path(), true, false).unwrap();
    let sha = workspace.path_info("obsolete.txt").unwrap().sha256.unwrap();

    workspace.delete_path("obsolete.txt", Some(&sha)).unwrap();
    assert!(!dir.path().join("obsolete.txt").exists());
    assert!(workspace.authorization.latest_pending().is_none());

    fs::write(dir.path().join("obsolete.txt"), "changed content\n").unwrap();
    assert!(workspace.delete_path("obsolete.txt", Some(&sha)).is_err());
    fs::create_dir(dir.path().join("nonempty")).unwrap();
    fs::write(dir.path().join("nonempty/file.txt"), "x").unwrap();
    assert!(workspace.delete_path("nonempty", None).is_err());

    // Explicit hardening still requires a one-shot destructive grant.
    let hardened = WorkspaceSecurity {
        allow_destructive_writes: false,
        ..WorkspaceSecurity::default()
    };
    let strict = Workspace::new_with_security(dir.path(), true, false, hardened).unwrap();
    let sha = strict.path_info("obsolete.txt").unwrap().sha256.unwrap();
    let error = strict
        .delete_path("obsolete.txt", Some(&sha))
        .unwrap_err()
        .to_string();
    assert!(error.contains("authorization required"));
    let request = strict.authorization.latest_pending().unwrap();
    assert_eq!(request.kind, AuthorizationKind::DestructiveDelete);
    assert!(strict.authorization.approve_session(&request.id));
    strict.delete_path("obsolete.txt", Some(&sha)).unwrap();
}

#[test]
fn delete_path_waits_for_the_file_write_lock() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("locked.txt"), "A\n").unwrap();
    let workspace = Workspace::new(dir.path(), true, false).unwrap();
    let sha = workspace.path_info("locked.txt").unwrap().sha256.unwrap();
    let file = workspace.existing_path("locked.txt").unwrap();
    let lock = workspace.write_lock_for(&file).unwrap();
    let held = lock.lock().unwrap();

    let caller = workspace.clone();
    let deleting = std::thread::spawn(move || caller.delete_path("locked.txt", Some(&sha)));
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(
        dir.path().join("locked.txt").exists(),
        "delete_path removed the file while another writer held the write lock"
    );
    drop(held);
    deleting.join().unwrap().unwrap();
    assert!(!dir.path().join("locked.txt").exists());
}

#[cfg(unix)]
#[test]
fn rejects_workspace_root_replaced_at_same_path() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("workspace");
    let old_root = parent.path().join("workspace-old");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("before.txt"), "before\n").unwrap();
    let workspace = Workspace::new(&root, false, false).unwrap();

    fs::rename(&root, &old_root).unwrap();
    fs::create_dir(&root).unwrap();
    fs::write(root.join("after.txt"), "after\n").unwrap();

    assert!(workspace.read_file("after.txt", 1, None).is_err());
}

#[cfg(unix)]
#[test]
fn blocks_symlink_and_hardlink_aliases() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("real.txt"), "hello\n").unwrap();
    symlink("real.txt", dir.path().join("alias.txt")).unwrap();
    fs::hard_link(dir.path().join("real.txt"), dir.path().join("hard.txt")).unwrap();
    let workspace = Workspace::new(dir.path(), true, false).unwrap();

    assert!(workspace.read_file("alias.txt", 1, None).is_err());
    let view = workspace.read_file("real.txt", 1, None).unwrap();
    assert!(workspace
        .replace_text("real.txt", "hello", "hi", &view.sha256)
        .is_err());
}

#[test]
fn redacts_high_confidence_secrets_from_model_context() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("config.txt"),
        "endpoint=https://example.com\napi_key=super-secret-value\npassword = \"hunter2\"\n",
    )
    .unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let view = workspace.read_file("config.txt", 1, None).unwrap();
    assert!(view.redacted);
    assert!(view.content.contains("api_key= [REDACTED]"));
    assert!(view.content.contains("password = [REDACTED]"));
    assert!(!view.content.contains("super-secret-value"));
    assert!(!view.content.contains("hunter2"));
}
