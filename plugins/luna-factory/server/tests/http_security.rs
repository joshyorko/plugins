use luna_factoryd::http::allowed_http;

#[test]
fn loopback_host_and_origin_are_required_without_cors_wildcards() {
    assert!(allowed_http(
        Some("127.0.0.1:8787"),
        None,
        "127.0.0.1:8787",
        None
    ));
    assert!(allowed_http(
        Some("localhost:8787"),
        Some("http://localhost:8787"),
        "127.0.0.1:8787",
        None
    ));
    for host in [
        None,
        Some("evil.invalid"),
        Some("127.0.0.1.evil.invalid:8787"),
        Some("127.0.0.1:9999"),
    ] {
        assert!(!allowed_http(host, None, "127.0.0.1:8787", None));
    }
    assert!(!allowed_http(
        Some("127.0.0.1:8787"),
        Some("https://evil.invalid"),
        "127.0.0.1:8787",
        None
    ));
    assert!(!allowed_http(
        Some("127.0.0.1:8787"),
        Some("null"),
        "127.0.0.1:8787",
        None
    ));
}

#[test]
fn published_container_authority_is_exact_and_loopback_only() {
    let origin = "http://127.0.0.1:18788";
    assert!(allowed_http(
        Some("127.0.0.1:18788"),
        Some(origin),
        "0.0.0.0:8787",
        Some(origin)
    ));
    assert!(allowed_http(
        Some("127.0.0.1:18788"),
        None,
        "0.0.0.0:8787",
        Some(origin)
    ));
    for (host, request_origin) in [
        ("127.0.0.1:18789", Some(origin)),
        ("192.168.1.20:18788", Some(origin)),
        ("127.0.0.1:18788", Some("http://evil.invalid")),
        ("0.0.0.0:8787", None),
    ] {
        assert!(!allowed_http(
            Some(host),
            request_origin,
            "0.0.0.0:8787",
            Some(origin)
        ));
    }
}
