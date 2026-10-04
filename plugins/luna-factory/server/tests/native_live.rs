//! Explicit opt-in installed-binary test. Only initialize + model/list are sent.
#[allow(dead_code)]
#[path = "../src/native.rs"]
mod native;
use native::{NativeClient, validate_luna_route};
use std::path::Path;

#[tokio::test]
#[ignore = "requires explicit read-only native runtime audit; no inference"]
async fn installed_binary_initializes_and_advertises_exact_luna() {
    assert_eq!(std::env::var("LUNA_FACTORY_LIVE_AUDIT").as_deref(), Ok("1"));
    let binary =
        std::env::var("LUNA_FACTORY_CODEX_BIN").expect("trusted Codex executable required");
    let client = NativeClient::spawn(Path::new(&binary), &["app-server".into(), "--stdio".into()])
        .await
        .unwrap();
    let catalog = client.list_models().await.unwrap();
    validate_luna_route(&catalog, "high").unwrap();
    client.shutdown().await.unwrap();
}
