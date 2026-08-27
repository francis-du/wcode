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
        "info" | "ps" | "images" => {
            require_risky_exec("Docker daemon inspection", allow_risky_exec)
        }
        "network" | "volume" if args.get(1).is_some_and(|action| action == "ls") => {
            require_risky_exec("Docker daemon inspection", allow_risky_exec)
        }
        "compose" => validate_docker_compose(&args[1..], allow_risky_exec),
        _ => bail!("docker {subcommand} is blocked; wcode only exposes bounded inspection and Compose workflows"),
    }
}

fn validate_docker_compose(args: &[String], allow_risky_exec: bool) -> Result<()> {
    let action = args
        .iter()
        .find(|arg| !arg.starts_with('-'))
        .map(String::as_str)
        .ok_or_else(|| anyhow!("docker compose subcommand is required"))?;
    match action {
        "config" | "ps" | "ls" => {
            require_risky_exec(&format!("docker compose {action}"), allow_risky_exec)
        }
        "build" | "up" | "start" | "stop" | "restart" | "pull" => {
            require_risky_exec(&format!("docker compose {action}"), allow_risky_exec)
        }
        "down" => {
            if args.iter().any(|arg| {
                matches!(
                    arg.as_str(),
                    "-v" | "--volumes" | "--remove-orphans" | "--rmi"
                ) || arg.starts_with("--rmi=")
            }) {
                bail!("docker compose down volume/image/orphan deletion flags are permanently blocked");
            }
            require_risky_exec("docker compose down", allow_risky_exec)
        }
        _ => bail!("docker compose {action} is blocked by the bounded Compose policy"),
    }
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
