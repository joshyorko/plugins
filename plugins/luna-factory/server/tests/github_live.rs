//! Opt-in, read-only live check of the operator's GitHub App. It never runs by default
//! (ignored, and additionally gated on `LUNA_GITHUB_LIVE=1`), so CI never contacts GitHub.
//!
//! ```sh
//! LUNA_GITHUB_LIVE=1 LUNA_GITHUB_KEY=/absolute/path/to/app.pem \
//!   cargo test --locked --test github_live -- --ignored --nocapture
//! ```
//!
//! Optional overrides: `LUNA_GITHUB_APP_ID` (default 5266798), `LUNA_GITHUB_INSTALLATION_ID`
//! (default 170065829), `LUNA_GITHUB_REPOSITORY` (default joshyorko/plugins) and
//! `LUNA_GITHUB_PARENT` (default 67). The key must be a mode-0600 PKCS#1 PEM file.
//!
//! It uses a disposable in-process server: a temporary SQLite ledger and a temporary empty Git
//! repository whose `origin` names the GitHub repository. It calls `inspect_factory_issue_graph`,
//! imports the candidates into a disposable planning graph and calls `read_factory_delivery`.
//! It prints only bounded counts and statuses. It never prints the key, its path, the app JWT or
//! installation tokens, and it asserts that none of them appear in any tool output.
use luna_factoryd::{config::Config, http::McpServer, lifecycle::Factory};
use serde_json::{Value, json};
use std::{process::Command, sync::Arc};

fn env(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_owned())
}
fn assert_redacted(value: &Value, key_path: &str) {
    let text = value.to_string();
    for forbidden in ["ghs_", "ghu_", "eyJ", "PRIVATE KEY", key_path] {
        assert!(
            !text.contains(forbidden),
            "credential material appeared in a tool output"
        );
    }
}
fn summary(value: &Value) -> String {
    let count = |key: &str| value[key].as_array().map_or(0, Vec::len);
    format!(
        "status={} reason={} nodes={} acceptance_candidates={} cycles={} external_dependencies={} cross_repository={} omitted={} truncated={} import_ready={}",
        value["status"],
        value["reason"],
        count("nodes"),
        count("acceptance_candidates"),
        count("cycles"),
        count("external_dependencies"),
        count("cross_repository"),
        count("omitted"),
        value["truncated"],
        value["import"]["ready"]
    )
}

