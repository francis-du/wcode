use super::*;

#[test]
fn rust_toolchain_home_overrides_are_limited_to_rust_tools() {
    for program in ["node", "python3", "git", "unknown-agent-tool"] {
        assert!(
            rust_toolchain_home_overrides(Path::new(program)).is_empty(),
            "non-Rust command received toolchain home access: {program}"
        );
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if let Some(home) = home.and_then(|path| path.canonicalize().ok()) {
        let expected = [("CARGO_HOME", ".cargo"), ("RUSTUP_HOME", ".rustup")]
            .into_iter()
            .filter_map(|(key, child)| {
                let path = std::env::var_os(key)
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home.join(child));
                let path = path.canonicalize().ok()?;
                (path.is_dir() && path.starts_with(&home)).then_some((key, path))
            })
            .collect::<Vec<_>>();
        assert_eq!(rust_toolchain_home_overrides(Path::new("cargo")), expected);
        assert_eq!(rust_toolchain_home_overrides(Path::new("rustc")), expected);
    }
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn macos_sandbox_runs_installed_cargo_with_isolated_home() {
    let root = tempfile::tempdir().unwrap();
    let args = vec!["--version".to_owned()];
    let (mut command, _guard) =
        prepare_macos(root.path(), root.path(), Path::new("cargo"), &args).unwrap();
    let output = command.output().await.unwrap();
    assert!(
        output.status.success(),
        "sandboxed cargo could not use the installed read-only toolchain: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("cargo "));
}

#[test]
fn broad_command_classification_sandboxes_shell_unknown_and_policy_bypass_only() {
    assert!(!command_requires_sandbox(
        false,
        true,
        true,
        "sh",
        &["-c".into(), "echo hi".into()],
    ));
    assert!(command_requires_sandbox(
        true,
        true,
        true,
        "sh",
        &["-c".into(), "echo hi".into()],
    ));
    assert!(command_requires_sandbox(
        true,
        true,
        true,
        "unknown-agent-tool",
        &["--do-anything".into()],
    ));
    assert!(!command_requires_sandbox(
        true,
        true,
        true,
        "cargo",
        &["test".into()],
    ));
    assert!(!command_requires_sandbox(
        true,
        false,
        true,
        "cargo",
        &["--version".into()],
    ));
    assert!(!command_requires_sandbox(
        true,
        true,
        false,
        "cargo",
        &["fmt".into()],
    ));
    for git_args in [
        vec!["add".into(), "-A".into()],
        vec!["describe".into(), "--always".into()],
        vec!["tag".into()],
    ] {
        assert!(
            !command_requires_sandbox(true, false, false, "git", &git_args),
            "bounded Git must stay direct under Full Access: {git_args:?}"
        );
    }
    assert!(!command_requires_sandbox(
        true,
        false,
        false,
        "sh",
        &["-n".into(), "syntax.sh".into()],
    ));
}

#[test]
fn sandbox_launch_plan_is_workspace_write_network_denied_and_fail_closed() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("repo");
    let scratch = root.path().join("scratch");
    let cwd = workspace.join("src");
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::create_dir_all(&scratch).unwrap();
    std::fs::create_dir_all(workspace.join(".ssh")).unwrap();
    std::fs::create_dir_all(workspace.join(".git")).unwrap();
    std::fs::create_dir_all(workspace.join("packages/api/.git")).unwrap();
    std::fs::write(workspace.join("packages/api/.env.local"), "TOKEN=dummy\n").unwrap();
    std::fs::write(workspace.join(".env"), "SECRET=hidden\n").unwrap();

    let canonical_workspace = workspace.canonicalize().unwrap();
    let profile = macos_profile(&workspace, &scratch).unwrap();
    assert!(profile.contains("(deny default)"));
    assert!(profile.contains("(allow file-read*)"));
    assert!(profile.contains("(allow file-write*"));
    let sandbox_path = |path: &std::path::Path| path.to_string_lossy().replace('\\', "\\\\");
    assert!(profile.contains(&sandbox_path(&workspace)));
    assert!(profile.contains(&sandbox_path(&scratch)));
    assert!(profile.contains(&sandbox_path(&canonical_workspace.join(".ssh"))));
    assert!(profile.contains(&sandbox_path(&canonical_workspace.join(".env"))));
    assert!(!profile.contains(&format!(
        "deny file-read* file-write* (subpath \"{}\")",
        canonical_workspace.join(".git").to_string_lossy()
    )));
    assert!(!profile.contains(&format!(
        "deny file-read* file-write* (subpath \"{}\")",
        canonical_workspace
            .join("packages/api/.git")
            .to_string_lossy()
    )));
    assert!(profile.contains(&sandbox_path(
        &canonical_workspace.join("packages/api/.env.local")
    )));
    assert!(profile.contains("deny file-read* file-write*"));
    assert!(!profile.contains("(allow network"));

    let protected = sandbox_protected_paths_with_roots(&workspace, &[], None).unwrap();
    let args = linux_bwrap_prefix_from_paths(&workspace, &cwd, &scratch, &protected)
        .unwrap()
        .into_iter()
        .map(|value| value.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert!(args
        .windows(3)
        .any(|items| items == ["--ro-bind", "/", "/"]));
    assert!(args.windows(3).any(|items| {
        items[0] == "--bind"
            && items[1] == workspace.to_string_lossy()
            && items[2] == workspace.to_string_lossy()
    }));
    assert!(args.windows(3).any(|items| {
        items[0] == "--bind"
            && items[1] == scratch.to_string_lossy()
            && items[2] == scratch.to_string_lossy()
    }));
    assert!(args.contains(&"--unshare-net".to_owned()));
    assert!(args.windows(3).any(|items| {
        items[0] == "--ro-bind"
            && items[1] == scratch.join("empty").to_string_lossy()
            && items[2] == canonical_workspace.join(".ssh").to_string_lossy()
    }));
    assert!(args.windows(3).any(|items| {
        items[0] == "--ro-bind"
            && items[1] == "/dev/null"
            && items[2] == canonical_workspace.join(".env").to_string_lossy()
    }));
    assert!(!args.windows(3).any(|items| {
        items[0] == "--ro-bind" && items[2] == canonical_workspace.join(".git").to_string_lossy()
    }));
    assert!(!args.windows(3).any(|items| {
        items[0] == "--ro-bind"
            && items[2]
                == canonical_workspace
                    .join("packages/api/.git")
                    .to_string_lossy()
    }));
    assert!(args.windows(3).any(|items| {
        items[0] == "--ro-bind"
            && items[1] == "/dev/null"
            && items[2]
                == canonical_workspace
                    .join("packages/api/.env.local")
                    .to_string_lossy()
    }));
    assert!(args
        .windows(2)
        .any(|items| { items[0] == "--chdir" && items[1] == cwd.to_string_lossy() }));

    let status = status();
    assert!(status.broad_execution_requires_sandbox);
    assert!(status.approval_independent);
    assert!(status.fail_closed);
    assert_eq!(status.network, "denied");
    assert_eq!(
        status.filesystem,
        "host_read_only_workspace_write_protected_paths_denied"
    );
    assert_eq!(status.available, status.backend != "unavailable");
}

#[cfg(unix)]
#[test]
fn protected_symlink_targets_are_masked_and_hardlink_aliases_fail_closed() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("repo");
    let outside = root.path().join("outside");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("secret"), "secret").unwrap();
    symlink(outside.join("secret"), workspace.join(".env")).unwrap();

    let protected = sandbox_protected_paths(&workspace).unwrap();
    assert!(protected.contains(&outside.join("secret").canonicalize().unwrap()));

    std::fs::remove_file(workspace.join(".env")).unwrap();
    std::fs::write(workspace.join(".env"), "secret").unwrap();
    std::fs::hard_link(workspace.join(".env"), workspace.join("alias.txt")).unwrap();
    let error = sandbox_protected_paths(&workspace).unwrap_err().to_string();
    assert!(error.contains("multiple hard links"));
}

