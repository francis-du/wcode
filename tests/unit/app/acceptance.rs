use super::*;
use clap::Parser;

fn parse(arguments: &[&str]) -> Result<super::super::Args, clap::Error> {
    super::super::Args::try_parse_from(arguments)
}

#[test]
fn acceptance_cli_requires_complete_candidate_shas_and_never_guesses_refs() {
    for value in [
        "HEAD", "main", "abc123", "../HEAD", "worktree", "1234\n", "--upload",
    ] {
        assert!(
            parse_sha(value).is_err(),
            "accepted ambiguous SHA {value:?}"
        );
    }
    assert_eq!(parse_sha(&"A".repeat(40)).unwrap(), "a".repeat(40));
    assert!(parse_sha(&"b".repeat(64)).is_ok());
    assert!(parse(&["wcode", "acceptance", "inspect", "--base", "HEAD"]).is_err());
    assert!(parse(&["wcode", "acceptance", "inspect"]).is_err());
}

#[test]
fn acceptance_cli_current_actions_and_history_have_distinct_parser_contracts() {
    let sha = "a".repeat(40);
    let valid = parse(&[
        "wcode",
        "acceptance",
        "inspect",
        "--base",
        &sha,
        "--head",
        &sha,
        "--check",
        "--json",
    ])
    .unwrap();
    assert!(matches!(
        valid.command,
        Some(super::super::ControlCommand::Acceptance {
            action: AcceptanceCommand::Inspect {
                check: true,
                json: true,
                ..
            }
        })
    ));
    for action in ["plan", "verify", "record"] {
        assert!(parse(&["wcode", "acceptance", action, "--base", &sha]).is_ok());
    }
    assert!(parse(&["wcode", "acceptance", "history", "--json"]).is_ok());
    assert!(parse(&["wcode", "acceptance", "history", "--base", &sha]).is_err());
    assert!(parse(&["wcode", "acceptance", "history", "--check"]).is_err());
    assert!(parse(&["wcode", "acceptance", "record", "--base", &sha, "--check"]).is_err());
    for timeout in ["0", "1801"] {
        assert!(parse(&[
            "wcode",
            "acceptance",
            "verify",
            "--base",
            &sha,
            "--timeout-seconds",
            timeout
        ])
        .is_err());
    }
}

#[test]
fn acceptance_cli_readonly_and_execution_guards_apply_before_native_operations() {
    let root = tempfile::tempdir().unwrap();
    let readonly = Workspace::new(root.path(), false, true).unwrap();
    let no_exec = Workspace::new(root.path(), true, false).unwrap();
    let base = "a".repeat(40);
    let commands = [
        AcceptanceCommand::Plan {
            base: base.clone(),
            head: None,
            json: true,
        },
        AcceptanceCommand::Verify {
            base: base.clone(),
            head: None,
            timeout_seconds: 1,
            check: true,
            json: true,
        },
        AcceptanceCommand::Record {
            base: base.clone(),
            head: None,
            json: true,
        },
    ];
    for command in &commands {
        assert!(command.validate_permissions(&readonly).is_err());
        assert!(command.validate_permissions(&no_exec).is_err());
    }
    let inspect = AcceptanceCommand::Inspect {
        base,
        head: None,
        check: false,
        json: true,
    };
    assert!(inspect.validate_permissions(&readonly).is_ok());
    assert!(inspect.validate_permissions(&no_exec).is_err());
    assert!(AcceptanceCommand::History { json: true }
        .validate_permissions(&no_exec)
        .is_ok());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn acceptance_cli_programmatic_arguments_preserve_validation_and_worktree_identity() {
    let invalid = AcceptanceCommand::Inspect {
        base: "main".into(),
        head: None,
        check: false,
        json: false,
    };
    assert!(invalid.candidate().is_err());
    let valid = AcceptanceCommand::Inspect {
        base: "a".repeat(40),
        head: None,
        check: false,
        json: false,
    };
    assert!(matches!(
        valid.candidate().unwrap().unwrap().1,
        GitChangeTarget::Worktree
    ));
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let invalid = AcceptanceCommand::Verify {
        base: "a".repeat(40),
        head: None,
        timeout_seconds: 1801,
        check: false,
        json: false,
    };
    assert!(invalid.validate_permissions(&workspace).is_err());
}

#[test]
fn acceptance_cli_history_never_becomes_current_gate_authority() {
    let historical = serde_json::json!({
        "workspace":"fixture", "authority":"historical_only", "current_acceptance":false,
        "retained":1, "capacity":256,
        "records":[{"id":"CAR-old", "state":"ready", "revision":{"code":"old-revision"}}],
    });
    let mut output = Vec::new();
    write_history(&mut output, &historical, false).unwrap();
    let text = String::from_utf8(output).unwrap();
    assert!(text.contains("historical only, not current gate authority"));
    assert!(text.contains("Historical state: ready"));
    let mut forged = historical;
    forged["current_acceptance"] = Value::Bool(true);
    assert!(write_history(&mut Vec::new(), &forged, true).is_err());
}
