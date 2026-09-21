use super::*;

fn headers(host: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(header::HOST, host.parse().unwrap());
    headers
}

#[test]
fn stale_endpoint_cleanup_cannot_remove_a_new_same_url_registration() {
    let url = "https://stable.example";
    let endpoints = PublicEndpoints::new(url.to_owned());
    let old_epoch = endpoints.registration_epoch(url).unwrap();
    let worker = endpoints.clone();
    std::thread::spawn(move || worker.register(url.to_owned()))
        .join()
        .unwrap();
    let new_epoch = endpoints.registration_epoch(url).unwrap();
    assert_ne!(old_epoch, new_epoch);
    assert!(!endpoints.unregister_if_epoch(url, old_epoch));
    let mut request = headers("stable.example");
    request.insert(header::ORIGIN, url.parse().unwrap());
    assert_eq!(endpoints.for_headers(&request).as_deref(), Some(url));
    assert!(endpoints.origin_allowed(&request));
    assert!(endpoints.unregister_if_epoch(url, new_epoch));
    assert!(!endpoints.unregister_if_epoch(url, new_epoch));
    endpoints.trust_resource("https://stable.example/mcp");
    assert_eq!(endpoints.for_headers(&request).as_deref(), Some(url));
    assert!(!endpoints.origin_allowed(&request));
}

#[test]
fn primary_promotion_preserves_the_endpoint_owner_epoch() {
    let endpoints = PublicEndpoints::new("http://127.0.0.1:8765".to_owned());
    endpoints.register("https://stable.example".to_owned());
    let epoch = endpoints
        .registration_epoch("https://stable.example")
        .unwrap();
    endpoints.set_primary("https://stable.example/".to_owned());
    assert_eq!(
        endpoints.registration_epoch("https://stable.example"),
        Some(epoch)
    );
    assert!(endpoints.unregister_if_epoch("https://stable.example/", epoch));
    assert!(endpoints.for_headers(&HeaderMap::new()).is_none());
}

#[test]
fn unknown_epoch_never_removes_another_endpoint() {
    let endpoints = PublicEndpoints::new("https://one.example".to_owned());
    endpoints.register("https://two.example".to_owned());
    let epoch = endpoints.registration_epoch("https://two.example").unwrap();
    assert!(!endpoints.unregister_if_epoch("https://one.example", epoch));
    assert!(!endpoints.unregister_if_epoch("https://missing.example", epoch));
    assert!(endpoints.for_headers(&headers("one.example")).is_some());
    assert!(endpoints.for_headers(&headers("two.example")).is_some());
}

#[test]
fn browser_origins_follow_active_aliases_not_the_primary_or_token_history() {
    let endpoints = PublicEndpoints::new("http://127.0.0.1:8765".to_owned());
    endpoints.set_primary("https://one.example".to_owned());
    endpoints.register("https://two.example".to_owned());
    let mut request = headers("two.example");
    request.insert(header::ORIGIN, "https://ONE.EXAMPLE:443".parse().unwrap());
    assert!(endpoints.origin_allowed(&request));
    assert_eq!(
        endpoints.for_headers(&request).as_deref(),
        Some("https://two.example")
    );

    endpoints.unregister("https://one.example");
    assert!(!endpoints.origin_allowed(&request));
    endpoints.trust_resource("https://historical.example/mcp");
    request.insert(
        header::ORIGIN,
        "https://historical.example".parse().unwrap(),
    );
    assert!(!endpoints.origin_allowed(&request));
    request.insert(header::ORIGIN, "https://two.example/".parse().unwrap());
    assert!(endpoints.origin_allowed(&request));
    request.remove(header::ORIGIN);
    assert!(endpoints.origin_allowed(&request));
}

#[test]
fn origin_headers_reject_url_repairs_credentials_and_untrusted_origins() {
    let endpoints = PublicEndpoints::new("https://one.example".to_owned());
    for origin in [
        "null",
        "https://one.example/mcp",
        "https://user@one.example",
        "https://user:secret@one.example",
        "https://one.example/..",
        "https://one.example//",
        " https://one.example",
        "https://one.example\\",
        "https://one.example?query=value",
        "https://one.example#fragment",
        "https://one.example https://two.example",
        "https://one.example,https://two.example",
        "http://one.example",
        "https://one.example:444",
        "https://one.example.attacker.example",
        "ftp://one.example",
    ] {
        let mut request = headers("one.example");
        request.insert(header::ORIGIN, origin.parse().unwrap());
        assert!(!endpoints.origin_allowed(&request), "accepted {origin}");
    }
}

#[test]
fn malformed_and_duplicate_origin_headers_are_not_treated_as_absent() {
    let endpoints = PublicEndpoints::new("https://one.example".to_owned());
    let mut request = headers("one.example");
    request.insert(
        header::ORIGIN,
        axum::http::HeaderValue::from_bytes(&[0xff]).unwrap(),
    );
    assert!(!endpoints.origin_allowed(&request));
    request.insert(header::ORIGIN, "https://one.example".parse().unwrap());
    request.append(header::ORIGIN, "https://one.example".parse().unwrap());
    assert!(!endpoints.origin_allowed(&request));
}

#[test]
fn request_hosts_accept_custom_domains_but_reject_duplicates_and_url_syntax() {
    let endpoints = PublicEndpoints::new("https://one.example".to_owned());
    for host in [
        "one.example/",
        "one.example\\",
        "user@one.example",
        "one.example#x",
        "one.example?x",
        " one.example",
    ] {
        let mut request = headers(host);
        request.insert("x-forwarded-host", "one.example".parse().unwrap());
        request.insert("forwarded", "host=one.example;proto=https".parse().unwrap());
        assert!(endpoints.for_headers(&request).is_none(), "accepted {host}");
    }
    assert_eq!(
        endpoints
            .for_headers(&headers("custom.example:8443"))
            .as_deref(),
        Some("https://custom.example:8443")
    );
    let mut request = headers("one.example");
    request.append(header::HOST, "one.example".parse().unwrap());
    assert!(endpoints.for_headers(&request).is_none());
}

#[test]
fn missing_host_does_not_resurrect_an_unregistered_primary() {
    let endpoints = PublicEndpoints::new("https://one.example".to_owned());
    endpoints.unregister("https://one.example");
    assert!(endpoints.for_headers(&HeaderMap::new()).is_none());
}

#[test]
fn selects_registered_origins_and_accepts_custom_request_hosts() {
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
    assert_eq!(
        endpoints.for_headers(&headers("custom.example")).as_deref(),
        Some("https://custom.example")
    );
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

    assert_eq!(
        endpoints.for_headers(&headers("old.example")).as_deref(),
        Some("https://old.example")
    );
    assert_eq!(
        endpoints
            .for_headers(&headers("current.example"))
            .as_deref(),
        Some("https://current.example")
    );
    assert!(endpoints
        .equivalent_mcp_resources("https://old.example/mcp", "https://current.example/mcp"));
}
