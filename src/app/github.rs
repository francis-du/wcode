//! Fixed-installation GitHub operations. Never execute PR code with publisher
//! credentials. Configuration is explicitly selected outside the candidate.
use crate::git_provider::github::{
    GitHubConfig, GitHubProvider, VerifiedPullRequestDelivery, MAX_WEBHOOK_BYTES,
};
use crate::git_provider::{GateVerdict, GitHubEnrollment};
use crate::verification::change::full_oid;
use anyhow::{anyhow, bail, Result};
use clap::{Args as ClapArgs, Subcommand};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};

#[path = "github_inbox.rs"]
mod inbox_commands;

#[derive(Clone, Debug, Eq, PartialEq, ClapArgs)]
pub(super) struct PublisherOptions {
    /// Trusted operator-managed directory containing .wcode/github-publisher.yaml.
    /// Do not select the untrusted candidate checkout.
    #[arg(long)]
    config_root: PathBuf,
    /// Exact open pull request number.
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    pull: u64,
    /// Print bounded machine-readable diagnostics or the publication receipt.
    #[arg(long)]
    json: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, ClapArgs)]
pub(super) struct EventOptions {
    /// Trusted operator-managed enrollment, never selected by a webhook payload.
    #[arg(long)]
    config_root: PathBuf,
    /// Installation ID selected by trusted deployment configuration, not the payload.
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    installation_id: u64,
    /// Emit only bounded authenticated routing metadata, never the original body.
    #[arg(long)]
    json: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, ClapArgs)]
pub(super) struct InboxOptions {
    #[command(flatten)]
    event: EventOptions,
    /// Private absolute inbox directory, separate from the candidate checkout.
    #[arg(long)]
    inbox: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq, Subcommand)]
pub(super) enum GitHubCommand {
    /// Create a new private durable inbox. Never reset an existing directory.
    InboxInit {
        #[command(flatten)]
        options: InboxOptions,
    },
    /// Read queue metadata without payloads, credentials, networking or writes.
    InboxStatus {
        #[command(flatten)]
        options: InboxOptions,
    },
    /// Privately archive terminal payloads and retain compact replay tombstones.
    /// Explicit generation required; no network, publication or history reset.
    ArchiveInbox {
        #[command(flatten)]
        options: InboxOptions,
        #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
        expected_generation: u64,
        #[arg(long)]
        output: PathBuf,
    },
    /// Check a confidential delivery archive against an independently retained digest.
    InspectInboxArchive {
        #[command(flatten)]
        options: InboxOptions,
        #[arg(long)]
        archive: PathBuf,
        #[arg(long)]
        digest: String,
    },
    /// Foreground loopback receiver; expose only behind a trusted HTTPS proxy.
    ServeInbox {
        #[command(flatten)]
        options: InboxOptions,
        /// Explicit loopback socket, for example 127.0.0.1:8788.
        #[arg(long)]
        listen: std::net::SocketAddr,
    },
    /// Authenticate raw stdin and durably enqueue once per signed body digest.
    EnqueueEvent {
        #[command(flatten)]
        options: InboxOptions,
    },
    /// Process one due delivery with fresh authentication and native publication.
    PublishNext {
        #[command(flatten)]
        options: InboxOptions,
        #[arg(long)]
        workspace_id: String,
    },
    /// Foreground worker: poll due deliveries, preserve retry backoff and drain on shutdown.
    /// Does not run candidate checks, replay completed events or install a service.
    WorkInbox {
        #[command(flatten)]
        options: InboxOptions,
        #[arg(long)]
        workspace_id: String,
    },
    /// Authenticate raw webhook JSON from stdin without network calls or state writes.
    /// The trusted receiver supplies headers and a separate webhook key via environment.
    VerifyEvent {
        #[command(flatten)]
        options: EventOptions,
    },
    /// Authenticate an event, then preflight and publish its exact native candidate.
    /// Requires a trusted bounded receiver/inbox and a fixed publisher installation.
    PublishEvent {
        #[command(flatten)]
        options: EventOptions,
        #[arg(long)]
        workspace_id: String,
    },
    /// Read GitHub configuration without posting Checks or changing branch rules.
    Preflight {
        #[command(flatten)]
        options: PublisherOptions,
        /// Exit nonzero unless a strict expected-App check binding was observed.
        /// This does not verify Check write permission, isolation or Acceptance.
        #[arg(long)]
        check: bool,
    },
    /// Foreground controller: recheck one fixed candidate and its native proof every 30 seconds.
    /// Reuse its Check, revoke on unavailable proof/shutdown, never retarget or run PR code.
    WatchCandidate {
        #[command(flatten)]
        options: PublisherOptions,
        #[arg(long)]
        workspace_id: String,
        #[arg(long, value_parser = parse_sha)]
        base: String,
        #[arg(long, value_parser = parse_sha)]
        head: String,
    },
    /// Publish from fresh native Acceptance, after preflight, for explicit commit SHAs.
    /// Run the fixed installed binary only in a trusted publisher environment.
    Publish {
        #[command(flatten)]
        options: PublisherOptions,
        /// Workspace identity used by the native Verification/Policy state owner.
        #[arg(long)]
        workspace_id: String,
        /// Complete trusted PR base SHA; stale events are rejected, not retargeted.
        #[arg(long, value_parser = parse_sha)]
        base: String,
        /// Complete trusted PR head SHA; never use a branch name or guessed ref.
        #[arg(long, value_parser = parse_sha)]
        head: String,
    },
}

