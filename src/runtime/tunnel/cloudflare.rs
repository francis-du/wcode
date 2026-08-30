use super::*;

pub(super) fn ensure_cloudflared(install_missing: bool) -> Result<()> {
    println!("  · cloudflared  checking dependency");
    if command_succeeds("cloudflared", &["--version"]) {
        println!("  ✓ cloudflared  available");
        return Ok(());
    }
    if !install_missing {
        bail!(
            "cloudflared is missing; {} Remove --no-install to allow the supported installer.",
            cloudflared_install_hint()
        );
    }

    #[cfg(target_os = "macos")]
    {
        if !command_succeeds("brew", &["--version"]) {
            bail!(
                "cloudflared is missing and Homebrew is unavailable; {}",
                cloudflared_install_hint()
            );
        }
        run_installer("brew", &["install", "cloudflared"], "Homebrew")?;
    }

    #[cfg(target_os = "windows")]
    {
        if !command_succeeds("winget", &["--version"]) {
            bail!(
                "cloudflared is missing and winget is unavailable. Install it from https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/ or place cloudflared.exe on PATH."
            );
        }
        run_installer(
            "winget",
            &[
                "install",
                "--id",
                "Cloudflare.cloudflared",
                "--exact",
                "--accept-package-agreements",
                "--accept-source-agreements",
            ],
            "winget",
        )?;
    }

    #[cfg(target_os = "linux")]
    {
        bail!(
            "cloudflared is missing. {} Automatic distro installation is intentionally disabled because cloudflared is not consistently available in default repositories.",
            cloudflared_install_hint()
        );
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        bail!("cloudflared is missing; install it from Cloudflare and place it on PATH");
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        if !command_succeeds("cloudflared", &["--version"]) {
            bail!(
                "the installer completed but cloudflared is still unavailable on PATH; restart the terminal or install it manually"
            );
        }
        println!("  ✓ cloudflared  installed");
        Ok(())
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn run_installer(program: &str, args: &[&str], label: &str) -> Result<()> {
    println!("  ↓ cloudflared  installing with {label}");
    let status = StdCommand::new(program)
        .args(args)
        .stdin(StdStdio::inherit())
        .stdout(StdStdio::inherit())
        .stderr(StdStdio::inherit())
        .status()
        .with_context(|| format!("failed to launch {label}"))?;
    if !status.success() {
        bail!(
            "{label} could not install cloudflared; {}",
            cloudflared_install_hint()
        );
    }
    Ok(())
}

fn cloudflared_install_hint() -> String {
    #[cfg(target_os = "macos")]
    {
        return "Run `brew install cloudflared`.".to_owned();
    }
    #[cfg(target_os = "windows")]
    {
        return "Run `winget install --id Cloudflare.cloudflared` or download the official Windows binary.".to_owned();
    }
    #[cfg(target_os = "linux")]
    {
        let manager = ["apt-get", "dnf", "yum", "pacman"]
            .into_iter()
            .find(|program| command_succeeds(program, &["--version"]))
            .unwrap_or("your distribution package manager");
        return format!(
            "Detected {manager}; follow Cloudflare's repository instructions at https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/."
        );
    }
    #[allow(unreachable_code)]
    "Install cloudflared from https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/ and place it on PATH.".to_owned()
}

pub(super) fn command_succeeds(program: &str, args: &[&str]) -> bool {
    StdCommand::new(program)
        .args(args)
        .stdin(StdStdio::null())
        .stdout(StdStdio::null())
        .stderr(StdStdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

pub(super) async fn start_cloudflared_once(local_url: &str) -> Result<(Child, String)> {
    let mut command = Command::new("cloudflared");
    command
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
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command.spawn().context("failed to start cloudflared")?;
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
                    if let Some(url) = extract_cloudflare_tunnel_url(&line) {
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
    let public_url = match timeout(Duration::from_secs(15), url_receiver).await {
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
            bail!("timed out after 15 seconds waiting for Cloudflare Tunnel URL");
        }
    };
    Ok((child, public_url))
}

pub(crate) fn extract_cloudflare_tunnel_url(line: &str) -> Option<String> {
    for (start, _) in line.match_indices("https://") {
        let candidate = line[start..]
            .split(|ch: char| {
                ch.is_whitespace()
                    || matches!(ch, '|' | '`' | '"' | '<' | '>' | ')' | ']' | '}' | ',')
            })
            .next()
            .unwrap_or_default()
            .trim_end_matches('/');
        let Ok(url) = Url::parse(candidate) else {
            continue;
        };
        let Some(host) = url.host_str() else { continue };
        if host.ends_with(".trycloudflare.com") && host != "api.trycloudflare.com" {
            return Some(candidate.to_owned());
        }
    }
    None
}
