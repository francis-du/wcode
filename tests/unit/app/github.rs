use super::*;
use clap::Parser;

fn publish(config: PathBuf) -> GitHubCommand {
    GitHubCommand::Publish {
        options: PublisherOptions {
            config_root: config,
            pull: 7,
            json: true,
        },
        workspace_id: "project".into(),
        base: "a".repeat(40),
        head: "b".repeat(40),
    }
}

#[test]
fn github_cli_requires_explicit_config_candidate_and_no_credential_flags() {
    for argv in [
        vec!["wcode", "github", "preflight", "--pull", "7"],
        vec![
            "wcode",
            "github",
            "preflight",
            "--config-root",
            ".",
            "--pull",
            "0",
        ],
        vec![
            "wcode",
            "github",
            "publish",
            "--config-root",
            ".",
            "--pull",
            "7",
        ],
        vec![
            "wcode",
            "github",
            "preflight",
            "--config-root",
            ".",
            "--pull",
            "7",
            "--token",
            "secret",
        ],
        vec![
            "wcode",
            "github",
            "preflight",
            "--config-root",
            ".",
            "--pull",
            "7",
            "--ready",
        ],
    ] {
        assert!(super::super::Args::try_parse_from(argv).is_err());
    }
    let args = super::super::Args::try_parse_from([
        "wcode",
        "github",
        "preflight",
        "--config-root",
        "operator-config",
        "--pull",
        "7",
        "--json",
        "--check",
    ])
    .unwrap();
    assert!(matches!(
        args.command,
        Some(super::super::ControlCommand::GitHub {
            action: GitHubCommand::Preflight { check: true, .. }
        })
    ));
    assert!(parse_sha("main").is_err());
    assert!(parse_sha("ABCDEF").is_err());
    assert_eq!(parse_sha(&"A".repeat(40)).unwrap(), "a".repeat(40));
}

#[test]
fn github_cli_safety_flags_reject_publication_before_paths_or_credentials() {
    for flag in ["--read-only", "--no-exec"] {
        let args = super::super::Args::try_parse_from(["wcode", flag]).unwrap();
        let error = candidate_root(&args, &publish(PathBuf::from("does-not-exist"))).unwrap_err();
        assert!(error.to_string().contains("disabled"));
    }
}

#[test]
fn github_cli_requires_disjoint_config_and_candidate_roots() {
    let root = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let args =
        super::super::Args::try_parse_from(["wcode", "-w", root.path().to_str().unwrap()]).unwrap();
    assert!(candidate_root(&args, &publish(root.path().to_path_buf())).is_err());
    std::fs::create_dir(root.path().join("nested-config")).unwrap();
    assert!(candidate_root(&args, &publish(root.path().join("nested-config"))).is_err());
    assert_eq!(
        candidate_root(&args, &publish(config.path().to_path_buf())).unwrap(),
        Some(root.path().canonicalize().unwrap())
    );
    #[cfg(unix)]
    {
        let alias = config.path().join("candidate-alias");
        std::os::unix::fs::symlink(root.path(), &alias).unwrap();
        assert!(candidate_root(&args, &publish(alias)).is_err());
    }
}

#[test]
fn github_cli_programmatic_invalid_identity_cannot_bypass_parser_checks() {
    let root = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let args =
        super::super::Args::try_parse_from(["wcode", "-w", root.path().to_str().unwrap()]).unwrap();
    let mut command = publish(config.path().to_path_buf());
    if let GitHubCommand::Publish { workspace_id, .. } = &mut command {
        *workspace_id = "\n".into();
    }
    assert!(candidate_root(&args, &command).is_err());
    let mut command = publish(config.path().to_path_buf());
    if let GitHubCommand::Publish { head, .. } = &mut command {
        *head = "refs/heads/main".into();
    }
    assert!(candidate_root(&args, &command).is_err());
}
