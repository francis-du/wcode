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
async fn workspace_all_command_grant_sandboxes_broad_shell_or_fails_closed() {
    if run_in_private_fixture("workspace_all_command_grant_sandboxes_broad_shell_or_fails_closed")
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

    let outcome = workspace
        .run_command(
            "sh",
            &[
                "-c".into(),
                "test \"$WCODE_SANDBOX\" = 1 || exit 41; if cat .env >/dev/null 2>&1; then exit 42; fi; printf unrestricted > unrestricted-command.txt; if printf escape > ../sandbox-escape.txt 2>/dev/null; then exit 43; else exit 0; fi".into(),
            ],
            ".",
            10,
        )
        .await;
    if crate::workspace::execution_sandbox_status().available {
        let result = outcome.expect("available strong sandbox must launch broad command");
        assert!(result.success, "{}", result.stderr);
        assert_eq!(
            std::fs::read_to_string(root.path().join("unrestricted-command.txt")).unwrap(),
            "unrestricted"
        );
        assert!(!root
            .path()
            .parent()
            .unwrap()
            .join("sandbox-escape.txt")
            .exists());
    } else {
        let error = outcome.unwrap_err().to_string();
        assert!(error.contains("sandbox_unavailable"));
    }
    assert!(workspace.authorization.latest_pending().is_none());
}

#[cfg(unix)]
#[tokio::test]
async fn full_access_keeps_bounded_commands_direct_but_sandboxes_policy_bypass() {
    if run_in_private_fixture(
        "full_access_keeps_bounded_commands_direct_but_sandboxes_policy_bypass",
    )
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
        .await;
    if crate::workspace::execution_sandbox_status().available {
        let result = broad.expect("available strong sandbox must launch policy bypass");
        assert!(result.success, "{}", result.stderr);
        assert_eq!(
            std::fs::read_to_string(root.path().join("full-access-command.txt")).unwrap(),
            "full-access"
        );
    } else {
        assert!(broad.unwrap_err().to_string().contains("sandbox"));
    }
}

#[cfg(unix)]
#[tokio::test]
async fn revoking_workspace_all_command_grant_restores_normal_policy() {
    if run_in_private_fixture("revoking_workspace_all_command_grant_restores_normal_policy").await {
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
        .await;
    if crate::workspace::execution_sandbox_status().available {
        assert!(broad.unwrap().success);
    } else {
        assert!(broad.unwrap_err().to_string().contains("sandbox"));
    }

    workspace
        .authorization
        .set_workspace_commands_granted(&workspace_id, false);
    assert!(workspace
        .run_command("/bin/sh", &["-c".into(), "exit 0".into()], ".", 10)
        .await
        .is_err());
}
