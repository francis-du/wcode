use super::*;

#[path = "access.rs"]
mod access;

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
fn write_lock_registry_prunes_inactive_paths() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path(), true, false).unwrap();
    let first_path = dir.path().join("first.txt");
    let second_path = dir.path().join("second.txt");

    let first = workspace.write_lock_for(&first_path).unwrap();
    assert_eq!(workspace.write_locks.lock().unwrap().len(), 1);
    drop(first);

    let second = workspace.write_lock_for(&second_path).unwrap();
    let locks = workspace.write_locks.lock().unwrap();
    assert_eq!(locks.len(), 1);
    assert!(locks.contains_key(&second_path));
    drop(locks);
    drop(second);
}

#[test]
fn list_files_exposes_workspace_files_but_search_skips_noise_and_secrets() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join(".idea")).unwrap();
    fs::write(dir.path().join(".idea/workspace.xml"), "private IDE state").unwrap();
    fs::write(dir.path().join(".env"), "TOKEN=secret").unwrap();
    fs::write(dir.path().join("server.log"), "noise").unwrap();
    fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();

    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    assert_eq!(
        workspace.list_files(".", 100).unwrap(),
        vec![".idea/workspace.xml", "main.rs", "server.log"]
    );
    assert!(workspace.search("secret", ".", 100).unwrap().is_empty());
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
fn rejects_overlapping_workspaces_by_default() {
    let root = tempfile::tempdir().unwrap();
    let child = root.path().join("child");
    fs::create_dir(&child).unwrap();

    assert!(Workspaces::new([root.path(), child.as_path()], false, false).is_err());

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
fn discovers_nested_project_subspaces_without_relaxing_overlap_policy() {
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
    assert_eq!(info["security"]["overlapping_workspaces"], false);
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

    assert!(Workspaces::new([root.path(), nested.as_path()], false, false).is_err());
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
    assert!(separate.add_workspace(&child).is_err());
}

#[test]
fn blocks_protected_paths_and_destructive_replacements() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(".env"), "TOKEN=secret\n").unwrap();
    fs::write(dir.path().join(".env.example"), "TOKEN=example\n").unwrap();
    let original = "x".repeat(10_000);
    fs::write(dir.path().join("large.txt"), &original).unwrap();
    let workspace = Workspace::new(dir.path(), true, false).unwrap();

    assert!(workspace.read_file(".env", 1, None).is_err());
    assert!(workspace.read_file(".env.example", 1, None).is_ok());
    let view = workspace.read_file("large.txt", 1, None).unwrap();
    assert!(workspace
        .replace_text("large.txt", &original, "small", &view.sha256)
        .is_err());

    let security = WorkspaceSecurity {
        allow_destructive_writes: true,
        ..WorkspaceSecurity::default()
    };
    let permissive = Workspace::new_with_security(dir.path(), true, false, security).unwrap();
    permissive
        .replace_text("large.txt", &original, "small", &view.sha256)
        .unwrap();
    assert_eq!(
        fs::read_to_string(dir.path().join("large.txt")).unwrap(),
        "small"
    );
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
fn delete_path_requires_one_shot_human_authorization() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("obsolete.txt"), "remove me\n").unwrap();
    let workspace = Workspace::new(dir.path(), true, false).unwrap();
    let sha = workspace.path_info("obsolete.txt").unwrap().sha256.unwrap();

    let first = workspace
        .delete_path("obsolete.txt", Some(&sha))
        .unwrap_err();
    assert!(first.to_string().contains("authorization required"));
    let request = workspace.authorization.latest_pending().unwrap();
    assert_eq!(request.kind, AuthorizationKind::DestructiveDelete);
    assert!(workspace.authorization.approve_session(&request.id));
    workspace.delete_path("obsolete.txt", Some(&sha)).unwrap();
    assert!(!dir.path().join("obsolete.txt").exists());

    fs::write(dir.path().join("obsolete.txt"), "remove me\n").unwrap();
    assert!(workspace.delete_path("obsolete.txt", Some(&sha)).is_err());
    fs::create_dir(dir.path().join("nonempty")).unwrap();
    fs::write(dir.path().join("nonempty/file.txt"), "x").unwrap();
    assert!(workspace.delete_path("nonempty", None).is_err());
}

