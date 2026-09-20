use super::*;

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
    assert!(profile.contains(workspace.to_str().unwrap()));
    assert!(profile.contains(scratch.to_str().unwrap()));
    assert!(profile.contains(workspace.join(".ssh").to_str().unwrap()));
    assert!(profile.contains(workspace.join(".env").to_str().unwrap()));
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
    assert!(profile.contains(workspace.join("packages/api/.env.local").to_str().unwrap()));
    assert!(profile.contains("deny file-read* file-write*"));
    assert!(!profile.contains("(allow network"));

    let args = linux_bwrap_prefix(&workspace, &cwd, &scratch)
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
