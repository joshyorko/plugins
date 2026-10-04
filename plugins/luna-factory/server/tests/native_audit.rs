//! The audit is read-only and its public report must never include raw payloads.
use std::process::Command;

#[test]
fn audit_projection_keeps_only_exact_luna_capability_and_sanitized_blocker() {
    let script = std::path::Path::new(file!())
        .canonicalize()
        .unwrap()
        .parent()
        .unwrap()
        .join("../../scripts/audit_app_server.py");
    let code = r#"
import importlib.util, sys
spec = importlib.util.spec_from_file_location('native_audit', sys.argv[1])
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)
raw = {'data':[{'model':'gpt-6-luna','supportedReasoningEfforts':[{'reasoningEffort':'max'},{'reasoningEffort':'fake-secret'}]}, {'model':'fake-secret'}]}
report = audit.catalog_projection(raw)
assert report == {'exact_luna_available': True, 'supported_efforts': ['max'], 'routing_evidence': 'catalog_only'}
assert audit.classify_stderr(b'Error: failed to initialize sqlite state runtime under /private/fake-secret') == 'state_runtime_initialization_failed'
assert 'fake-secret' not in str(report)
"#;
    let out = Command::new("python3")
        .args(["-c", code])
        .arg(script)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
