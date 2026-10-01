//! Run only from a fixed, reviewed publisher installation, never from a PR checkout.
use anyhow::{bail, Result};
use clap::Parser;
use std::path::PathBuf;
use wcode::git_provider::github::GitHubProvider;
use wcode::git_provider::{GateVerdict, GitHubEnrollment};

#[derive(Parser)]
#[command(about = "Publish an exact-SHA GitHub check from current native Acceptance")]
struct Arguments {
    /// Trusted, independently managed candidate checkout. No checks execute here.
    #[arg(long)]
    workspace: PathBuf,
    #[arg(long)]
    workspace_id: String,
    #[arg(long)]
    pull: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    let arguments = Arguments::parse();
    let token = std::env::var("WCODE_GITHUB_APP_TOKEN")
        .map_err(|_| anyhow::anyhow!("publisher credential is missing or invalid"))?;
    let enrollment = GitHubEnrollment::load(&arguments.workspace)?
        .ok_or_else(|| anyhow::anyhow!(
            "GitHub publisher enrollment is missing; run wcode setup --project with the GitHub enrollment options"
        ))?;
    let config = enrollment.github_config()?;
    let provider = GitHubProvider::new(config, &token)?;
    drop(token);
    let receipt = provider
        .publish_local(arguments.pull, arguments.workspace, &arguments.workspace_id)
        .await?;
    println!("{}", serde_json::to_string(&receipt)?);
    if receipt.verdict() != GateVerdict::Ready {
        bail!("current native Acceptance blocks this exact candidate");
    }
    Ok(())
}
