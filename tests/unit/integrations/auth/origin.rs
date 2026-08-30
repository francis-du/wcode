use super::*;

fn headers(host: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(header::HOST, host.parse().unwrap());
    headers
}

#[test]
fn selects_only_registered_request_origins() {
    let endpoints = PublicEndpoints::new("http://127.0.0.1:9999".to_owned());
    endpoints.set_primary("https://one.example".to_owned());
    endpoints.register("https://two.example".to_owned());

    assert_eq!(
        endpoints.for_headers(&headers("one.example")).as_deref(),
        Some("https://one.example")
    );
    assert_eq!(
        endpoints
            .for_headers(&headers("TWO.EXAMPLE:443"))
            .as_deref(),
        Some("https://two.example")
    );
    assert!(endpoints
        .for_headers(&headers("attacker.example"))
        .is_none());
}

#[test]
fn changing_primary_keeps_previous_origins_registered() {
    let endpoints = PublicEndpoints::new("https://one.example".to_owned());
    endpoints.set_primary("https://two.example".to_owned());
    assert_eq!(
        endpoints.for_headers(&headers("one.example")).as_deref(),
        Some("https://one.example")
    );
    assert!(
        endpoints.equivalent_mcp_resources("https://one.example/mcp", "https://two.example/mcp")
    );
    assert!(!endpoints
        .equivalent_mcp_resources("https://one.example/mcp", "https://unknown.example/mcp"));
}

#[test]
fn removed_tunnel_is_historical_only() {
    let endpoints = PublicEndpoints::new("https://old.example".to_owned());
    endpoints.set_primary("https://current.example".to_owned());
    endpoints.unregister("https://old.example");

    assert!(endpoints.for_headers(&headers("old.example")).is_none());
    assert_eq!(
        endpoints
            .for_headers(&headers("current.example"))
            .as_deref(),
        Some("https://current.example")
    );
    assert!(endpoints
        .equivalent_mcp_resources("https://old.example/mcp", "https://current.example/mcp"));
}