#[tokio::test]
#[ignore = "opt-in live GitHub check: set LUNA_GITHUB_LIVE=1 and LUNA_GITHUB_KEY, then pass --ignored"]
async fn live_read_only_github_app_check() {
    if std::env::var("LUNA_GITHUB_LIVE").as_deref() != Ok("1") {
        eprintln!("skipped: LUNA_GITHUB_LIVE is not 1");
        return;
    }
    let key_path = std::env::var("LUNA_GITHUB_KEY").expect("LUNA_GITHUB_KEY must name the key");
    assert!(
        std::path::Path::new(&key_path).is_absolute(),
        "LUNA_GITHUB_KEY must be absolute"
    );
    let repository = env("LUNA_GITHUB_REPOSITORY", "joshyorko/plugins");
    let parent: u64 = env("LUNA_GITHUB_PARENT", "67").parse().unwrap();
    let app_id: u64 = env("LUNA_GITHUB_APP_ID", "5266798").parse().unwrap();
    let installation: u64 = env("LUNA_GITHUB_INSTALLATION_ID", "170065829")
        .parse()
        .unwrap();

    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    for args in [
        vec!["init", "-q"],
        vec![
            "-c",
            "user.name=Luna live check",
            "-c",
            "user.email=live-check@example.invalid",
            "-c",
            "core.hooksPath=/dev/null",
            "commit",
            "--allow-empty",
            "-qm",
            "disposable",
        ],
    ] {
        assert!(
            Command::new("git")
                .current_dir(&repo)
                .args(args)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    let origin = format!("https://github.com/{repository}.git");
    assert!(
        Command::new("git")
            .current_dir(&repo)
            .args(["remote", "add", "origin", &origin])
            .output()
            .unwrap()
            .status
            .success()
    );
    let config: Config = serde_json::from_value(json!({
        "listen":"127.0.0.1:8787","database":temp.path().join("state/runs.sqlite"),
        "codex_binary":"/absent-codex-live-github-check","skill_path":temp.path().join("SKILL.md"),
        "repositories":{"live":{"root":repo,"max_finish":"local_candidate"}},
        "profiles":{"default":{"effort":"low"}},
        "limits":{"capacity":1,"repair_attempts":0,"wall_seconds":300},
        "github_app":{"app_id":app_id,"installation_id":installation,"private_key_path":key_path,
            "allowed_repositories":[repository]}
    }))
    .unwrap();
    config
        .github_app
        .as_ref()
        .unwrap()
        .validate()
        .expect("github_app configuration is invalid");
    let server = McpServer {
        factory: Factory::new(config).unwrap(),
        html: Arc::new(String::new()),
    };
    let capabilities = server
        .invoke("get_factory_capabilities", json!({}))
        .await
        .unwrap();
    println!("capabilities.github = {}", capabilities["github"]);
    assert_eq!(capabilities["github"]["configured"], true);

    let inspection = server
        .invoke(
            "inspect_factory_issue_graph",
            json!({"repository":repository,"parent":parent}),
        )
        .await
        .unwrap();
    assert_redacted(&inspection, &key_path);
    println!("inspect_factory_issue_graph: {}", summary(&inspection));
    assert_eq!(
        inspection["status"], "available",
        "{}",
        inspection["reason"]
    );

    let graph = server.invoke("create_factory_graph", json!({
        "repository":"live","objective":format!("Disposable live check of {repository}#{parent}"),
        "acceptance":["Disposable live check only"],"non_goals":["No execution"],"finish":"local_candidate",
        "profile":"default","capacity":1,"repair_attempts":0,"wall_seconds":300,"idempotency_key":"live-github-check"
    })).await.unwrap();
    let run_id = graph["graph"]["run_id"].clone();
    let identity = graph["graph"]["repository"]["identity"].clone();
    let nodes: Vec<Value> = inspection["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| {
            json!({"id":n["id"],"title":n["title"],"criterion_ids":["A1"],"dependencies":n["dependencies"],
            "source":{"provider":"github","repository_id":identity,"item_id":n["source"]["item_id"],"revision":n["source"]["revision"]}})
        })
        .collect();
    if nodes.is_empty() || !inspection["cycles"].as_array().unwrap().is_empty() {
        println!("no importable candidates; delivery read skipped");
        return;
    }
    let proposed = server
        .invoke(
            "propose_factory_change",
            json!({"run_id":run_id,"expected_revision":graph["graph"]["revision"],
        "idempotency_key":"live-import","change":{"kind":"import_candidates","nodes":nodes}}),
        )
        .await
        .unwrap();
    server
        .invoke(
            "apply_factory_change",
            json!({"run_id":run_id,"change_id":proposed["proposal"]["id"],
        "expected_revision":proposed["graph"]["revision"]}),
        )
        .await
        .unwrap();
    let before = server
        .invoke("get_factory_run", json!({"run_id":run_id}))
        .await
        .unwrap();

    let delivery = server
        .invoke("read_factory_delivery", json!({"run_id":run_id}))
        .await
        .unwrap();
    assert_redacted(&delivery, &key_path);
    let statuses = delivery["nodes"].as_array().unwrap().iter().fold(
        std::collections::BTreeMap::<String, usize>::new(),
        |mut acc, n| {
            *acc.entry(format!("{}/{}", n["status"], n["reason"]))
                .or_default() += 1;
            acc
        },
    );
    let pulls: usize = delivery["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["pull_requests"].as_array().map_or(0, Vec::len))
        .sum();
    println!(
        "read_factory_delivery: available={} reason={} node_statuses={statuses:?} pull_requests={pulls}",
        delivery["available"], delivery["reason"]
    );
    let after = server
        .invoke("get_factory_run", json!({"run_id":run_id}))
        .await
        .unwrap();
    assert_eq!(before["control"], after["control"]);
    assert_eq!(
        before["presentation"]["result"],
        after["presentation"]["result"]
    );
    assert_eq!(
        before["presentation"]["actions"],
        after["presentation"]["actions"]
    );
}