#[test]
fn command_policy_keeps_direct_checks_safe_and_repository_execution_exact() {
    let safe = WorkspaceSecurity::default();
    assert!(validate_command_policy("git", &["status".to_owned()], safe).is_ok());
    assert!(validate_command_policy("git", &["clean".to_owned(), "-fd".to_owned()], safe).is_err());
    assert!(validate_command_policy(
        "python3",
        &["-c".to_owned(), "print('unsafe')".to_owned()],
        safe,
    )
    .is_err());
    assert!(validate_command_policy("cargo", &["fmt".to_owned()], safe).is_err());
    assert!(
        validate_command_policy("cargo", &["fmt".to_owned(), "--check".to_owned()], safe,).is_ok()
    );
    assert!(validate_command_policy("cargo", &["check".to_owned()], safe).is_ok());
    assert!(
        validate_command_policy("cargo", &["check".to_owned(), "--locked".to_owned()], safe,)
            .is_ok()
    );
    assert!(validate_command_policy("cargo", &["test".to_owned()], safe).is_err());
    assert!(
        validate_command_policy("cargo", &["test".to_owned(), "--locked".to_owned()], safe,)
            .is_err()
    );
    assert!(validate_command_policy(
        "cargo",
        &[
            "clippy".to_owned(),
            "--locked".to_owned(),
            "--".to_owned(),
            "-D".to_owned(),
            "warnings".to_owned(),
        ],
        safe,
    )
    .is_ok());
    assert!(validate_command_policy(
        "cargo",
        &[
            "build".to_owned(),
            "--release".to_owned(),
            "--locked".to_owned()
        ],
        safe,
    )
    .is_err());
    assert!(validate_command_policy(
        "cargo",
        &["check".to_owned(), "--workspace".to_owned()],
        safe,
    )
    .is_err());
    assert!(validate_command_policy("cargo", &["metadata".to_owned()], safe).is_err());
    assert!(validate_command_policy("go", &["list".to_owned()], safe).is_err());
    assert!(validate_command_policy("npm", &["list".to_owned()], safe).is_err());
    assert!(
        validate_command_policy("rg", &["needle".to_owned(), "--hidden".to_owned()], safe,)
            .is_err()
    );
    for arguments in [
        vec!["-f".to_owned(), ".env".to_owned()],
        vec!["--file".to_owned(), ".env".to_owned()],
        vec!["--ignore-file".to_owned(), ".env".to_owned()],
        vec!["--glob".to_owned(), ".env".to_owned(), "needle".to_owned()],
        vec!["--type-add=secret:*.env".to_owned(), "needle".to_owned()],
    ] {
        assert!(
            validate_command_policy("rg", &arguments, safe).is_err(),
            "ripgrep helper/file-selection bypass was accepted: {arguments:?}"
        );
    }
    assert!(
        validate_command_policy("git", &["show".to_owned(), "HEAD:.env".to_owned()], safe,)
            .is_err()
    );
    assert!(validate_command_policy(
        "git",
        &["log".to_owned(), "--show-signature".to_owned()],
        safe,
    )
    .is_err());
    assert!(
        validate_command_policy("git", &["log".to_owned(), "--format=%G?".to_owned()], safe,)
            .is_err()
    );
    assert!(
        validate_command_policy("rg", &["TOKEN".to_owned(), ".env".to_owned()], safe,).is_err()
    );

    let trusted = WorkspaceSecurity {
        allow_risky_exec: true,
        ..WorkspaceSecurity::default()
    };
    assert!(validate_command_policy("cargo", &["test".to_owned()], trusted).is_ok());
    assert!(validate_command_policy("cargo", &["metadata".to_owned()], trusted).is_ok());
    assert!(validate_command_policy("go", &["list".to_owned()], trusted).is_ok());
    assert!(validate_command_policy("npm", &["list".to_owned()], trusted).is_ok());
    assert!(validate_command_policy(
        "cargo",
        &[
            "metadata".to_owned(),
            "--config".to_owned(),
            "build.rustc-wrapper=tool".to_owned(),
        ],
        trusted,
    )
    .is_err());
    assert!(validate_command_policy(
        "go",
        &["list".to_owned(), "-C".to_owned(), "subdir".to_owned()],
        trusted,
    )
    .is_err());
    assert!(validate_command_policy(
        "npm",
        &[
            "list".to_owned(),
            "--prefix".to_owned(),
            "subdir".to_owned()
        ],
        trusted,
    )
    .is_err());
    assert!(validate_command_policy("rustc", &["@args.txt".to_owned()], trusted).is_err());
}

