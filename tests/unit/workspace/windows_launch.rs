use super::*;
use std::fs;

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn windows_verification_launcher_resolves_only_fixed_package_manager_checks() {
    let root = tempfile::tempdir().unwrap();
    for program in ["npm", "pnpm", "yarn"] {
        let launcher = root.path().join(format!("{program}.cmd"));
        fs::write(&launcher, "@exit /b 0\r\n").unwrap();
        for check in [
            "lint",
            "typecheck",
            "check",
            "format:check",
            "test",
            "build",
        ] {
            assert_eq!(
                find_verification_launcher(program, &args(&["run", check]), &[root.path().into()]),
                Some(launcher.clone())
            );
        }
    }
}

#[test]
fn windows_verification_launcher_never_adds_a_shell_for_unreviewed_arguments() {
    let root = tempfile::tempdir().unwrap();
    for program in ["npm", "pnpm", "yarn", "node", "custom"] {
        fs::write(root.path().join(format!("{program}.cmd")), "unused").unwrap();
    }
    for values in [
        vec![],
        vec!["install"],
        vec!["run", "custom"],
        vec!["run", "test", "--", "extra"],
        vec!["run", "test&whoami"],
        vec!["run", "%PRIVATE%"],
        vec!["run", "!PRIVATE!"],
        vec!["run", "test\r\nexit"],
        vec!["run", "test|exit"],
        vec!["run", "test>file"],
        vec!["run", "test", ""],
        vec!["run", "test", "--prefix=.."],
        vec!["run", "test", "&&", "whoami"],
        vec!["run", "test", "&", "whoami"],
        vec!["run", "test^&whoami"],
        vec!["run", "test;whoami"],
        vec!["run", "test\0"],
        vec!["run", "$(whoami)"],
        vec!["run", "`whoami`"],
    ] {
        for program in ["npm", "pnpm", "yarn"] {
            assert!(
                find_verification_launcher(program, &args(&values), &[root.path().into()])
                    .is_none()
            );
        }
    }
    for program in [
        "node",
        "custom",
        "npm.cmd",
        "npm.exe",
        "../npm",
        "bin/npm",
        "bin\\npm",
        "C:\\npm",
        "npm ",
        "npm&whoami",
        "npm\0",
        "NPM",
    ] {
        assert!(find_verification_launcher(
            program,
            &args(&["run", "test"]),
            &[root.path().into()]
        )
        .is_none());
    }
}

#[test]
fn windows_verification_launcher_preserves_native_executables_and_path_order() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let directories = [first.path().into(), second.path().into()];
    fs::write(second.path().join("npm.cmd"), "unused").unwrap();
    assert_eq!(
        find_verification_launcher("npm", &args(&["run", "test"]), &directories),
        Some(second.path().join("npm.cmd"))
    );
    fs::write(first.path().join("npm.cmd"), "unused").unwrap();
    assert_eq!(
        find_verification_launcher("npm", &args(&["run", "test"]), &directories),
        Some(first.path().join("npm.cmd"))
    );
    fs::write(second.path().join("npm.exe"), "native executable sentinel").unwrap();
    assert!(find_verification_launcher("npm", &args(&["run", "test"]), &directories).is_none());
}

#[test]
fn windows_verification_launcher_does_not_use_extensionless_scripts_or_relative_paths() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("npm"), "#!/bin/sh\nexit 0\n").unwrap();
    fs::create_dir(root.path().join("npm.cmd")).unwrap();
    assert!(
        find_verification_launcher("npm", &args(&["run", "test"]), &[root.path().into()]).is_none()
    );
    assert!(
        find_verification_launcher("npm", &args(&["run", "test"]), &[PathBuf::from(".")]).is_none()
    );
}