fn parse_sha(value: &str) -> Result<String, String> {
    if full_oid(value) {
        Ok(value.to_ascii_lowercase())
    } else {
        Err("expected a complete Git commit SHA".to_owned())
    }
}

fn candidate_root(args: &super::Args, command: &GitHubCommand) -> Result<Option<PathBuf>> {
    let (config_root, workspace_id, shas_valid) = match command {
        GitHubCommand::Publish {
            options,
            workspace_id,
            base,
            head,
        }
        | GitHubCommand::WatchCandidate {
            options,
            workspace_id,
            base,
            head,
        } => (
            &options.config_root,
            workspace_id,
            full_oid(base) && full_oid(head),
        ),
        GitHubCommand::PublishEvent {
            options,
            workspace_id,
        } => (&options.config_root, workspace_id, true),
        GitHubCommand::PublishNext {
            options,
            workspace_id,
        }
        | GitHubCommand::WorkInbox {
            options,
            workspace_id,
        } => (&options.event.config_root, workspace_id, true),
        _ => return Ok(None),
    };
    {
        if !(args.allow_write || args.full_access) || !(args.allow_exec || args.full_access) {
            bail!("GitHub publication is disabled by read-only or no-exec mode");
        }
        if args.workspace.len() != 1 {
            bail!("GitHub publication requires exactly one candidate workspace");
        }
        if workspace_id.trim().is_empty()
            || workspace_id.len() > 256
            || workspace_id.chars().any(char::is_control)
            || !shas_valid
        {
            bail!(
                "GitHub publication requires bounded workspace identity and complete commit SHAs"
            );
        }
        let candidate = std::fs::canonicalize(&args.workspace[0])
            .map_err(|_| anyhow!("GitHub candidate workspace is unavailable"))?;
        let trusted = std::fs::canonicalize(config_root)
            .map_err(|_| anyhow!("GitHub publisher configuration directory is unavailable"))?;
        if candidate.starts_with(&trusted) || trusted.starts_with(&candidate) {
            bail!("publisher configuration must not overlap the candidate workspace");
        }
        let state_root = crate::core_types::authority_state_root()
            .map_err(|_| anyhow!("GitHub publisher authority state root is unavailable"))?;
        if !state_root.is_absolute() {
            bail!("GitHub publisher authority state root must be absolute");
        }
        let state_root = if state_root.exists() {
            std::fs::canonicalize(&state_root)
                .map_err(|_| anyhow!("GitHub publisher authority state root is unsafe"))?
        } else {
            state_root
        };
        if candidate.starts_with(&state_root) || state_root.starts_with(&candidate) {
            bail!("publisher authority state must not overlap the candidate workspace");
        }
        Ok(Some(candidate))
    }
}