#[test]
fn broad_execution_requires_sandbox_only_for_unbounded_bypass() {
    assert!(!broad_execution_requires_sandbox(false, false));
    assert!(!broad_execution_requires_sandbox(false, true));
    assert!(!broad_execution_requires_sandbox(true, true));
    assert!(broad_execution_requires_sandbox(true, false));
}

fn assert_read_only_mask(args: &[OsString], source: &Path, target: &Path) {
    assert!(
        args.windows(3).any(|items| {
            items[0].as_os_str() == std::ffi::OsStr::new("--ro-bind")
                && items[1].as_os_str() == source.as_os_str()
                && items[2].as_os_str() == target.as_os_str()
        }),
        "missing read-only mask for {}",
        target.display()
    );
}

#[test]
fn authority_state_roots_are_masked_inside_and_outside_the_workspace() {
    let fixture = tempfile::tempdir().unwrap();
    let workspace = fixture.path().join("repo");
    let scratch = fixture.path().join("scratch");
    let inside = workspace.join("storage");
    let outside = fixture.path().join("external-state");
    let ordinary = workspace.join("wcode");
    for path in [
        &inside,
        &inside.join("intelligence"),
        &outside,
        &ordinary,
        &scratch.join("empty"),
    ] {
        fs::create_dir_all(path).unwrap();
    }
    fs::write(inside.join(".env"), "fixture").unwrap();
    #[cfg(unix)]
    fs::hard_link(inside.join(".env"), inside.join("alias")).unwrap();
    fs::write(ordinary.join("main.rs"), "fn main() {}").unwrap();

    let roots = vec![inside.clone(), inside.join("intelligence"), outside.clone()];
    let protected = sandbox_protected_paths_with_roots(&workspace, &roots, None).unwrap();
    let inside = inside.canonicalize().unwrap();
    let outside = outside.canonicalize().unwrap();
    assert_eq!(protected.len(), 2);
    assert!(protected.contains(&inside));
    assert!(protected.contains(&outside));
    assert!(!protected.contains(&inside.join("intelligence")));
    assert!(!protected.contains(&ordinary.canonicalize().unwrap()));

    let profile = macos_profile_from_paths(&workspace, &scratch, &protected).unwrap();
    for root in [&inside, &outside] {
        let escaped = sandbox_string(root).unwrap();
        assert!(profile.contains(&format!(
            "(deny file-read* file-write* (literal \"{escaped}\"))"
        )));
        assert!(profile.contains(&format!(
            "(deny file-read* file-write* (subpath \"{escaped}\"))"
        )));
    }
    let args = linux_bwrap_prefix_from_paths(&workspace, &workspace, &scratch, &protected).unwrap();
    for root in [&inside, &outside] {
        assert_read_only_mask(&args, &scratch.join("empty"), root);
    }
    assert!(!args.windows(3).any(|items| {
        items[0].as_os_str() == std::ffi::OsStr::new("--ro-bind")
            && items[2].as_os_str() == ordinary.canonicalize().unwrap().as_os_str()
    }));
}

