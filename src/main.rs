mod auth;
mod harness;
mod mcp;
mod monitor;
mod workspace;

use anyhow::{bail, Context, Result};
use auth::AuthState;
use clap::{ArgAction, Parser};
use harness::ToolHarness;
use mcp::AppState;
use monitor::{MonitorConfig, TaskMonitor};
use std::path::PathBuf;
use std::process::{Command as StdCommand, Stdio as StdStdio};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpListener;
use tokio::process::{Child, Command};
use tokio::sync::oneshot;
use tokio::time::{sleep, timeout, Duration};
use tracing_subscriber::EnvFilter;
use workspace::Workspaces;

#[derive(Debug, Parser)]
#[command(
    name = "wcode",
    version,
    about = "Give ChatGPT Web authenticated, workspace-scoped coding tools"
)]
struct Args {
    /// Local code directory exposed to ChatGPT. Repeat to expose multiple roots.
    #[arg(long, value_name = "PATH", default_value = ".")]
    workspace: Vec<PathBuf>,

    /// Local interface for the MCP server.
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Local port for the MCP server.
    #[arg(long, default_value_t = 8765)]
    port: u16,

    /// Use an existing public base URL instead of starting Cloudflare Tunnel.
    #[arg(long)]
    public_url: Option<String>,

    /// Keep the server local and do not start a public tunnel.
    #[arg(long)]
    no_tunnel: bool,

    /// Disable file modification tools.
    #[arg(long = "read-only", action = ArgAction::SetFalse, default_value_t = true)]
    allow_write: bool,

    /// Disable build/test/status command execution.
    #[arg(long = "no-exec", action = ArgAction::SetFalse, default_value_t = true)]
    allow_exec: bool,

    /// Maximum number of tool bodies that may run concurrently across requests.
    #[arg(long, default_value_t = 8)]
    max_parallel_tools: usize,

    /// Disable the live terminal task monitor.
    #[arg(long = "no-monitor", action = ArgAction::SetFalse, default_value_t = true)]
    monitor: bool,

    /// Do not offer to install a missing cloudflared dependency with Homebrew.
    #[arg(long)]
    no_install: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .compact()
        .without_time()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("wcode=info")),
        )
        .with_target(false)
        .init();

    let args = Args::parse();
    let workspaces = Workspaces::new(&args.workspace, args.allow_write, args.allow_exec)?;
    let harness = ToolHarness::new(args.max_parallel_tools)?;
    let monitor = TaskMonitor::new(workspaces.roots().into_iter().map(|(id, _)| id));
    let listener = TcpListener::bind(format!("{}:{}", args.host, args.port))
        .await
        .with_context(|| format!("cannot bind {}:{}", args.host, args.port))?;
    let local_url = format!("http://{}:{}", args.host, listener.local_addr()?.port());
    let auth = Arc::new(AuthState::new(local_url.clone()));

    let mut tunnel: Option<Child> = None;
    let public_url = if let Some(url) = args.public_url {
        url.trim_end_matches('/').to_owned()
    } else if args.no_tunnel {
        local_url.clone()
    } else {
        ensure_cloudflared(!args.no_install)?;
        let (child, url) = start_cloudflared(&local_url).await?;
        tunnel = Some(child);
        url
    };
    auth.set_public_url(public_url.clone());

    let app_state = Arc::new(AppState {
        auth: auth.clone(),
        workspaces: workspaces.clone(),
        harness: harness.clone(),
        monitor: monitor.clone(),
    });
    let app = auth::router(auth.clone()).merge(mcp::router(app_state));

    let monitor_config = MonitorConfig {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        local_url: local_url.clone(),
        mcp_url: format!("{public_url}/mcp"),
        pairing_code: auth.pairing_code().to_owned(),
        max_parallel: harness.max_parallel(),
        workspaces: workspaces
            .roots()
            .into_iter()
            .map(|(id, root)| {
                let is_default = id == workspaces.default_id();
                (id, root.display().to_string(), is_default)
            })
            .collect(),
    };
    let renderer = monitor.spawn_renderer(monitor_config, args.monitor);
    if renderer.is_none() {
        print_setup_guide(
            &workspaces,
            &local_url,
            &public_url,
            auth.pairing_code(),
            args.no_tunnel,
            harness.max_parallel(),
        );
    }

    let server = axum::serve(listener, app);
    tokio::select! {
        result = server => result.context("MCP server failed")?,
        _ = tokio::signal::ctrl_c() => {}
    }
    if let Some(renderer) = renderer {
        renderer.stop().await;
    }
    println!("  ◼ wcode stopped");

    if let Some(mut child) = tunnel.take() {
        let _ = child.start_kill();
        let _ = child.wait().await;
    }
    Ok(())
}

