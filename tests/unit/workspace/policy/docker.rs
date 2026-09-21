use super::*;

#[test]
fn compose_mutations_require_exact_risky_authorization_and_keep_task_lifecycle_attached() {
    let safe = WorkspaceSecurity::default();
    let trusted = WorkspaceSecurity {
        allow_risky_exec: true,
        ..WorkspaceSecurity::default()
    };

    for values in [
        vec!["build"],
        vec!["up"],
        vec![
            "up",
            "--no-build",
            "--pull",
            "never",
            "--abort-on-container-exit",
        ],
        vec!["start"],
        vec!["stop"],
        vec!["restart"],
        vec!["pull"],
        vec!["down"],
    ] {
        let command = args(&[values[0]]);
        assert!(
            validate_docker_command(
                &std::iter::once("compose".to_owned())
                    .chain(command.into_iter())
                    .collect::<Vec<_>>(),
                false,
            )
            .is_err(),
            "Compose mutation unexpectedly ran without exact authorization: {values:?}"
        );
        assert!(
            validate_docker_command(
                &std::iter::once("compose".to_owned())
                    .chain(values.iter().map(|value| (*value).to_owned()))
                    .collect::<Vec<_>>(),
                trusted.allow_risky_exec,
            )
            .is_ok(),
            "authorized bounded Compose mutation was rejected: {values:?}"
        );
    }

    for values in [
        vec!["compose", "up", "-d"],
        vec!["compose", "up", "--detach"],
        vec!["compose", "up", "--wait"],
        vec!["compose", "up", "--remove-orphans"],
        vec!["compose", "up", "--renew-anon-volumes"],
        vec!["compose", "up", "--scale", "web=100"],
    ] {
        assert!(
            validate_docker_command(&args(&values), trusted.allow_risky_exec).is_err(),
            "detached/destructive/unbounded Compose up shape unexpectedly accepted: {values:?}"
        );
    }

    assert!(!safe.allow_risky_exec);
}

#[test]
fn compose_blocks_scope_redirects_and_externally_consequential_build_flags() {
    for values in [
        vec!["compose", "-f", "other.yaml", "up"],
        vec!["compose", "up", "--file=other.yaml"],
        vec!["compose", "up", "--env-file", "other.env"],
        vec!["compose", "up", "--project-directory", "other"],
        vec!["compose", "up", "--project-name", "other"],
        vec!["compose", "up", "--profile", "debug"],
        vec!["compose", "build", "--push"],
        vec!["compose", "build", "--ssh=default"],
        vec!["compose", "build", "--builder", "remote"],
        vec!["compose", "build", "--build-arg", "TOKEN"],
    ] {
        assert!(
            validate_docker_command(&args(&values), true).is_err(),
            "Compose redirect/publish/credential expansion unexpectedly accepted: {values:?}"
        );
    }
}

#[test]
fn compose_inspection_does_not_render_interpolated_environment_values() {
    for values in [
        vec!["compose", "config", "--quiet"],
        vec!["compose", "config", "--services"],
        vec!["compose", "config", "--profiles"],
        vec!["compose", "ps", "--services"],
        vec![
            "compose",
            "ps",
            "--all",
            "--orphans=false",
            "--format",
            "{{.Service}}\t{{.State}}\t{{.Health}}\t{{.ExitCode}}",
        ],
        vec!["compose", "ls"],
    ] {
        assert!(
            validate_docker_command(&args(&values), false).is_ok(),
            "bounded Compose inspection unexpectedly rejected: {values:?}"
        );
    }

    for values in [
        vec!["compose", "config"],
        vec!["compose", "config", "--environment"],
        vec!["compose", "config", "--variables"],
        vec!["compose", "config", "--format", "json"],
        vec!["compose", "config", "--output", "rendered.yaml"],
        vec!["compose", "ps"],
        vec!["compose", "ps", "--format", "json"],
        vec!["compose", "ps", "--format", "{{json .}}"],
        vec!["compose", "ps", "--all", "--services"],
    ] {
        assert!(
            validate_docker_command(&args(&values), true).is_err(),
            "Compose inspection shape that may expose interpolated values or container commands unexpectedly accepted: {values:?}"
        );
    }
}
