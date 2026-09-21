use super::*;

pub(super) fn validate_docker_command(args: &[String], allow_risky_exec: bool) -> Result<()> {
    if args.iter().any(|arg| {
        matches!(arg.as_str(), "--context" | "--host" | "-H" | "--config")
            || arg.starts_with("--context=")
            || arg.starts_with("--host=")
            || arg.starts_with("--config=")
    }) {
        bail!("docker daemon/config redirection is blocked; use the operator-selected local Docker context");
    }
    let subcommand = args
        .first()
        .map(String::as_str)
        .ok_or_else(|| anyhow!("docker subcommand is required"))?;
    match subcommand {
        "info" | "ps" | "images" => Ok(()),
        "network" | "volume" if args.get(1).is_some_and(|action| action == "ls") => Ok(()),
        "compose" => validate_docker_compose(&args[1..], allow_risky_exec),
        _ => bail!("docker {subcommand} is blocked; wcode only exposes bounded inspection and Compose workflows"),
    }
}

fn validate_docker_compose(args: &[String], allow_risky_exec: bool) -> Result<()> {
    let action = args
        .first()
        .map(String::as_str)
        .ok_or_else(|| anyhow!("docker compose subcommand is required"))?;
    if action.starts_with('-') {
        bail!("docker compose global options before the subcommand are blocked; use the selected workspace, default Compose file, and operator-selected Docker context");
    }
    if compose_has_option(
        args,
        &[
            "-f",
            "--file",
            "--env-file",
            "--project-directory",
            "-p",
            "--project-name",
            "--profile",
            "--all-resources",
            "--compatibility",
        ],
    ) {
        bail!("docker compose file/environment/project/profile redirection is blocked; use repository defaults in the selected workspace");
    }

    let action_args = &args[1..];
    match action {
        "config" => {
            if action_args.is_empty()
                || !action_args.iter().all(|arg| {
                    matches!(arg.as_str(), "-q" | "--quiet" | "--services" | "--profiles")
                })
            {
                bail!("docker compose config rendering is blocked because interpolation may expose environment values; use --quiet, --services, or --profiles only");
            }
            Ok(())
        }
        "ps" => validate_docker_compose_ps(action_args),
        "ls" => Ok(()),
        "build" => {
            if compose_has_option(
                action_args,
                &["--push", "--ssh", "--builder", "--build-arg"],
            ) {
                bail!("docker compose build push/SSH/builder/build-arg expansion is blocked");
            }
            require_risky_exec("docker compose build", allow_risky_exec)
        }
        "up" => {
            if compose_has_option(
                action_args,
                &[
                    "-d",
                    "--detach",
                    "--wait",
                    "--remove-orphans",
                    "-V",
                    "--renew-anon-volumes",
                    "--scale",
                ],
            ) {
                bail!("docker compose up detach/orphan-removal/anonymous-volume-renewal/scale flags are blocked; keep the stack attached to the supervised task lifecycle");
            }
            require_risky_exec("docker compose up", allow_risky_exec)
        }
        "start" | "stop" | "restart" | "pull" => {
            require_risky_exec(&format!("docker compose {action}"), allow_risky_exec)
        }
        "down" => {
            if compose_has_option(
                action_args,
                &["-v", "--volumes", "--remove-orphans", "--rmi"],
            ) {
                bail!("docker compose down volume/image/orphan deletion flags are permanently blocked");
            }
            require_risky_exec("docker compose down", allow_risky_exec)
        }
        _ => bail!("docker compose {action} is blocked by the bounded Compose policy"),
    }
}

fn validate_docker_compose_ps(args: &[String]) -> Result<()> {
    const STATUS_FORMAT: &str = "{{.Service}}\t{{.State}}\t{{.Health}}\t{{.ExitCode}}";
    let safe_status = matches!(
        args,
        [all, orphans, format, template]
            if all == "--all"
                && orphans == "--orphans=false"
                && format == "--format"
                && template == STATUS_FORMAT
    );
    if args == ["--services"] || safe_status {
        Ok(())
    } else {
        bail!("docker compose ps output is restricted to service names or the bounded state/health status format so container commands are not exposed")
    }
}

fn compose_has_option(args: &[String], options: &[&str]) -> bool {
    args.iter().any(|arg| {
        options.iter().any(|option| {
            arg == option
                || option.starts_with("--")
                    && arg
                        .strip_prefix(option)
                        .is_some_and(|suffix| suffix.starts_with('='))
        })
    })
}

pub(super) fn validate_kubectl_command(args: &[String], allow_risky_exec: bool) -> Result<()> {
    if args.iter().any(|arg| {
        [
            "--token",
            "--username",
            "--password",
            "--client-key",
            "--client-certificate",
            "--certificate-authority",
            "--kubeconfig",
            "--context",
            "--cluster",
            "--user",
            "--server",
            "--as",
            "--as-group",
            "--as-uid",
            "--insecure-skip-tls-verify",
        ]
        .iter()
        .any(|flag| arg == flag || arg.starts_with(&format!("{flag}=")))
    }) {
        bail!("kubectl credential, impersonation, server, context, and kubeconfig overrides are blocked");
    }
    let subcommand = args
        .first()
        .map(String::as_str)
        .ok_or_else(|| anyhow!("kubectl subcommand is required"))?;
    match subcommand {
        "explain" | "api-resources" | "api-versions" => Ok(()),
        "get" | "describe" | "logs" | "events" | "cluster-info" => {
            require_risky_exec("kubectl cluster data inspection", allow_risky_exec)
        }
        "auth" if args.get(1).is_some_and(|action| action == "can-i") => {
            require_risky_exec("kubectl authorization inspection", allow_risky_exec)
        }
        "rollout" if args.get(1).is_some_and(|action| matches!(action.as_str(), "status" | "history")) => {
            require_risky_exec("kubectl rollout inspection", allow_risky_exec)
        }
        "diff" => require_risky_exec("kubectl server-side diff", allow_risky_exec),
        _ => bail!("kubectl {subcommand} is blocked; cluster mutations require a dedicated bounded policy rather than generic command access"),
    }
}

pub(super) fn validate_terraform_command(args: &[String], allow_risky_exec: bool) -> Result<()> {
    if args
        .iter()
        .any(|arg| arg == "-chdir" || arg.starts_with("-chdir="))
    {
        bail!("terraform -chdir is blocked; use the run_command cwd inside the selected workspace");
    }
    let subcommand = args
        .first()
        .map(String::as_str)
        .ok_or_else(|| anyhow!("terraform subcommand is required"))?;
    match subcommand {
        "validate" => Ok(()),
        "fmt" if args.iter().any(|arg| matches!(arg.as_str(), "-check" | "--check")) => Ok(()),
        "providers" | "graph" => Ok(()),
        "plan" => require_risky_exec("terraform plan provider/data-source execution", allow_risky_exec),
        "fmt" | "init" => require_risky_exec(&format!("terraform {subcommand}"), allow_risky_exec),
        "show" | "output" | "state" => bail!("terraform {subcommand} is blocked because state output can expose sensitive values to the model"),
        "apply" | "destroy" | "import" | "refresh" | "taint" | "untaint" | "force-unlock" | "login" | "logout" | "workspace" => {
            bail!("terraform {subcommand} is permanently blocked by the infrastructure mutation boundary")
        }
        _ => bail!("terraform subcommand is blocked by the bounded infrastructure policy: {subcommand}"),
    }
}