fn ensure_cloudflared(install_missing: bool) -> Result<()> {
    println!("  · cloudflared  checking dependency");
    if command_succeeds("cloudflared", &["--version"]) {
        println!("  ✓ cloudflared  available");
        return Ok(());
    }
    if !install_missing {
        bail!("cloudflared is missing; install it with `brew install cloudflared` or omit --no-install");
    }
    if !command_succeeds("brew", &["--version"]) {
        bail!("cloudflared is missing and Homebrew is unavailable; install cloudflared manually");
    }
    println!("  ↓ cloudflared  installing with Homebrew");
    let status = StdCommand::new("brew")
        .args(["install", "cloudflared"])
        .stdin(StdStdio::inherit())
        .stdout(StdStdio::inherit())
        .stderr(StdStdio::inherit())
        .status()
        .context("failed to launch Homebrew")?;
    if !status.success() {
        bail!("Homebrew could not install cloudflared");
    }
    println!("  ✓ cloudflared  installed");
    Ok(())
}

fn command_succeeds(program: &str, args: &[&str]) -> bool {
    StdCommand::new(program)
        .args(args)
        .stdin(StdStdio::null())
        .stdout(StdStdio::null())
        .stderr(StdStdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

async fn start_cloudflared(local_url: &str) -> Result<(Child, String)> {
    println!("  ↗ tunnel       requesting HTTPS endpoint");
    let mut last_error = String::new();
    for attempt in 1..=3 {
        match start_cloudflared_once(local_url).await {
            Ok(result) => {
                println!("  ✓ tunnel       connected");
                return Ok(result);
            }
            Err(error) => {
                last_error = format!("{error:#}");
                let summary = last_error.lines().next().unwrap_or("unknown error");
                eprintln!("  ! tunnel       attempt {attempt}/3 failed · {summary}");
                if attempt < 3 {
                    sleep(Duration::from_secs(attempt * 2)).await;
                }
            }
        }
    }
    bail!(
        "Cloudflare Quick Tunnel failed after 3 attempts.\n\
         Last error: {last_error}\n\
         Check network/VPN access to https://api.trycloudflare.com, or use a stable reverse proxy and pass \
         `--public-url https://your-host.example`."
    )
}

async fn start_cloudflared_once(local_url: &str) -> Result<(Child, String)> {
    let mut child = Command::new("cloudflared")
        .args([
            "tunnel",
            "--url",
            local_url,
            "--protocol",
            "http2",
            "--no-autoupdate",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("failed to start cloudflared")?;
    let stderr = child
        .stderr
        .take()
        .context("cloudflared stderr is unavailable")?;
    let (url_sender, url_receiver) = oneshot::channel::<Result<String, String>>();
    tokio::spawn(async move {
        let mut url_sender = Some(url_sender);
        let mut recent_logs: Vec<String> = Vec::new();
        let mut lines = BufReader::new(stderr).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    recent_logs.push(line.clone());
                    if recent_logs.len() > 12 {
                        recent_logs.remove(0);
                    }
                    if let Some(url) = extract_tunnel_url(&line) {
                        if let Some(sender) = url_sender.take() {
                            let _ = sender.send(Ok(url));
                        }
                    }
                    if line.contains("ERR") || line.contains("error") {
                        tracing::debug!(target: "wcode::tunnel", "{line}");
                    }
                }
                Ok(None) => {
                    if let Some(sender) = url_sender.take() {
                        let details = if recent_logs.is_empty() {
                            "cloudflared exited without output".to_owned()
                        } else {
                            recent_logs.join("\n")
                        };
                        let _ = sender.send(Err(details));
                    }
                    break;
                }
                Err(error) => {
                    tracing::debug!(target: "wcode::tunnel", "failed to read logs: {error}");
                    if let Some(sender) = url_sender.take() {
                        let _ =
                            sender.send(Err(format!("failed to read cloudflared logs: {error}")));
                    }
                    break;
                }
            }
        }
    });
    let public_url = match timeout(Duration::from_secs(30), url_receiver).await {
        Ok(Ok(Ok(url))) => url,
        Ok(Ok(Err(details))) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            bail!("cloudflared exited before producing a public URL:\n{details}");
        }
        Ok(Err(_)) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            bail!("cloudflared log channel closed unexpectedly");
        }
        Err(_) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            bail!("timed out after 30 seconds waiting for Cloudflare Tunnel URL");
        }
    };
    Ok((child, public_url))
}