#[test]
fn git_arguments_disable_repository_helpers_and_external_config_paths() {
    let status = hardened_command_args("git", &["status".to_owned(), "--short".to_owned()]);
    assert!(status
        .windows(2)
        .any(|pair| pair[0] == "-c" && pair[1] == "core.fsmonitor=false"));
    assert!(status.iter().any(|arg| arg.starts_with("core.hooksPath=")));
    assert!(status
        .iter()
        .any(|arg| arg.starts_with("core.attributesFile=")));
    assert!(status
        .iter()
        .any(|arg| arg.starts_with("core.excludesFile=")));
    for blocked_helper in [
        "credential.helper=",
        "core.askPass=",
        "core.sshCommand=false",
        "core.gitProxy=",
        "http.extraHeader=",
    ] {
        assert!(status.iter().any(|arg| arg == blocked_helper));
    }
    assert_eq!(
        status
            .get(status.len().saturating_sub(2))
            .map(String::as_str),
        Some("status")
    );
    assert_eq!(status.last().map(String::as_str), Some("--short"));

    let diff = hardened_command_args("git", &["diff".to_owned(), "--cached".to_owned()]);
    let subcommand = diff.iter().position(|arg| arg == "diff").unwrap();
    assert_eq!(diff[subcommand + 1], "--no-ext-diff");
    assert_eq!(diff[subcommand + 2], "--no-textconv");
    assert_eq!(diff.last().map(String::as_str), Some("--cached"));

    let push = hardened_command_args(
        "git",
        &[
            "push".to_owned(),
            "origin".to_owned(),
            "HEAD:main".to_owned(),
        ],
    );
    assert!(push.iter().any(|arg| {
        arg == "core.sshCommand=ssh -oBatchMode=yes -oStrictHostKeyChecking=accept-new"
    }));
    assert!(!push.iter().any(|arg| arg == "core.sshCommand=false"));
    assert!(push.iter().any(|arg| arg == "credential.helper="));

    let lfs_push = hardened_command_args(
        "git",
        &[
            "lfs".to_owned(),
            "push".to_owned(),
            "origin".to_owned(),
            "main".to_owned(),
        ],
    );
    assert!(lfs_push.iter().any(|arg| {
        arg == "core.sshCommand=ssh -oBatchMode=yes -oStrictHostKeyChecking=accept-new"
    }));
    assert!(!lfs_push.iter().any(|arg| arg == "core.sshCommand=false"));
}

#[test]
fn verification_policy_allows_only_inferred_quality_shapes() {
    assert!(validate_verification_command_shape(
        "cargo",
        &["check".to_owned(), "--locked".to_owned()],
    )
    .is_ok());
    assert!(validate_verification_command_shape(
        "cargo",
        &[
            "clippy".to_owned(),
            "--locked".to_owned(),
            "--".to_owned(),
            "-D".to_owned(),
            "warnings".to_owned(),
        ],
    )
    .is_ok());
    assert!(validate_verification_command_shape(
        "cargo",
        &[
            "build".to_owned(),
            "--release".to_owned(),
            "--locked".to_owned(),
        ],
    )
    .is_ok());
    assert!(validate_verification_command_shape(
        "cargo",
        &[
            "nextest".to_owned(),
            "run".to_owned(),
            "--locked".to_owned(),
        ],
    )
    .is_ok());
    assert!(validate_verification_command_shape(
        "cargo",
        &[
            "nextest".to_owned(),
            "run".to_owned(),
            "name(test)".to_owned(),
        ],
    )
    .is_err());
    assert!(validate_verification_command_shape(
        "pnpm",
        &["run".to_owned(), "typecheck".to_owned()],
    )
    .is_ok());
    assert!(
        validate_verification_command_shape("go", &["test".to_owned(), "./...".to_owned()],)
            .is_ok()
    );
    assert!(validate_verification_command_shape("cargo", &["run".to_owned()]).is_err());
    assert!(validate_verification_command_shape(
        "npm",
        &["run".to_owned(), "postinstall".to_owned()],
    )
    .is_err());
    assert!(validate_verification_command_shape(
        "python3",
        &["-c".to_owned(), "print('no')".to_owned()],
    )
    .is_err());
}

#[tokio::test]
async fn bounded_direct_and_harness_checks_run_without_risky_exec() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("src/lib.rs"),
        "pub fn value() -> u8 { 1 }\n",
    )
    .unwrap();
    let workspace = Workspace::new(dir.path(), false, true).unwrap();

    let direct = workspace
        .run_command("cargo", &["check".to_owned()], ".", 30)
        .await
        .expect("bounded direct cargo check should not require risky-exec");
    assert!(direct.success, "cargo check failed: {}", direct.stderr);

    let verified = workspace
        .run_verification_command("cargo", &["check".to_owned()], ".", 30)
        .await
        .expect("exact Harness verification shape may run without the global risky flag");
    assert!(verified.success, "cargo check failed: {}", verified.stderr);
}

