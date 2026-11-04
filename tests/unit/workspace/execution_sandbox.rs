//! Broad-command tests use a private child process, never the developer's stores.
use super::*;

async fn run_in_private_fixture(name: &str) -> bool {
    crate::workspace::tests::process_fixture::run_in_private_fixture(module_path!(), name).await
}

#[tokio::test]
async fn sandbox_fixture_initializes_exact_private_roots() {
    if run_in_private_fixture("sandbox_fixture_initializes_exact_private_roots").await {
        return;
    }
    for root in [
        crate::core_types::authority_state_root().unwrap(),
        crate::core_types::intelligence_state_root().unwrap(),
    ] {
        assert!(
            root.is_dir(),
            "sandbox fixture must initialize every resolver root before launch"
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn workspace_all_command_grant_runs_directly_by_default() {
    if run_in_private_fixture("workspace_all_command_grant_runs_directly_by_default").await {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join(".env"), "SECRET=must-not-be-readable\n").unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let workspace_id = workspace.authorization_workspace_id();
    workspace
        .authorization
        .set_workspace_commands_granted(&workspace_id, true);

    // The default lane never selects the sandbox: the marker is absent, the
    // workspace credential stays readable and writes escape the Workspace.
    let command = "test \"$WCODE_SANDBOX\" = 1 && exit 41; cat .env >/dev/null 2>&1 || exit 42; printf unrestricted > unrestricted-command.txt; printf escape > ../sandbox-escape.txt";
    let result = workspace
        .run_command("sh", &["-c".into(), command.into()], ".", 10)
        .await
        .expect("broad command must run directly by default");
    assert!(result.success, "{}", result.stderr);
    assert_eq!(
        std::fs::read_to_string(root.path().join("unrestricted-command.txt")).unwrap(),
        "unrestricted"
    );
    let escape = root.path().parent().unwrap().join("sandbox-escape.txt");
    assert!(
        escape.exists(),
        "direct execution must not contain the command"
    );
    assert!(workspace.authorization.latest_pending().is_none());
}

#[cfg(unix)]
#[tokio::test]
async fn workspace_all_command_grant_sandboxes_broad_shell_when_requested() {
    if run_in_private_fixture("workspace_all_command_grant_sandboxes_broad_shell_when_requested")
        .await
    {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join(".env"), "SECRET=must-not-be-readable\n").unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let workspace_id = workspace.authorization_workspace_id();
    workspace
        .authorization
        .set_workspace_commands_granted(&workspace_id, true);

    let sandboxed = crate::workspace::execution_sandbox_status().available;
    let command = if sandboxed {
        "test \"$WCODE_SANDBOX\" = 1 || exit 41; if cat .env >/dev/null 2>&1; then exit 42; fi; printf unrestricted > unrestricted-command.txt; if printf escape > ../sandbox-escape.txt 2>/dev/null; then exit 43; else exit 0; fi"
    } else {
        "test \"$WCODE_SANDBOX\" = 1 && exit 41; cat .env >/dev/null 2>&1 || exit 42; printf unrestricted > unrestricted-command.txt; printf escape > ../sandbox-escape.txt"
    };
    let result = workspace
        .run_command_sandboxed("sh", &["-c".into(), command.into()], ".", 10)
        .await
        .expect("opt-in sandbox must run with or without a backend");
    assert!(result.success, "{}", result.stderr);
    assert_eq!(
        std::fs::read_to_string(root.path().join("unrestricted-command.txt")).unwrap(),
        "unrestricted"
    );
    let escape = root.path().parent().unwrap().join("sandbox-escape.txt");
    assert_eq!(escape.exists(), !sandboxed);
    assert!(workspace.authorization.latest_pending().is_none());
}

#[cfg(unix)]
#[tokio::test]
async fn full_access_keeps_bounded_commands_direct_and_runs_policy_bypass() {
    if run_in_private_fixture("full_access_keeps_bounded_commands_direct_and_runs_policy_bypass")
        .await
    {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new_with_security(
        root.path(),
        true,
        true,
        WorkspaceSecurity {
            allow_unrestricted_commands: true,
            ..WorkspaceSecurity::default()
        },
    )
    .unwrap();

    let bounded = workspace
        .run_command("cargo", &["--version".into()], ".", 30)
        .await
        .expect("bounded command must remain available without broad sandbox overhead");
    assert!(bounded.success, "{}", bounded.stderr);

    // Policy bypass is direct by default too; isolation requires an explicit
    // opt-in through run_command_sandboxed.
    let broad = workspace
        .run_command(
            "/bin/sh",
            &[
                "-c".into(),
                "printf full-access > full-access-command.txt".into(),
            ],
            ".",
            10,
        )
        .await
        .expect("policy bypass command must run with or without a sandbox backend");
    assert!(broad.success, "{}", broad.stderr);
    assert_eq!(
        std::fs::read_to_string(root.path().join("full-access-command.txt")).unwrap(),
        "full-access"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn full_access_opt_in_sandbox_isolates_broad_shell_when_backend_exists() {
    if run_in_private_fixture("full_access_opt_in_sandbox_isolates_broad_shell_when_backend_exists")
        .await
    {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new_with_security(
        root.path(),
        true,
        true,
        WorkspaceSecurity {
            allow_unrestricted_commands: true,
            ..WorkspaceSecurity::default()
        },
    )
    .unwrap();

    let sandboxed = crate::workspace::execution_sandbox_status().available;
    let command = if sandboxed {
        "test \"$WCODE_SANDBOX\" = 1 || exit 41; printf full-access-sandboxed > full-access-sandboxed.txt; if printf escape > ../full-access-sandbox-escape.txt 2>/dev/null; then exit 42; fi"
    } else {
        "test \"$WCODE_SANDBOX\" = 1 && exit 41; printf full-access-sandboxed > full-access-sandboxed.txt; printf escape > ../full-access-sandbox-escape.txt"
    };
    let result = workspace
        .run_command_sandboxed("/bin/sh", &["-c".into(), command.into()], ".", 10)
        .await
        .expect("opt-in sandbox must run with or without a backend");
    assert!(result.success, "{}", result.stderr);
    assert_eq!(
        std::fs::read_to_string(root.path().join("full-access-sandboxed.txt")).unwrap(),
        "full-access-sandboxed"
    );
    let escape = root
        .path()
        .parent()
        .unwrap()
        .join("full-access-sandbox-escape.txt");
    assert_eq!(escape.exists(), !sandboxed);
}

#[cfg(unix)]
#[tokio::test]
async fn revoking_workspace_all_command_grant_does_not_block_execution() {
    if run_in_private_fixture("revoking_workspace_all_command_grant_does_not_block_execution").await
    {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let workspace_id = workspace.authorization_workspace_id();
    workspace
        .authorization
        .set_workspace_commands_granted(&workspace_id, true);
    let broad = workspace
        .run_command("/bin/sh", &["-c".into(), "exit 0".into()], ".", 10)
        .await
        .expect("broad command runs");
    assert!(broad.success);

    workspace
        .authorization
        .set_workspace_commands_granted(&workspace_id, false);
    let after_revoke = workspace
        .run_command("/bin/sh", &["-c".into(), "exit 0".into()], ".", 10)
        .await
        .expect("revoking the session grant does not block the permissive default");
    assert!(after_revoke.success);
}