pub(super) async fn run(args: &super::Args, command: &GitHubCommand) -> Result<()> {
    if matches!(
        command,
        GitHubCommand::InboxInit { .. }
            | GitHubCommand::InboxStatus { .. }
            | GitHubCommand::ArchiveInbox { .. }
            | GitHubCommand::InspectInboxArchive { .. }
            | GitHubCommand::EnqueueEvent { .. }
            | GitHubCommand::PublishNext { .. }
            | GitHubCommand::WorkInbox { .. }
            | GitHubCommand::ServeInbox { .. }
    ) {
        return inbox_commands::run(args, command).await;
    }
    if matches!(
        command,
        GitHubCommand::VerifyEvent { .. } | GitHubCommand::PublishEvent { .. }
    ) {
        return run_event(args, command).await;
    }
    // Validate restrictions before reading credentials or making any network call.
    let candidate = candidate_root(args, command)?;
    let options = match command {
        GitHubCommand::Preflight { options, .. }
        | GitHubCommand::Publish { options, .. }
        | GitHubCommand::WatchCandidate { options, .. } => options,
        _ => unreachable!("event commands are handled before publisher credential access"),
    };
    if options.pull == 0 {
        bail!("GitHub pull request identity must be positive");
    }
    let enrollment = GitHubEnrollment::load(&options.config_root)
        .map_err(|_| anyhow!("GitHub publisher enrollment is invalid or unsafe"))?
        .ok_or_else(|| {
            anyhow!("GitHub publisher enrollment is missing in the selected config root")
        })?;
    let provider = if app_mode_requested() {
        app_provider(
            enrollment.github_config()?,
            None,
            candidate.as_deref(),
            None,
        )?
    } else {
        let token = std::env::var("WCODE_GITHUB_PUBLISHER_TOKEN")
            .map_err(|_| anyhow!("GitHub publisher credential is missing or invalid"))?;
        let provider = GitHubProvider::new(enrollment.github_config()?, &token)?;
        drop(token);
        provider
    };
    if let GitHubCommand::WatchCandidate {
        workspace_id,
        base,
        head,
        ..
    } = command
    {
        let shutdown = inbox_commands::worker::shutdown_signal()?;
        let mut watch = provider.watch_candidate(
            options.pull,
            (base, head),
            candidate.expect("validated watch candidate"),
            workspace_id,
        )?;
        watch
            .run(shutdown, inbox_commands::emit_worker_event)
            .await?;
        return Ok(());
    }
    match command {
        GitHubCommand::VerifyEvent { .. }
        | GitHubCommand::PublishEvent { .. }
        | GitHubCommand::InboxInit { .. }
        | GitHubCommand::InboxStatus { .. }
        | GitHubCommand::ArchiveInbox { .. }
        | GitHubCommand::InspectInboxArchive { .. }
        | GitHubCommand::EnqueueEvent { .. }
        | GitHubCommand::PublishNext { .. }
        | GitHubCommand::WorkInbox { .. }
        | GitHubCommand::ServeInbox { .. }
        | GitHubCommand::WatchCandidate { .. } => unreachable!(),
        GitHubCommand::Preflight { check, .. } => {
            let report = provider.preflight(options.pull).await?;
            if options.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "GitHub required-check configuration: {}",
                    if report.configuration_verified {
                        "verified observation (not Acceptance)"
                    } else {
                        "incomplete"
                    }
                );
                for finding in &report.findings {
                    println!("{}: {}", finding.code, finding.action);
                }
                println!("No repository changes. App-key mode may issue a credential; write permission, deployment isolation and current Acceptance require separate verification.");
            }
            if *check && !report.configuration_verified {
                bail!("GitHub deployment preflight incomplete");
            }
        }
        GitHubCommand::Publish {
            workspace_id,
            base,
            head,
            ..
        } => {
            let receipt = provider
                .publish_preflighted(
                    options.pull,
                    (base, head),
                    candidate.expect("validated publication workspace"),
                    workspace_id,
                )
                .await?;
            if options.json {
                println!("{}", serde_json::to_string_pretty(&receipt)?);
            } else {
                println!(
                    "GitHub exact-candidate publication: {:?}; Check {}",
                    receipt.verdict(),
                    receipt.check().id
                );
            }
            if receipt.verdict() != GateVerdict::Ready {
                bail!("current native Acceptance blocks this exact candidate");
            }
        }
    }
    Ok(())
}

fn app_mode_requested() -> bool {
    [
        "WCODE_GITHUB_APP_PRIVATE_KEY",
        "WCODE_GITHUB_APP_PRIVATE_KEY_FILE",
        "WCODE_GITHUB_APP_KEY_FILE",
        "WCODE_GITHUB_INSTALLATION_ID",
    ]
    .into_iter()
    .any(|name| std::env::var_os(name).is_some())
}

