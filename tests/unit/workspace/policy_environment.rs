use super::*;

#[test]
fn team_secret_environment_does_not_reach_child_processes() {
    const MARKER: &str = "WCODE_TEST_INHERITANCE_STAGE";
    const TEST: &str = "team_secret_environment_does_not_reach_child_processes";
    let protected = [
        "WCODE_TEAM_CREDENTIAL",
        "WCODE_TEAM_CREDENTIAL_FILE",
        "WCODE_GITHUB_APP_PRIVATE_KEY",
        "WCODE_GITHUB_APP_PRIVATE_KEY_FILE",
        "WCODE_GITHUB_APP_KEY_FILE",
    ];
    match std::env::var(MARKER).as_deref() {
        Ok("observe") => {
            for name in protected {
                assert!(
                    std::env::var_os(name).is_none(),
                    "protected key inherited: {name}"
                );
            }
            assert_eq!(std::env::var("WCODE_TEST_ORDINARY").unwrap(), "ordinary");
        }
        Ok("scrub") => {
            for program in ["git", "node", "gh"] {
                let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
                command
                    .args([TEST, "--nocapture", "--test-threads=1"])
                    .env(MARKER, "observe");
                scrub_sensitive_environment(&mut command, program, &[], false);
                let output = command.as_std_mut().output().unwrap();
                assert!(
                    output.status.success(),
                    "child inherited a protected key for {program}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }
        _ => {
            // Test-owned subprocess environment only; no process-global mutation
            // races with other tests or alters the operator's actual credentials.
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command
                .args([TEST, "--nocapture", "--test-threads=1"])
                .env(MARKER, "scrub")
                .env("WCODE_TEST_ORDINARY", "ordinary");
            for name in protected {
                command.env(name, "ISOLATED-TEST-NOT-LIVE");
            }
            let output = command.output().unwrap();
            assert!(
                output.status.success(),
                "sensitive environment test failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}

#[test]
fn python_execution_uses_one_isolated_nonwriting_bytecode_namespace() {
    let mut prefixes = Vec::new();
    for program in ["python3", "pytest"] {
        let mut command = tokio::process::Command::new(program);
        scrub_sensitive_environment(&mut command, program, &[], false);
        let env = command
            .as_std()
            .get_envs()
            .map(|(key, value)| {
                (
                    key.to_string_lossy().into_owned(),
                    value.map(|value| value.to_string_lossy().into_owned()),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>();

        assert_eq!(
            env.get("PYTHONDONTWRITEBYTECODE")
                .and_then(|value| value.as_deref()),
            Some("1")
        );
        let prefix = env
            .get("PYTHONPYCACHEPREFIX")
            .and_then(|value| value.as_deref())
            .expect("Python execution must isolate bytecode cache lookup");
        assert!(prefix.contains("wcode-python-cache-"));
        prefixes.push(prefix.to_owned());
    }
    assert_eq!(prefixes[0], prefixes[1]);
}

#[test]
fn managed_launch_environment_rejects_privileged_ports_and_untrusted_keys_without_echoing_them() {
    for port in ["0", "1", "22", "80", "1023", "65536"] {
        assert!(
            validate_command_environment(&[("PORT".to_owned(), port.to_owned())]).is_err(),
            "unexpectedly accepted privileged or invalid port {port}"
        );
    }
    for port in ["1024", "4317", "65535"] {
        assert!(
            validate_command_environment(&[("PORT".to_owned(), port.to_owned())]).is_ok(),
            "unexpectedly rejected bounded launch port {port}"
        );
    }

    let error = validate_command_environment(&[("INJECT\nHEADER".to_owned(), "x".repeat(129))])
        .unwrap_err()
        .to_string();
    assert!(!error.contains("INJECT"));
    assert!(error.contains("[REDACTED]"));
}

#[test]
fn managed_launch_environment_clears_inherited_managed_keys_before_applying_overrides() {
    let mut command = tokio::process::Command::new("node");
    command
        .env("HOST", "0.0.0.0")
        .env("PORT", "22")
        .env("NODE_ENV", "production")
        .env("RUST_LOG", "crate=trace")
        .env("LOG_LEVEL", "verbose");

    let environment = validate_command_environment(&[
        ("HOST".to_owned(), "127.0.0.1".to_owned()),
        ("PORT".to_owned(), "4317".to_owned()),
    ])
    .unwrap();
    apply_command_environment(&mut command, &environment);

    let env = command
        .as_std()
        .get_envs()
        .map(|(key, value)| {
            (
                key.to_string_lossy().into_owned(),
                value.map(|value| value.to_string_lossy().into_owned()),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(
        env.get("HOST").and_then(|value| value.as_deref()),
        Some("127.0.0.1")
    );
    assert_eq!(
        env.get("PORT").and_then(|value| value.as_deref()),
        Some("4317")
    );
    for key in ["NODE_ENV", "RUST_LOG", "LOG_LEVEL"] {
        assert_eq!(
            env.get(key),
            Some(&None),
            "{key} must not leak from the host"
        );
    }
}

#[test]
fn command_environment_injection_detection_covers_common_runtime_and_loader_hooks() {
    for key in [
        "LD_PRELOAD",
        "LD_LIBRARY_PATH",
        "DYLD_INSERT_LIBRARIES",
        "NODE_OPTIONS",
        "NODE_PATH",
        "PYTHONPATH",
        "PYTHONHOME",
        "RUBYOPT",
        "PERL5OPT",
        "LUA_INIT",
        "PHPRC",
        "JAVA_TOOL_OPTIONS",
        "DOTNET_STARTUP_HOOKS",
        "RUSTC_WRAPPER",
        "RUSTFLAGS",
    ] {
        assert!(
            command_environment_injection_key(key),
            "missed execution-injection environment key {key}"
        );
    }
    for key in ["PATH", "HOME", "HOST", "PORT", "NODE_ENV", "NO_COLOR"] {
        assert!(
            !command_environment_injection_key(key),
            "over-broadly classified ordinary environment key {key}"
        );
    }
}

#[test]
fn managed_launch_environment_accepts_only_typed_non_secret_values() {
    let accepted = validate_command_environment(&[
        ("RUST_LOG".to_owned(), "debug".to_owned()),
        ("PORT".to_owned(), "3000".to_owned()),
        ("HOST".to_owned(), "127.0.0.1".to_owned()),
        ("NODE_ENV".to_owned(), "development".to_owned()),
        ("LOG_LEVEL".to_owned(), "info".to_owned()),
    ])
    .unwrap();
    assert_eq!(accepted[0], ("HOST".to_owned(), "127.0.0.1".to_owned()));
    assert_eq!(accepted[4], ("RUST_LOG".to_owned(), "debug".to_owned()));

    for (key, value) in [
        ("PATH", "/tmp/bin"),
        ("AWS_SECRET_ACCESS_KEY", "secret"),
        ("PORT", "0"),
        ("PORT", "70000"),
        ("HOST", "0.0.0.0"),
        ("HOST", "localhost"),
        ("NODE_ENV", "production"),
        ("RUST_LOG", "crate=trace"),
        ("LOG_LEVEL", "info\nTOKEN=value"),
    ] {
        assert!(
            validate_command_environment(&[(key.to_owned(), value.to_owned())]).is_err(),
            "unexpectedly accepted {key}={value:?}"
        );
    }
    assert!(validate_command_environment(&[
        ("PORT".to_owned(), "3000".to_owned()),
        ("PORT".to_owned(), "4000".to_owned()),
    ])
    .is_err());
}
