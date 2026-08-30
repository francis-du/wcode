use super::cloudflare::extract_cloudflare_tunnel_url;
use super::*;

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
}
