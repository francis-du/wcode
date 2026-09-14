use super::*;

pub(super) fn ensure_devtunnel() -> Result<()> {
    if command_succeeds("devtunnel", &["--version"]) {
        return Ok(());
    }
    bail!(
        "Microsoft Dev Tunnels CLI is unavailable. Install `devtunnel`, run `devtunnel user login`, create a persistent anonymous tunnel, and configure the wcode port before selecting --tunnel-provider dev-tunnel"
    )
}

fn validate_devtunnel_id(tunnel_id: &str) -> Result<()> {
    let value = tunnel_id.trim();
    if value.is_empty()
        || value.len() > 160
        || value.starts_with('-')
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        bail!("--dev-tunnel-id must contain only letters, digits, '-' or '_'");
    }
    Ok(())
}

pub(super) async fn start_devtunnel_once(
    local_url: &str,
    tunnel_id: &str,
    monitor: &TaskMonitor,
) -> Result<(Child, String)> {
    validate_devtunnel_id(tunnel_id)?;
    let local = Url::parse(local_url).context("invalid local tunnel target URL")?;
    let port = local
        .port_or_known_default()
        .context("local tunnel target is missing a port")?;
    monitor.operator_message(
        OperatorMessageKind::Info,
        "dev-tunnel",
        format!(
            "hosting persistent tunnel {tunnel_id} for local port {port}; tunnel must already allow anonymous access and contain this port"
        ),
    );

    let mut command = Command::new("devtunnel");
    command
        .args(["host", tunnel_id])
        .stdin(std::process::Stdio::null())
        .stdout(StdStdio::piped())
        .stderr(StdStdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command
        .spawn()
        .context("failed to start Microsoft Dev Tunnel")?;
    let stdout = child
        .stdout
        .take()
        .context("devtunnel stdout is unavailable")?;
    let stderr = child
        .stderr
        .take()
        .context("devtunnel stderr is unavailable")?;
    let (line_sender, mut line_receiver) = mpsc::unbounded_channel::<String>();
    spawn_tunnel_output_reader(stdout, line_sender.clone(), "dev-tunnel");
    spawn_tunnel_output_reader(stderr, line_sender, "dev-tunnel");

    let wait_for_url = async {
        let mut recent_logs = Vec::new();
        while let Some(line) = line_receiver.recv().await {
            if recent_logs.len() >= 20 {
                recent_logs.remove(0);
            }
            recent_logs.push(line.clone());
            if let Some(url) = extract_devtunnel_url(&line, port) {
                return Ok(url);
            }
        }
        Err(if recent_logs.is_empty() {
            "devtunnel exited without output".to_owned()
        } else {
            recent_logs.join("\n")
        })
    };

    let public_url = match timeout(Duration::from_secs(15), wait_for_url).await {
        Ok(Ok(url)) => url,
        Ok(Err(details)) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            bail!("devtunnel exited before exposing local port {port}:\n{details}");
        }
        Err(_) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            bail!("timed out waiting for devtunnel URL for local port {port}");
        }
    };
    Ok((child, public_url))
}

pub(super) fn extract_devtunnel_url(line: &str, port: u16) -> Option<String> {
    if !line.contains(&format!("Hosting port {port} at")) {
        return None;
    }
    let mut fallback = None;
    for token in line.split(|ch: char| {
        ch.is_whitespace() || matches!(ch, ',' | '|' | '`' | '"' | '<' | '>' | ')' | ']' | '}')
    }) {
        let candidate = token
            .trim_matches(|ch: char| matches!(ch, '(' | '[' | '{' | ':' | ';'))
            .trim_end_matches('/')
            .trim_end_matches('.');
        if !candidate.starts_with("https://") {
            continue;
        }
        let Ok(url) = Url::parse(candidate) else {
            continue;
        };
        let Some(host) = url.host_str() else {
            continue;
        };
        let host = host.to_ascii_lowercase();
        if !host.ends_with(".devtunnels.ms")
            || host.contains("-inspect.")
            || !url.username().is_empty()
            || url.password().is_some()
            || !matches!(url.path(), "" | "/")
            || url.query().is_some()
            || url.fragment().is_some()
        {
            continue;
        }
        let normalized = candidate.trim_end_matches('/').to_owned();
        if url.port().is_none() {
            return Some(normalized);
        }
        fallback = Some(normalized);
    }
    fallback
}