// Deliberate opt-in only, after existing safety and event-authentication guards.
// The static-token path remains compatible and never silently falls back from a
// failed App configuration. Neither private key nor generated token is printed.
fn app_provider(
    config: GitHubConfig,
    expected_installation: Option<u64>,
    candidate: Option<&Path>,
    inbox: Option<&Path>,
) -> Result<GitHubProvider> {
    if std::env::var_os("WCODE_GITHUB_PUBLISHER_TOKEN").is_some() {
        bail!("GitHub publisher credential sources are ambiguous; select static token or App key");
    }
    let installation = match std::env::var("WCODE_GITHUB_INSTALLATION_ID") {
        Ok(value) if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) => {
            value.parse::<u64>().ok().filter(|id| *id > 0)
        }
        Err(std::env::VarError::NotPresent) => expected_installation,
        _ => None,
    }
    .ok_or_else(|| anyhow!("GitHub App renewal requires a positive trusted installation ID"))?;
    if expected_installation.is_some_and(|expected| expected != installation) {
        bail!("GitHub App renewal installation differs from trusted event routing");
    }
    let private_key =
        std::env::var_os("WCODE_GITHUB_APP_PRIVATE_KEY").filter(|value| !value.is_empty());
    let private_key_file =
        std::env::var_os("WCODE_GITHUB_APP_PRIVATE_KEY_FILE").filter(|value| !value.is_empty());
    let legacy_key_file =
        std::env::var_os("WCODE_GITHUB_APP_KEY_FILE").filter(|value| !value.is_empty());
    let source_count = usize::from(private_key.is_some())
        + usize::from(private_key_file.is_some())
        + usize::from(legacy_key_file.is_some());
    if source_count != 1 {
        bail!("GitHub App renewal requires exactly one signing-key source");
    }
    if private_key.is_some() {
        let key = std::env::var("WCODE_GITHUB_APP_PRIVATE_KEY")
            .map_err(|_| anyhow!("GitHub App in-memory signing key is invalid"))?;
        let provider = GitHubProvider::with_app_key_pem(config, installation, key.as_bytes());
        drop(key);
        return provider;
    }
    let key = PathBuf::from(
        private_key_file
            .or(legacy_key_file)
            .expect("one validated file source remains"),
    );
    for excluded in [candidate, inbox].into_iter().flatten() {
        if key.starts_with(excluded) {
            bail!("GitHub App signing key must be outside candidate and inbox directories");
        }
    }
    // with_app_key rejects relative paths, symlink ancestors, hard links and
    // nonprivate files before reading. Do not canonicalize away those checks.
    GitHubProvider::with_app_key(config, installation, key)
}

fn read_event_body(input: impl Read) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    input
        .take(MAX_WEBHOOK_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| anyhow!("GitHub webhook input could not be read"))?;
    if bytes.is_empty() || bytes.len() > MAX_WEBHOOK_BYTES {
        bail!("GitHub webhook body is empty or exceeds its byte bound");
    }
    Ok(bytes)
}

async fn run_event(args: &super::Args, command: &GitHubCommand) -> Result<()> {
    // Reject disabled or overlapping publication before stdin, keys or networking.
    let candidate = candidate_root(args, command)?;
    let (options, workspace_id) = match command {
        GitHubCommand::VerifyEvent { options } => (options, None),
        GitHubCommand::PublishEvent {
            options,
            workspace_id,
        } => (options, Some(workspace_id)),
        _ => unreachable!(),
    };
    if options.installation_id == 0 {
        bail!("GitHub webhook installation identity must be positive");
    }
    let enrollment = GitHubEnrollment::load(&options.config_root)
        .map_err(|_| anyhow!("GitHub publisher enrollment is invalid or unsafe"))?
        .ok_or_else(|| {
            anyhow!("GitHub publisher enrollment is missing in the selected config root")
        })?;
    let config = enrollment.github_config()?;
    let secret = std::env::var("WCODE_GITHUB_WEBHOOK_SECRET")
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
    if std::io::stdin().is_terminal() {
        bail!("GitHub webhook requires raw JSON on piped stdin");
    }
    let body = read_event_body(std::io::stdin().lock())?;
    let delivery = VerifiedPullRequestDelivery::verify(
        &config,
        options.installation_id,
        &headers,
        &body,
        secret.as_bytes(),
    )?;
    drop(secret);
    drop(body);
    if let Some(workspace_id) = workspace_id {
        // Only authenticated and bounded event input reaches the API credential.
        let provider = if app_mode_requested() {
            app_provider(
                config,
                Some(options.installation_id),
                candidate.as_deref(),
                None,
            )?
        } else {
            let credential = std::env::var("WCODE_GITHUB_PUBLISHER_TOKEN")
                .map_err(|_| anyhow!("GitHub publisher credential is missing or invalid"))?;
            let provider = GitHubProvider::new(config, &credential)?;
            drop(credential);
            provider
        };
        let receipt = provider
            .publish_delivery(
                &delivery,
                candidate.expect("validated publication workspace"),
                workspace_id,
            )
            .await?;
        if options.json {
            println!("{}", serde_json::to_string_pretty(&receipt)?);
        } else {
            println!(
                "GitHub exact-event publication: {:?}; Check {}",
                receipt.verdict(),
                receipt.check().id
            );
        }
        if receipt.verdict() != GateVerdict::Ready {
            bail!("current native Acceptance blocks this exact candidate");
        }
    } else if options.json {
        println!("{}", serde_json::to_string_pretty(&delivery)?);
    } else {
        println!(
            "GitHub webhook authenticated for PR {}; {}",
            delivery.target().change,
            delivery.body_digest()
        );
        println!("No remote changes. Signature validation is not freshness, replay protection, human approval or Acceptance.");
    }
    Ok(())
}

#[cfg(test)]
#[path = "../../tests/unit/app/github.rs"]
mod tests;
