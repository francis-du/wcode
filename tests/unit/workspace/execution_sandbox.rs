//! Broad-command tests use a private child process, never the developer's stores.
use super::*;

const FIXTURE_ROOT: &str = "WCODE_SANDBOX_TEST_ROOT";

async fn run_in_private_fixture(name: &str) -> bool {
    if let Some(root) = std::env::var_os(FIXTURE_ROOT) {
        let root = std::path::PathBuf::from(root);
        assert!(root.is_absolute() && root.is_dir());
        let authority = crate::core_types::authority_state_root().unwrap();
        let intelligence = crate::core_types::intelligence_state_root().unwrap();
        assert_eq!(authority, root.join("authority"));
        assert!(intelligence.starts_with(root.join("tmp")));
        // Materialize only the child-private resolver roots, before any sandbox
        // preflight; never rely on another parallel test having created stores.
        std::fs::create_dir_all(&authority).unwrap();
        std::fs::create_dir_all(&intelligence).unwrap();
        return false;
    }
    let original_state = std::env::var_os("WCODE_STATE_DIR");
    let original_temp = std::env::temp_dir();
    let root = tempfile::Builder::new()
        .prefix("wcode-sandbox-fixture-")
        .tempdir()
        .unwrap();
    let root_path = root.path().canonicalize().unwrap();
    let temporary = root_path.join("tmp");
    std::fs::create_dir(&temporary).unwrap();
    let module = module_path!().split_once("::").unwrap().1;
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            format!("{module}::{name}"),
            "--exact".into(),
            "--nocapture".into(),
        ])
        .env(FIXTURE_ROOT, &root_path)
        .env("WCODE_STATE_DIR", root_path.join("authority"))
        .env("TMPDIR", &temporary)
        .env("TMP", &temporary)
        .env("TEMP", &temporary)
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(60), command.output())
        .await
        .expect("private sandbox test must terminate")
        .unwrap();
    assert_eq!(std::env::var_os("WCODE_STATE_DIR"), original_state);
    assert_eq!(std::env::temp_dir(), original_temp);
    assert!(
        output.status.success(),
        "isolated sandbox test failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("running 1 test") && stdout.contains("1 passed; 0 failed"),
        "the exact child test must execute, not an empty filter: {stdout}"
    );
    true
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