#[test]
fn windows_verification_launcher_skips_unusable_entries_and_other_script_extensions() {
    let root = tempfile::tempdir().unwrap();
    let tools = root.path().join("node tools with spaces");
    fs::create_dir(&tools).unwrap();
    for name in ["npm", "npm.bat", "npm.com", "npm.ps1"] {
        fs::write(tools.join(name), "not a supported launcher").unwrap();
    }
    fs::create_dir(tools.join("npm.exe")).unwrap();
    let directories = [
        PathBuf::new(),
        PathBuf::from("."),
        PathBuf::from("relative/bin"),
        root.path().join("missing"),
        tools.clone(),
    ];
    assert!(find_verification_launcher("npm", &args(&["run", "check"]), &[]).is_none());
    assert!(find_verification_launcher("npm", &args(&["run", "check"]), &directories).is_none());
    let launcher = tools.join("npm.cmd");
    fs::write(&launcher, "@exit /b 0\r\n").unwrap();
    assert_eq!(
        find_verification_launcher("npm", &args(&["run", "check"]), &directories),
        Some(launcher)
    );
}

#[cfg(windows)]
fn node_check_workspace(script: &str) -> (tempfile::TempDir, crate::workspace::Workspace) {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("node project with spaces");
    fs::create_dir(&project).unwrap();
    fs::write(project.join("check.js"), script).unwrap();
    fs::write(
        project.join("package.json"),
        r#"{"name":"windows-launch-check","version":"1.0.0","scripts":{"check":"node check.js"}}"#,
    )
    .unwrap();
    let workspace = crate::workspace::Workspace::new(root.path(), true, true).unwrap();
    (root, workspace)
}

#[cfg(windows)]
async fn run_node_check(
    workspace: &crate::workspace::Workspace,
    entry: &str,
) -> crate::workspace::CommandResult {
    let arguments = args(&["run", "check"]);
    let cwd = "node project with spaces";
    let result = match entry {
        "direct" => workspace.run_command("npm", &arguments, cwd, 60).await,
        "verification" => {
            workspace
                .run_verification_command("npm", &arguments, cwd, 60)
                .await
        }
        "runtime" => {
            workspace
                .run_trusted_runtime_command("npm", &arguments, cwd, 60)
                .await
        }
        _ => panic!("unknown test entry: {entry}"),
    };
    result.unwrap_or_else(|error| panic!("{entry} failed to launch npm: {error:#}"))
}

#[cfg(windows)]
#[tokio::test]
async fn windows_npm_verification_runs_real_node_checks_without_extra_authorization() {
    let (root, workspace) = node_check_workspace(
        "require('node:fs').writeFileSync('check-ran.txt', 'ok'); console.log('native-node-check');\n",
    );
    for entry in ["direct", "verification", "runtime"] {
        let result = run_node_check(&workspace, entry).await;
        assert!(result.success && !result.timed_out, "{entry}: {result:?}");
        assert_eq!(result.exit_code, Some(0), "{entry}: {result:?}");
        assert!(
            result.stdout.contains("native-node-check"),
            "{entry}: {result:?}"
        );
        assert_eq!(result.program, "npm");
        assert_eq!(result.args, args(&["run", "check"]));
        let marker = root.path().join("node project with spaces/check-ran.txt");
        assert_eq!(fs::read_to_string(&marker).unwrap(), "ok");
        fs::remove_file(marker).unwrap();
        assert!(workspace.authorization.latest_pending().is_none());
    }
}

#[cfg(windows)]
#[tokio::test]
async fn windows_npm_verification_preserves_failure_output_and_exit_status() {
    let (_root, workspace) = node_check_workspace(
        "console.log('node-check-started'); console.error('node-check-failed'); process.exitCode = 7;\n",
    );
    for entry in ["direct", "verification", "runtime"] {
        let result = run_node_check(&workspace, entry).await;
        assert!(!result.success && !result.timed_out, "{entry}: {result:?}");
        assert_eq!(result.exit_code, Some(7), "{entry}: {result:?}");
        assert!(
            result.stdout.contains("node-check-started"),
            "{entry}: {result:?}"
        );
        assert!(
            result.stderr.contains("node-check-failed"),
            "{entry}: {result:?}"
        );
        assert!(!result.output_incomplete, "{entry}: {result:?}");
        assert!(workspace.authorization.latest_pending().is_none());
    }
}