#[tokio::test]
async fn trusted_runtime_executor_requires_explicit_repository_trust() {
    let dir = tempfile::tempdir().unwrap();
    let blocked = Workspace::new(dir.path(), false, true).unwrap();
    let error = blocked
        .run_trusted_runtime_command("rustc", &["--version".to_owned()], ".", 10)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("authorization required"));
    let request = blocked.authorization.latest_pending().unwrap();
    assert_eq!(request.kind, AuthorizationKind::RuntimeExecutor);
    assert!(blocked.authorization.approve_session(&request.id));
    let approved = blocked
        .run_trusted_runtime_command("rustc", &["--version".to_owned()], ".", 10)
        .await
        .unwrap();
    assert!(approved.success);

    let trusted = Workspace::new_with_security(
        dir.path(),
        false,
        true,
        WorkspaceSecurity {
            allow_risky_exec: true,
            ..WorkspaceSecurity::default()
        },
    )
    .unwrap();
    let result = trusted
        .run_trusted_runtime_command("rustc", &["--version".to_owned()], ".", 10)
        .await
        .unwrap();
    assert!(result.success);
}

#[tokio::test]
async fn revoked_catalog_command_becomes_selectively_authorizable() {
    let dir = tempfile::tempdir().unwrap();
    let workspaces = Workspaces::new([dir.path()], true, true).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    workspaces
        .revoke_command(Some(&workspace_id), "git")
        .unwrap();
    let (_, workspace) = workspaces.select(Some(&workspace_id)).unwrap();

    let error = workspace
        .run_command("git", &["status".to_owned()], ".", 10)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("authorization required"));
    let request = workspaces.latest_pending_authorization().unwrap();
    assert_eq!(request.kind, AuthorizationKind::CommandAccess);
    assert_eq!(request.workspace, workspace_id);
    assert_eq!(request.program.as_deref(), Some("git"));
    assert!(workspaces.approve_authorization_session(&request.id));
    assert!(workspace
        .allowed_commands()
        .iter()
        .any(|command| command == "git"));

    workspaces
        .revoke_command(Some(&workspace_id), "git")
        .unwrap();
    let denied = workspace
        .run_command("git", &["status".to_owned()], ".", 10)
        .await
        .unwrap_err();
    assert!(denied.to_string().contains("authorization required"));
    let request = workspaces.latest_pending_authorization().unwrap();
    assert!(workspaces.deny_authorization(&request.id));
    assert!(!workspace
        .allowed_commands()
        .iter()
        .any(|command| command == "git"));

    let arbitrary = workspace
        .run_command("hugo", &["--version".to_owned()], ".", 10)
        .await
        .unwrap_err();
    assert!(arbitrary.to_string().contains("authorization required"));
    let request = workspaces.latest_pending_authorization().unwrap();
    assert_eq!(request.kind, AuthorizationKind::CommandAccess);
    assert_eq!(request.program.as_deref(), Some("hugo"));
    assert!(workspaces.approve_authorization_session(&request.id));
    assert!(workspace
        .allowed_commands()
        .iter()
        .any(|command| command == "hugo"));

    let hard_denied = workspace
        .run_command("bash", &["-lc".to_owned(), "echo no".to_owned()], ".", 10)
        .await
        .unwrap_err();
    assert!(hard_denied.to_string().contains("no-shell"));
    assert!(workspaces.latest_pending_authorization().is_none());
}

#[test]
fn workspace_command_allowlist_supports_defaults_and_operator_authorized_programs() {
    let dir = tempfile::tempdir().unwrap();
    let workspaces = Workspaces::new([dir.path()], true, true).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    let (_, selected_before) = workspaces.select(Some(&workspace_id)).unwrap();
    assert!(selected_before
        .allowed_commands()
        .iter()
        .any(|command| command == "git"));

    let revoked = workspaces
        .revoke_command(Some(&workspace_id), "git")
        .unwrap();
    assert_eq!(revoked["changed"].as_bool(), Some(true));
    assert!(!selected_before
        .allowed_commands()
        .iter()
        .any(|command| command == "git"));
    assert!(selected_before
        .available_commands()
        .iter()
        .any(|command| command == "git"));

    let restored = workspaces
        .allow_command(Some(&workspace_id), "git")
        .unwrap();
    assert_eq!(restored["changed"].as_bool(), Some(true));
    assert!(selected_before
        .allowed_commands()
        .iter()
        .any(|command| command == "git"));
    assert!(workspaces
        .allow_command(Some(&workspace_id), "hugo")
        .is_ok());
    assert!(selected_before
        .allowed_commands()
        .iter()
        .any(|command| command == "hugo"));
    assert!(workspaces
        .allow_command(Some(&workspace_id), "bash")
        .is_err());
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
