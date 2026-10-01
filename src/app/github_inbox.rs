//! Local durable webhook operations for a fixed trusted publisher installation.
use super::*;
use crate::git_provider::github::GitHubInbox;
use anyhow::ensure;
use serde_json::json;

#[path = "github_worker.rs"]
pub(super) mod worker;

pub(super) fn emit_worker_event(event: serde_json::Value) -> Result<()> {
    use std::io::Write;
    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    serde_json::to_writer(&mut output, &event)?;
    writeln!(output)?;
    output.flush()?;
    Ok(())
}

pub(super) async fn run(args: &super::super::Args, command: &GitHubCommand) -> Result<()> {
    if !matches!(
        command,
        GitHubCommand::InboxStatus { .. } | GitHubCommand::InspectInboxArchive { .. }
    ) && !(args.allow_write || args.full_access)
    {
        bail!("GitHub inbox mutation is disabled by read-only mode");
    }
    if let GitHubCommand::ServeInbox { listen, .. } = command {
        ensure!(
            listen.ip().is_loopback(),
            "webhook receiver must bind a loopback address behind HTTPS"
        );
    }
    let candidate = candidate_root(args, command)?;
    let options = match command {
        GitHubCommand::InboxInit { options }
        | GitHubCommand::InboxStatus { options }
        | GitHubCommand::ArchiveInbox { options, .. }
        | GitHubCommand::InspectInboxArchive { options, .. }
        | GitHubCommand::EnqueueEvent { options }
        | GitHubCommand::ServeInbox { options, .. }
        | GitHubCommand::PublishNext { options, .. }
        | GitHubCommand::WorkInbox { options, .. } => options,
        _ => unreachable!(),
    };
    if let Some(candidate) = &candidate {
        let inbox = options
            .inbox
            .canonicalize()
            .map_err(|_| anyhow!("inbox unavailable"))?;
        ensure!(
            !candidate.starts_with(&inbox) && !inbox.starts_with(candidate),
            "inbox and candidate must not overlap"
        );
    }
    let config = GitHubEnrollment::load(&options.event.config_root)
        .map_err(|_| anyhow!("GitHub publisher enrollment is invalid or unsafe"))?
        .ok_or_else(|| anyhow!("GitHub publisher enrollment is missing"))?
        .github_config()?;
    let inbox = if matches!(command, GitHubCommand::InboxInit { .. }) {
        GitHubInbox::initialize(
            &options.inbox,
            config.clone(),
            options.event.installation_id,
        )?
    } else {
        GitHubInbox::open(
            &options.inbox,
            config.clone(),
            options.event.installation_id,
        )?
    };
    if let GitHubCommand::ServeInbox { listen, .. } = command {
        let key = std::env::var("WCODE_GITHUB_WEBHOOK_SECRET")
            .map_err(|_| anyhow!("GitHub webhook verification key is missing or invalid"))?;
        let router = inbox.receiver_router(key.as_bytes())?;
        drop(key);
        let shutdown = worker::shutdown_signal()?;
        let listener = tokio::net::TcpListener::bind(listen).await?;
        println!(
            "{}",
            json!({"listening":listener.local_addr()?.to_string(),
            "path":"/github/webhook", "publication_enabled":false})
        );
        std::io::Write::flush(&mut std::io::stdout())?;
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = shutdown.await;
            })
            .await?;
        return Ok(());
    }
    if let GitHubCommand::WorkInbox { workspace_id, .. } = command {
        let key = std::env::var("WCODE_GITHUB_WEBHOOK_SECRET")
            .map_err(|_| anyhow!("GitHub webhook verification key is missing or invalid"))?;
        ensure!(
            (16..=4096).contains(&key.len()),
            "GitHub webhook verification key is invalid"
        );
        let provider = if app_mode_requested() {
            app_provider(
                config,
                Some(options.event.installation_id),
                candidate.as_deref(),
                Some(&options.inbox),
            )?
        } else {
            let credential = std::env::var("WCODE_GITHUB_PUBLISHER_TOKEN")
                .map_err(|_| anyhow!("GitHub publisher credential is missing or invalid"))?;
            GitHubProvider::new(config, &credential)?
        };
        let candidate = candidate.expect("validated candidate");
        let shutdown = worker::shutdown_signal()?;
        worker::run(
            || inbox.publish_next(&provider, &candidate, workspace_id, key.as_bytes()),
            shutdown,
            emit_worker_event,
        )
        .await?;
        return Ok(());
    }
    let result = match command {
        GitHubCommand::InboxInit { .. } | GitHubCommand::InboxStatus { .. } => inbox.status()?,
        GitHubCommand::ArchiveInbox {
            expected_generation,
            output,
            ..
        } => serde_json::to_value(inbox.archive_terminal(*expected_generation, output)?)?,
        GitHubCommand::InspectInboxArchive {
            archive, digest, ..
        } => inbox.inspect_archive(archive, digest)?,
        GitHubCommand::EnqueueEvent { .. } => {
            let key = std::env::var("WCODE_GITHUB_WEBHOOK_SECRET")
                .map_err(|_| anyhow!("GitHub webhook verification key is missing or invalid"))?;
            let mut headers = HeaderMap::new();
            for (name, variable) in [
                ("x-hub-signature-256", "WCODE_GITHUB_SIGNATURE_256"),
                ("x-github-event", "WCODE_GITHUB_EVENT"),
                ("x-github-delivery", "WCODE_GITHUB_DELIVERY"),
                ("content-type", "WCODE_GITHUB_CONTENT_TYPE"),
            ] {
                let value = std::env::var(variable)
                    .map_err(|_| anyhow!("GitHub webhook header environment is incomplete"))?;
                headers.insert(
                    HeaderName::from_static(name),
                    HeaderValue::from_str(&value)
                        .map_err(|_| anyhow!("GitHub webhook header environment is invalid"))?,
                );
            }
            ensure!(
                !std::io::stdin().is_terminal(),
                "GitHub webhook requires raw JSON on piped stdin"
            );
            let body = read_event_body(std::io::stdin().lock())?;
            serde_json::to_value(inbox.enqueue(&headers, &body, key.as_bytes())?)?
        }
        GitHubCommand::PublishNext { workspace_id, .. } => {
            let key = std::env::var("WCODE_GITHUB_WEBHOOK_SECRET")
                .map_err(|_| anyhow!("GitHub webhook verification key is missing or invalid"))?;
            let provider = if app_mode_requested() {
                app_provider(
                    config,
                    Some(options.event.installation_id),
                    candidate.as_deref(),
                    Some(&options.inbox),
                )?
            } else {
                let credential = std::env::var("WCODE_GITHUB_PUBLISHER_TOKEN")
                    .map_err(|_| anyhow!("GitHub publisher credential is missing or invalid"))?;
                GitHubProvider::new(config, &credential)?
            };
            let receipt = inbox
                .publish_next(
                    &provider,
                    &candidate.expect("validated candidate"),
                    workspace_id,
                    key.as_bytes(),
                )
                .await?;
            let result = json!({"publication_performed":receipt.is_some(), "receipt":receipt});
            if receipt
                .as_ref()
                .is_some_and(|receipt| receipt.verdict() != GateVerdict::Ready)
            {
                println!("{}", serde_json::to_string_pretty(&result)?);
                bail!("current native Acceptance blocks this exact candidate");
            }
            result
        }
        _ => unreachable!(),
    };
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
