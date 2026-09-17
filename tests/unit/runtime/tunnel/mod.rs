use super::cloudflare::extract_cloudflare_tunnel_url;
use super::*;

#[test]
fn tunnel_event_stays_compact_and_preserves_endpoint_ownership() {
    assert!(
        std::mem::size_of::<TunnelEvent>() <= 64,
        "platform-specific process handles must not inflate every queued event"
    );
    let mut active = ActiveTunnel::test_fixture(
        TunnelProvider::Tailscale,
        "https://stable.example".to_owned(),
    );
    active.endpoint_epoch = Some(42);
    let event = TunnelEvent::Connected(Box::new(active));
    let TunnelEvent::Connected(active) = event else {
        panic!("connection event changed kind");
    };
    let active = *active;
    assert_eq!(active.endpoint_epoch, Some(42));
    assert_eq!(active.public_url(), "https://stable.example");
    assert_eq!(active.provider(), TunnelProvider::Tailscale);
}

#[test]
fn tunnel_runtime_never_writes_directly_to_the_terminal() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for path in [
        "src/runtime/tunnel/mod.rs",
        "src/runtime/tunnel/cloudflare.rs",
    ] {
        let source = std::fs::read_to_string(root.join(path)).unwrap();
        assert!(!source.contains("println!("), "{path} bypasses TaskMonitor");
        assert!(
            !source.contains("eprintln!("),
            "{path} bypasses TaskMonitor"
        );
        assert!(
            !source.contains("Stdio::inherit"),
            "{path} lets a child process corrupt the dashboard"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn blocking_tunnel_dependency_setup_does_not_stall_async_runtime() {
    let (started_tx, started_rx) = oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let worker = tokio::spawn(async move {
        run_blocking_tunnel_setup("test dependency", move || {
            let _ = started_tx.send(());
            release_rx
                .recv_timeout(Duration::from_secs(2))
                .map_err(|error| anyhow!("test dependency release failed: {error}"))?;
            Ok(())
        })
        .await
        .unwrap();
    });

    timeout(Duration::from_secs(1), started_rx)
        .await
        .expect("blocking worker should start")
        .expect("blocking worker should report startup");
    timeout(
        Duration::from_millis(100),
        tokio::time::sleep(Duration::from_millis(10)),
    )
    .await
    .expect("a blocking dependency check must not stall the async runtime");
    release_tx.send(()).unwrap();
    timeout(Duration::from_secs(1), worker)
        .await
        .expect("blocking worker should finish")
        .expect("blocking worker task should join");
}

#[tokio::test]
async fn recovered_stable_tunnel_is_not_treated_as_a_dead_child_process() {
    let mut tunnel = ActiveTunnel {
        child: None,
        endpoint_epoch: None,
        public_url: "https://stable.example".to_owned(),
        provider: TunnelProvider::Tailscale,
        connected_at: std::time::Instant::now(),
    };
    assert!(tunnel.try_wait().unwrap().is_none());
    tokio::time::timeout(Duration::from_millis(50), tunnel.stop())
        .await
        .expect("stopping a recovered stable endpoint must be a no-op");
}

#[test]
fn public_health_response_must_match_the_current_instance() {
    let body = br#"{"ok":true,"instance_id":"instance-a"}"#;
    assert!(validate_health_response(body, "instance-a").is_ok());

    let mismatch = validate_health_response(body, "instance-b").unwrap_err();
    assert!(mismatch.contains("different wcode instance"));
    assert!(validate_health_response(br#"{"ok":true}"#, "instance-a")
        .unwrap_err()
        .contains("missing instance_id"));
    assert!(
        validate_health_response(br#"{"ok":false,"instance_id":"instance-a"}"#, "instance-a")
            .unwrap_err()
            .contains("ok=true")
    );
}

#[test]
fn health_probe_parallelism_covers_auto_providers_without_unbounded_alias_fanout() {
    assert_eq!(
        PUBLIC_HEALTH_PARALLELISM,
        TunnelProvider::auto_candidates().len(),
        "all auto providers may probe concurrently, but retained aliases must queue"
    );
}

#[test]
fn provider_startup_retries_are_staggered_without_randomness() {
    let first = [
        provider_retry_delay(TunnelProvider::Cloudflare, 1),
        provider_retry_delay(TunnelProvider::LocalhostRun, 1),
        provider_retry_delay(TunnelProvider::Pinggy, 1),
        provider_retry_delay(TunnelProvider::Tailscale, 1),
        provider_retry_delay(TunnelProvider::DevTunnel, 1),
    ];
    assert_eq!(first[0], Duration::from_secs(5));
    assert_eq!(first[1], Duration::from_secs(6));
    assert_eq!(first[2], Duration::from_secs(4));
    assert_eq!(first[3], Duration::from_secs(3));
    assert_eq!(first[4], Duration::from_secs(5));
    assert_eq!(
        provider_retry_delay(TunnelProvider::Cloudflare, 1),
        first[0]
    );
    assert_eq!(
        provider_retry_delay(TunnelProvider::Cloudflare, 2),
        Duration::from_secs(9)
    );
    assert_eq!(
        provider_retry_delay(TunnelProvider::Cloudflare, 3),
        Duration::from_secs(12)
    );
    assert_eq!(
        provider_retry_delay(TunnelProvider::Cloudflare, 4),
        Duration::from_secs(25)
    );
    assert!(provider_retry_delay(TunnelProvider::Cloudflare, 40) <= PROVIDER_STARTUP_MAX_DELAY);
}

#[test]
fn public_url_requires_https_or_loopback_http() {
    assert_eq!(
        normalize_public_url("https://example.com/").unwrap(),
        "https://example.com"
    );
    assert_eq!(
        normalize_public_url("http://127.0.0.1:8765/").unwrap(),
        "http://127.0.0.1:8765"
    );
    assert_eq!(
        normalize_public_url("http://[::1]:8765/").unwrap(),
        "http://[::1]:8765"
    );
    for value in [
        "http://example.com",
        "https://example.com/gateway",
        "ftp://example.com",
        "https://user@example.com",
        "https://example.com?mode=test",
        "https://example.com/#fragment",
        "not-a-url",
    ] {
        assert!(
            normalize_public_url(value).is_err(),
            "unexpectedly accepted {value}"
        );
    }
}

#[test]
fn parses_persistent_dev_tunnel_url_for_the_requested_port() {
    let line = "Hosting port 8765 at https://demo.usw2.devtunnels.ms:8765/, https://demo-8765.usw2.devtunnels.ms/ and inspect it at https://demo-8765-inspect.usw2.devtunnels.ms/";
    assert_eq!(
        extract_devtunnel_url(line, 8765).as_deref(),
        Some("https://demo-8765.usw2.devtunnels.ms")
    );
    assert_eq!(extract_devtunnel_url(line, 3978), None);
    assert_eq!(
        extract_devtunnel_url(
            "Hosting port 8765 at https://demo-8765-inspect.usw2.devtunnels.ms/",
            8765
        ),
        None
    );
}

#[test]
fn parses_quick_tunnel_url() {
    let line = "2026 INF | https://bright-demo.trycloudflare.com |";
    assert_eq!(
        extract_cloudflare_tunnel_url(line).as_deref(),
        Some("https://bright-demo.trycloudflare.com")
    );
    assert_eq!(
        extract_cloudflare_tunnel_url("request https://api.trycloudflare.com/tunnel\": failed"),
        None
    );
}

#[test]
fn detects_managed_quick_tunnel_mcp_urls() {
    for url in [
        "https://bright-demo.trycloudflare.com/mcp",
        "https://bright-demo.localhost.run/mcp",
        "https://5d993e65a9d400.lhr.life/mcp",
        "https://rndm-abcd1234.pinggy.link/mcp",
        "https://rndm.run.pinggy-free.link/mcp",
        "https://rndm.free.pinggy.net/mcp",
    ] {
        assert!(is_quick_tunnel_url(url), "expected quick tunnel {url}");
    }
    for url in [
        "https://admin.localhost.run/mcp",
        "https://www.localhost.run/mcp",
        "https://api.trycloudflare.com/mcp",
        "https://example.com/mcp",
        "http://127.0.0.1:8765/mcp",
    ] {
        assert!(
            !is_quick_tunnel_url(url),
            "unexpectedly treated {url} as a quick tunnel"
        );
    }
}

#[test]
fn parses_free_ssh_tunnel_urls_without_accepting_provider_hosts() {
    assert_eq!(
        extract_ssh_tunnel_url(
            TunnelProvider::LocalhostRun,
            "https://bright-demo.localhost.run tunneled with tls termination"
        )
        .as_deref(),
        Some("https://bright-demo.localhost.run")
    );
    assert_eq!(
            extract_ssh_tunnel_url(
                TunnelProvider::LocalhostRun,
                "5d993e65a9d400.lhr.life tunneled with tls termination, https://5d993e65a9d400.lhr.life"
            )
            .as_deref(),
            Some("https://5d993e65a9d400.lhr.life")
        );
    assert_eq!(
        extract_ssh_tunnel_url(
            TunnelProvider::LocalhostRun,
            "https://5d993e65a9d400.lhr.life"
        )
        .as_deref(),
        Some("https://5d993e65a9d400.lhr.life")
    );
    assert_eq!(
        extract_ssh_tunnel_url(
            TunnelProvider::LocalhostRun,
            "To set up and manage custom domains go to https://admin.localhost.run/"
        ),
        None
    );
    assert_eq!(
        extract_ssh_tunnel_url(
            TunnelProvider::LocalhostRun,
            "https://admin.localhost.run tunneled with tls termination"
        ),
        None
    );
    assert_eq!(
        extract_ssh_tunnel_url(TunnelProvider::Pinggy, "Host: rndm-abcd1234.pinggy.link")
            .as_deref(),
        Some("https://rndm-abcd1234.pinggy.link")
    );
    assert_eq!(
        extract_ssh_tunnel_url(
            TunnelProvider::Pinggy,
            "Forwarding HTTPS traffic from https://rndm.run.pinggy-free.link"
        )
        .as_deref(),
        Some("https://rndm.run.pinggy-free.link")
    );
    assert_eq!(
        extract_ssh_tunnel_url(
            TunnelProvider::Pinggy,
            "Forwarding HTTPS traffic from https://rndm.free.pinggy.net"
        )
        .as_deref(),
        Some("https://rndm.free.pinggy.net")
    );
    assert_eq!(
        extract_ssh_tunnel_url(TunnelProvider::LocalhostRun, "connect localhost.run"),
        None
    );
    assert_eq!(
        extract_ssh_tunnel_url(TunnelProvider::LocalhostRun, "connect www.localhost.run"),
        None
    );
    assert_eq!(
        extract_ssh_tunnel_url(TunnelProvider::Pinggy, "connect free.pinggy.io"),
        None
    );
    assert_eq!(
        TunnelProvider::auto_candidates(),
        vec![
            TunnelProvider::Cloudflare,
            TunnelProvider::LocalhostRun,
            TunnelProvider::Pinggy,
            TunnelProvider::Tailscale
        ]
    );
    assert!(TunnelProvider::DevTunnel.has_stable_endpoint());
    assert!(!TunnelProvider::auto_candidates().contains(&TunnelProvider::DevTunnel));
}

#[test]
fn parses_persistent_devtunnel_url_for_the_selected_port() {
    assert_eq!(
        extract_devtunnel_url(
            "Hosting port 8765 at https://demo-8765.usw2.devtunnels.ms/",
            8765
        )
        .as_deref(),
        Some("https://demo-8765.usw2.devtunnels.ms")
    );
    assert_eq!(
        extract_devtunnel_url(
            "Hosting port 8765 at https://demo-8765-inspect.usw2.devtunnels.ms/",
            8765
        ),
        None
    );
    assert_eq!(
        extract_devtunnel_url(
            "Hosting port 9999 at https://demo-9999.usw2.devtunnels.ms/",
            8765
        ),
        None
    );
}

#[test]
fn provisioning_rate_limits_get_a_cooldown_without_delaying_transport_retries() {
    for error in [
        "quick tunnel provisioning failed with status 429: error code: 1015",
        "cloudflared exited before producing a public URL:\nlogs\nerror code: 1015",
        "Cloudflare provisioning rate limited (HTTP 429 / error 1015)",
    ] {
        assert_eq!(
            provider_failure_cooldown(TunnelProvider::Cloudflare, error),
            Duration::from_secs(300)
        );
        assert_eq!(
            provider_failure_cooldown(TunnelProvider::Pinggy, error),
            Duration::ZERO
        );
    }
    assert_eq!(
        provider_failure_cooldown(TunnelProvider::Cloudflare, "TLS handshake timeout"),
        Duration::ZERO
    );
    assert_eq!(
        provider_failure_cooldown(
            TunnelProvider::Tailscale,
            "Funnel is not enabled on your tailnet"
        ),
        Duration::from_secs(300)
    );
}

#[cfg(unix)]
#[tokio::test]
async fn cancelled_tunnel_startup_closes_descendant_pipes_without_touching_other_children() {
    use tokio::io::AsyncReadExt;
    let mut unrelated = Command::new("sleep")
        .arg("30")
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let (ready_tx, ready_rx) = oneshot::channel();
    let worker = tokio::spawn(async move {
        let mut command = Command::new("sh");
        command
            .args(["-c", "sleep 30 & echo ready; wait"])
            .stdout(StdStdio::piped())
            .stderr(StdStdio::null());
        let mut child = TunnelChild::spawn(&mut command).unwrap();
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        stdout.read_line(&mut line).await.unwrap();
        assert_eq!(line.trim(), "ready");
        ready_tx.send(stdout).unwrap();
        std::future::pending::<()>().await;
        drop(child);
    });
    let mut stdout = timeout(Duration::from_secs(5), ready_rx)
        .await
        .unwrap()
        .unwrap();
    worker.abort();
    let _ = worker.await;
    let mut remaining = String::new();
    let closed = timeout(
        Duration::from_secs(3),
        stdout.read_to_string(&mut remaining),
    )
    .await;
    assert!(
        closed.is_ok(),
        "an orphaned grandchild still holds the pipe open"
    );
    assert!(
        unrelated.try_wait().unwrap().is_none(),
        "unrelated process was killed"
    );
    unrelated.kill().await.unwrap();
}