#[test]
fn missing_authority_roots_are_denied_on_macos_and_fail_closed_on_linux() {
    let fixture = tempfile::tempdir().unwrap();
    let workspace = fixture.path().join("repo");
    let scratch = fixture.path().join("scratch");
    fs::create_dir_all(&workspace).unwrap();
    fs::create_dir_all(&scratch).unwrap();
    let roots = vec![
        workspace.join("future/state"),
        fixture.path().join("future/state"),
    ];
    let protected = sandbox_protected_paths_with_roots(&workspace, &roots, None).unwrap();
    assert_eq!(protected.len(), 2);
    let profile = macos_profile_from_paths(&workspace, &scratch, &protected).unwrap();
    for root in &protected {
        assert!(!root.exists());
        let escaped = sandbox_string(root).unwrap();
        assert!(profile.contains(&format!(
            "(deny file-read* file-write* (literal \"{escaped}\"))"
        )));
        assert!(profile.contains(&format!(
            "(deny file-read* file-write* (subpath \"{escaped}\"))"
        )));
    }
    let error = linux_bwrap_prefix_from_paths(&workspace, &workspace, &scratch, &protected)
        .unwrap_err()
        .to_string();
    assert!(error.contains("sandbox_unavailable: cannot mask protected path"));
    assert!(protected.iter().all(|root| !root.exists()));
}

#[test]
fn authority_state_workspace_selection_and_disappearing_roots_fail_closed() {
    let fixture = tempfile::tempdir().unwrap();
    let state = fixture.path().join("state");
    let workspace = fixture.path().join("repo");
    let child = state.join("subworkspace");
    let scratch = fixture.path().join("scratch");
    for path in [&child, &workspace, &scratch] {
        fs::create_dir_all(path).unwrap();
    }
    for selected in [&state, &child] {
        let error =
            sandbox_protected_paths_with_roots(selected, std::slice::from_ref(&state), None)
                .unwrap_err()
                .to_string();
        assert!(error.contains("Workspace is inside an authority-state root"));
    }
    let protected =
        sandbox_protected_paths_with_roots(&workspace, std::slice::from_ref(&state), None).unwrap();
    fs::remove_dir_all(&state).unwrap();
    let error = linux_bwrap_prefix_from_paths(&workspace, &workspace, &scratch, &protected)
        .unwrap_err()
        .to_string();
    assert!(error.contains("cannot mask protected path"));
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&workspace, &state).unwrap();
        let error = linux_bwrap_prefix_from_paths(&workspace, &workspace, &scratch, &protected)
            .unwrap_err()
            .to_string();
        assert!(error.contains("unsupported protected path"));
    }
}

#[test]
fn authority_state_roots_resolve_parent_aliases_before_masking() {
    let fixture = tempfile::tempdir().unwrap();
    let workspace = fixture.path().join("repo");
    let state = fixture.path().join("state");
    fs::create_dir_all(&workspace).unwrap();
    fs::create_dir_all(&state).unwrap();
    let aliases = vec![
        workspace.join("../state"),
        workspace.join("../future/state"),
    ];
    let protected = sandbox_protected_paths_with_roots(&workspace, &aliases, None).unwrap();
    assert!(protected.contains(&state.canonicalize().unwrap()));
    assert!(protected.contains(&fixture.path().canonicalize().unwrap().join("future/state")));
    assert!(protected.iter().all(|path| path.is_absolute()));
    assert!(protected.iter().all(|path| {
        !path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    }));
}
