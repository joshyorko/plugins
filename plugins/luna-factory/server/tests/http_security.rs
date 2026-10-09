use luna_factoryd::http::allowed_http;

#[test]
fn loopback_host_and_origin_are_required_without_cors_wildcards() {
    assert!(allowed_http(Some("127.0.0.1:8787"), None, "127.0.0.1:8787"));
    assert!(allowed_http(
        Some("localhost:8787"),
        Some("http://localhost:8787"),
        "127.0.0.1:8787"
    ));
    for host in [
        None,
        Some("evil.invalid"),
        Some("127.0.0.1.evil.invalid:8787"),
        Some("127.0.0.1:9999"),
    ] {
        assert!(!allowed_http(host, None, "127.0.0.1:8787"));
    }
    assert!(!allowed_http(
        Some("127.0.0.1:8787"),
        Some("https://evil.invalid"),
        "127.0.0.1:8787"
    ));
    assert!(!allowed_http(
        Some("127.0.0.1:8787"),
        Some("null"),
        "127.0.0.1:8787"
    ));
}