fn extract_tunnel_url(line: &str) -> Option<String> {
    for (start, _) in line.match_indices("https://") {
        let candidate = line[start..]
            .split(|ch: char| {
                ch.is_whitespace()
                    || matches!(ch, '|' | '`' | '"' | '<' | '>' | ')' | ']' | '}' | ',')
            })
            .next()
            .unwrap_or_default()
            .trim_end_matches('/');
        let Ok(url) = url::Url::parse(candidate) else {
            continue;
        };
        let Some(host) = url.host_str() else { continue };
        if host.ends_with(".trycloudflare.com") && host != "api.trycloudflare.com" {
            return Some(candidate.to_owned());
        }
    }
    None
}

fn print_setup_guide(
    workspaces: &Workspaces,
    local_url: &str,
    public_url: &str,
    pairing_code: &str,
    local_only: bool,
    max_parallel_tools: usize,
) {
    println!(
        "\n╭─ wcode {} ─────────────────────────────────────────",
        env!("CARGO_PKG_VERSION")
    );
    println!("│");
    println!("│  MCP        {public_url}/mcp");
    println!("│  Local      {local_url}");
    println!("│  Pair code  {pairing_code}");
    println!("│  Parallel   {max_parallel_tools} tool calls");
    println!("│");
    println!("│  Workspaces");
    for (id, root) in workspaces.roots() {
        let default = if id == workspaces.default_id() {
            "  default"
        } else {
            ""
        };
        println!("│    • {id:<16} {}{default}", root.display());
    }
    println!("│");
    if local_only {
        println!("│  Local-only mode: remote ChatGPT cannot reach this endpoint.");
        println!("│  Restart without --no-tunnel or pass --public-url.");
    } else {
        println!("│  Connect");
        println!("│    1  Add a custom MCP server in ChatGPT Developer mode");
        println!("│    2  URL   {public_url}/mcp");
        println!("│    3  Auth  OAuth · enter pairing code {pairing_code}");
    }
    println!("│");
    println!("│  GitHub     https://github.com/francis-du/wcode");
    println!("╰─ Ctrl-C to stop ─────────────────────────────────────\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_quick_tunnel_url() {
        let line = "2026 INF | https://bright-demo.trycloudflare.com |";
        assert_eq!(
            extract_tunnel_url(line).as_deref(),
            Some("https://bright-demo.trycloudflare.com")
        );
        assert_eq!(
            extract_tunnel_url("request https://api.trycloudflare.com/tunnel\": failed"),
            None
        );
    }
}
