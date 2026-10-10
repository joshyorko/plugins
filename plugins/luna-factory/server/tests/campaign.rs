//! Campaign entity: persisted grouping of a parent item, its planning graph and runs.
//! Disposable SQLite and git fixtures only; no native executable exists here.
use luna_factoryd::{
    config::Config, graph::ApplyChange, http::McpServer, lifecycle::Factory, mcp::tool_definitions,
    store::Store,
};
use serde_json::{Value, json};
use std::sync::Arc;
use tempfile::TempDir;

fn setup() -> (TempDir, Config) {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    for args in [
        vec!["init", "-q"],
        vec![
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "core.hooksPath=/dev/null",
            "commit",
            "--allow-empty",
            "-qm",
            "init",
        ],
    ] {
        assert!(
            std::process::Command::new("git")
                .args(args)
                .current_dir(&repo)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    let config: Config = serde_json::from_value(json!({
        "listen":"127.0.0.1:8787", "database":temp.path().join("state/runs.sqlite"),
        "codex_binary":temp.path().join("native-must-not-exist"), "skill_path":temp.path().join("SKILL.md"),
        "repositories":{"test":{"root":repo,"max_finish":"local_candidate"}},
        "profiles":{"default":{"effort":"low"}},
        "limits":{"capacity":2,"repair_attempts":2,"wall_seconds":600}
    }))
    .unwrap();
    (temp, config)
}
fn server(config: &Config) -> McpServer {
    McpServer {
        factory: Factory::new(config.clone()).unwrap(),
        html: Arc::new(String::new()),
    }
}
fn request(key: &str, item: &str) -> Value {
    json!({
        "repository":"test",
        "parent":{"provider":"github","item_id":item,"revision":"sha256:recorded-v1",
            "display":{"number":67,"url":"https://github.com/example/plugins/issues/67"}},
        "title":"Shape conversation-first issue graph and mission control",
        "objective":"Plan the parent issue and its sub-issues","acceptance":["Every sub-issue is resolved by a verified change"],
        "non_goals":["No execution from this planning graph"],"finish":"local_candidate","profile":"default",
        "idempotency_key":key
    })
}
fn valid(name: &str, output: &Value) {
    let tool = tool_definitions()
        .into_iter()
        .find(|tool| tool.name == name)
        .unwrap();
    let schema = serde_json::to_value(tool.output_schema.unwrap()).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let errors: Vec<_> = validator
        .iter_errors(output)
        .map(|e| e.to_string())
        .collect();
    assert!(errors.is_empty(), "{name}: {}", errors.join("; "));
}
async fn call(server: &McpServer, name: &str, args: Value) -> Value {
    let output = server.invoke(name, args).await.unwrap();
    valid(name, &output);
    output
}
async fn rejected(server: &McpServer, name: &str, args: Value) -> String {
    server.invoke(name, args).await.unwrap_err().to_string()
}
/// Journal and campaign rows as persisted, read beside the running service.
fn ledger(config: &Config) -> (i64, i64, Vec<String>) {
    let connection = rusqlite::Connection::open(&config.database).unwrap();
    let count = |sql: &str| connection.query_row(sql, [], |r| r.get(0)).unwrap();
    let mut statement = connection
        .prepare("SELECT payload FROM campaigns ORDER BY rowid")
        .unwrap();
    let campaigns = statement
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    (
        count("SELECT count(*) FROM control_events"),
        count("SELECT count(*) FROM runs"),
        campaigns,
    )
}

#[tokio::test]
async fn creation_links_one_planning_graph_and_replays_deterministically() {
    let (_temp, config) = setup();
    let server = server(&config);
    let created = call(
        &server,
        "create_factory_campaign",
        request("campaign-67", "1148934299:I_parent67"),
    )
    .await;
    let campaign = &created["campaign"];
    let graph = &created["graph"];
    assert_eq!(campaign["status"], "planned");
    assert_eq!(campaign["repository"], "test");
    assert_eq!(campaign["finish"], "local_candidate");
    assert_eq!(campaign["run_ids"], json!([]));
    assert_eq!(campaign["runs"], json!([]));
    assert_eq!(campaign["parent"]["display"]["number"], 67);
    assert_eq!(campaign["planning_run_id"], graph["run_id"]);
    assert_eq!(
        campaign["planning"],
        json!({"run_id":graph["run_id"],"revision":0,"planning_only":true,
            "tasks":{"total":1,"candidate":1,"ready":0,"running":0,"verify":0,"done":0,"blocked":0}})
    );
    assert_eq!(
        campaign["promotion"],
        json!({"allowed":false,"reason":"execution_not_qualified"})
    );
    assert_eq!(graph["planning_only"], true);
    assert_eq!(graph["claim"], json!({"held":false,"status":"released"}));
    assert_eq!(graph["attempts"], json!([]));
    assert_eq!(ledger(&config).0, 1, "only the planning admission event");

    // The same key and payload replay; the planning graph is not admitted twice.
    assert_eq!(
        call(
            &server,
            "create_factory_campaign",
            request("campaign-67", "1148934299:I_parent67")
        )
        .await,
        created
    );
    // An absent display and an empty display are the same request.
    let mut bare = request("campaign-bare", "1148934299:I_bare");
    bare["parent"]["display"] = json!({});
    let first = call(&server, "create_factory_campaign", bare.clone()).await;
    bare["parent"].as_object_mut().unwrap().remove("display");
    assert_eq!(call(&server, "create_factory_campaign", bare).await, first);
    assert_eq!(first["campaign"]["parent"]["display"], Value::Null);
    let before = ledger(&config);

    // Same key, different fingerprint: rejected without any write.
    let mut changed = request("campaign-67", "1148934299:I_parent67");
    changed["title"] = json!("A different title");
    assert!(
        rejected(&server, "create_factory_campaign", changed)
            .await
            .contains("campaign_idempotency_conflict")
    );
    changed = request("campaign-67", "1148934299:I_parent67");
    changed["parent"]["revision"] = json!("sha256:recorded-v2");
    assert!(
        rejected(&server, "create_factory_campaign", changed)
            .await
            .contains("campaign_idempotency_conflict")
    );
    // One campaign per repository identity and parent item, whatever the key or revision.
    let mut duplicate = request("campaign-67-again", "1148934299:I_parent67");
    duplicate["parent"]["revision"] = json!("sha256:recorded-v2");
    assert!(
        rejected(&server, "create_factory_campaign", duplicate)
            .await
            .contains("campaign_parent_exists")
    );
    assert_eq!(ledger(&config), before, "rejections leave no orphan graph");

    let listed = call(&server, "list_factory_campaigns", json!({})).await;
    let ids: Vec<_> = listed["campaigns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].clone())
        .collect();
    assert_eq!(
        ids,
        vec![first["campaign"]["id"].clone(), campaign["id"].clone()]
    );
    assert_eq!(
        call(&server, "list_factory_campaigns", json!({"limit":1})).await["campaigns"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let read = call(
        &server,
        "get_factory_campaign",
        json!({"campaign_id":campaign["id"]}),
    )
    .await;
    assert_eq!(read["campaign"], *campaign);
    assert!(
        rejected(
            &server,
            "get_factory_campaign",
            json!({"campaign_id":"missing"})
        )
        .await
        .contains("campaign_not_found")
    );

    // Persisted: a restarted service reads the same campaign.
    drop(server);
    let restarted = self::server(&config);
    assert_eq!(
        call(
            &restarted,
            "get_factory_campaign",
            json!({"campaign_id":campaign["id"]})
        )
        .await,
        read
    );
}

#[tokio::test]
async fn invalid_parents_titles_and_authority_are_rejected_before_any_write() {
    let (_temp, config) = setup();
    let server = server(&config);
    let base = request("bad", "1148934299:I_bad");
    let cases: Vec<(Value, &str)> = vec![
        (json!({"finish":"pr"}), "authority_exceeded"),
        (json!({"repository":"unknown"}), "unknown_repository"),
        (json!({"profile":"unknown"}), "unknown_profile"),
        (json!({"title":"  "}), "invalid_campaign_title"),
        (json!({"title":"x".repeat(1001)}), "invalid_campaign_title"),
        (json!({"acceptance":[]}), "invalid_acceptance"),
        (
            json!({"idempotency_key":""}),
            "invalid_campaign_idempotency_key",
        ),
        (
            json!({"parent":{"provider":"","item_id":"x","revision":"v1"}}),
            "invalid_campaign_parent",
        ),
        (
            json!({"parent":{"provider":"github","item_id":"x".repeat(257),"revision":"v1"}}),
            "invalid_campaign_parent",
        ),
        (
            json!({"parent":{"provider":"github","item_id":"x\u{7}","revision":"v1"}}),
            "invalid_campaign_parent",
        ),
        (
            json!({"parent":{"provider":"github","item_id":"x","revision":"v1","display":{"number":0}}}),
            "invalid_campaign_parent_display",
        ),
        (
            json!({"parent":{"provider":"github","item_id":"x","revision":"v1","display":{"url":"http://example.com/1"}}}),
            "invalid_campaign_parent_display",
        ),
        (
            json!({"parent":{"provider":"github","item_id":"x","revision":"v1","display":{"url":"https://example.com/a b"}}}),
            "invalid_campaign_parent_display",
        ),
        (
            json!({"parent":{"provider":"github","item_id":"x","revision":"v1","display":{"url":"javascript:alert(1)"}}}),
            "invalid_campaign_parent_display",
        ),
        (json!({"actor":"admin"}), "unknown field"),
        (json!({"capacity":8}), "unknown field"),
        (
            json!({"parent":{"provider":"github","item_id":"x","revision":"v1","repository_id":"forged"}}),
            "unknown field",
        ),
    ];
    for (patch, reason) in cases {
        let mut input = base.clone();
        for (key, value) in patch.as_object().unwrap() {
            input[key] = value.clone();
        }
        let error = rejected(&server, "create_factory_campaign", input).await;
        assert!(error.contains(reason), "{patch}: {error}");
    }
    assert_eq!(ledger(&config), (0, 0, vec![]));
    assert_eq!(
        call(&server, "list_factory_campaigns", json!({})).await,
        json!({"campaigns":[]})
    );
}

#[tokio::test]
async fn display_assertions_are_redacted_and_never_become_proof() {
    let (_temp, config) = setup();
    let server = server(&config);
    let mut input = request("redacted", "1148934299:I_secret");
    input["title"] = json!("Rotate ghp_examplecredential before release");
    input["parent"]["display"]["url"] =
        json!("https://github.com/example/plugins/issues/9?access_token=abc");
    let created = call(&server, "create_factory_campaign", input).await;
    let campaign = &created["campaign"];
    assert_eq!(campaign["parent"]["display"]["number"], 67);
    assert_eq!(campaign["parent"]["display"]["url"], Value::Null);
    assert!(
        !campaign["title"]
            .as_str()
            .unwrap()
            .to_ascii_lowercase()
            .contains("ghp_")
    );
    // A reported parent number never proves a criterion or creates a decision.
    let run = call(
        &server,
        "get_factory_run",
        json!({"run_id":campaign["planning_run_id"]}),
    )
    .await;
    assert_eq!(run["presentation"]["criteria"]["proven"], 0);
    assert_eq!(run["presentation"]["result"]["kind"], "unverified");
    assert_eq!(run["pending_decision"], Value::Null);
    assert_eq!(run["planning_only"], true);
}

#[tokio::test]
async fn promotion_fails_closed_without_events_revisions_or_records() {
    let (_temp, config) = setup();
    let server = server(&config);
    let created = call(
        &server,
        "create_factory_campaign",
        request("promote", "1148934299:I_promote"),
    )
    .await;
    let campaign_id = created["campaign"]["id"].clone();
    let run_id = created["graph"]["run_id"].as_str().unwrap().to_owned();
    // Revision fencing and planning edits are unchanged on a campaign's graph.
    let identity = created["graph"]["repository"]["identity"].clone();
    let proposal = call(&server, "propose_factory_change", json!({"run_id":run_id,"expected_revision":0,"idempotency_key":"import",
        "change":{"kind":"import_candidates","nodes":[{"id":"issue-69","title":"Projection fields","criterion_ids":["A1"],"dependencies":[],
            "source":{"provider":"github","repository_id":identity,"item_id":"1148934299:I_child69","revision":"v1"}}]}})).await;
    assert!(
        rejected(
            &server,
            "apply_factory_change",
            json!({"run_id":run_id,"change_id":proposal["proposal"]["id"],"expected_revision":0})
        )
        .await
        .contains("stale_control_revision")
    );
    let applied = server
        .factory
        .apply_graph_change(ApplyChange {
            run_id: run_id.clone(),
            change_id: proposal["proposal"]["id"].as_str().unwrap().into(),
            expected_revision: 1,
        })
        .await
        .unwrap();
    assert_eq!(applied["graph"]["revision"], 2);
    let campaign = call(
        &server,
        "get_factory_campaign",
        json!({"campaign_id":campaign_id}),
    )
    .await;
    assert_eq!(campaign["campaign"]["planning"]["revision"], 2);
    assert_eq!(campaign["campaign"]["planning"]["tasks"]["total"], 2);
    assert_eq!(campaign["campaign"]["planning"]["tasks"]["candidate"], 2);

    let capabilities = call(&server, "get_factory_capabilities", json!({})).await;
    assert_eq!(capabilities["execution"]["eligible"], false);
    let before = ledger(&config);
    let graph_before = call(&server, "get_factory_graph", json!({"run_id":run_id})).await;
    for (revision, key) in [
        (2, "promote-1"),
        (2, "promote-1"),
        (1, "promote-stale"),
        (0, "promote-1"),
        (9_007_199_254_740_991_u64, "promote-future"),
    ] {
        let error = rejected(
            &server,
            "promote_factory_campaign",
            json!({"campaign_id":campaign_id,"expected_revision":revision,"idempotency_key":key}),
        )
        .await;
        assert_eq!(error, "execution_not_qualified", "revision {revision}");
    }
    // An unknown campaign is refused at the same boundary, before any read.
    assert_eq!(
        rejected(
            &server,
            "promote_factory_campaign",
            json!({"campaign_id":"missing","expected_revision":0,"idempotency_key":"k"})
        )
        .await,
        "execution_not_qualified"
    );
    for bad in [
        json!({"campaign_id":campaign_id,"expected_revision":2}),
        json!({"campaign_id":campaign_id,"expected_revision":2,"idempotency_key":""}),
        json!({"campaign_id":campaign_id,"expected_revision":2,"idempotency_key":"k","force":true}),
        json!({"campaign_id":campaign_id,"expected_revision":-1,"idempotency_key":"k"}),
    ] {
        assert!(
            server
                .invoke("promote_factory_campaign", bad.clone())
                .await
                .is_err(),
            "{bad}"
        );
    }
    assert_eq!(ledger(&config), before, "no event, run or campaign write");
    assert_eq!(
        call(&server, "get_factory_graph", json!({"run_id":run_id})).await,
        graph_before
    );
    assert_eq!(
        call(
            &server,
            "get_factory_campaign",
            json!({"campaign_id":campaign_id})
        )
        .await,
        campaign
    );
    let raw = Store::open(&config).unwrap().get(&run_id).unwrap();
    assert!(raw.planning_only && !raw.claim_held && raw.thread_id.is_none());
    assert_eq!(raw.control.as_ref().unwrap().revision, 2);
}

#[tokio::test]
async fn a_planning_only_campaign_cannot_dispatch() {
    let (temp, config) = setup();
    let server = server(&config);
    let created = call(
        &server,
        "create_factory_campaign",
        request("dispatch", "1148934299:I_dispatch"),
    )
    .await;
    let run_id = created["campaign"]["planning_run_id"].clone();
    let before = ledger(&config);
    for (name, args) in [
        ("resume_factory_run", json!({"run_id":run_id})),
        (
            "resume_factory_run",
            json!({"run_id":run_id,"expected_revision":0}),
        ),
        (
            "steer_factory_run",
            json!({"run_id":run_id,"expected_turn_id":"turn","message":"go"}),
        ),
        ("cancel_factory_run", json!({"run_id":run_id})),
        ("reconcile_factory_run", json!({"run_id":run_id})),
    ] {
        assert!(
            server.invoke(name, args.clone()).await.is_err(),
            "{name} {args}"
        );
    }
    // The planning record's own request key can never be replayed as executable admission.
    let planning = Store::open(&config)
        .unwrap()
        .get(run_id.as_str().unwrap())
        .unwrap();
    let start = serde_json::to_value(&planning.request).unwrap();
    assert!(
        rejected(&server, "start_factory", start)
            .await
            .contains("idempotency_conflict")
    );
    assert_eq!(ledger(&config), before, "no event, run or campaign write");
    let raw = Store::open(&config)
        .unwrap()
        .get(run_id.as_str().unwrap())
        .unwrap();
    let control = raw.control.as_ref().unwrap();
    assert!(
        raw.planning_only
            && !raw.claim_held
            && raw.thread_id.is_none()
            && raw.dispatch_id.is_none()
    );
    assert!(control.attempts.is_empty() && control.effects.is_empty());
    assert_eq!(control.dispatch_generation, 0);
    assert_eq!(control.revision, 0);
    let campaign = call(
        &server,
        "get_factory_campaign",
        json!({"campaign_id":created["campaign"]["id"]}),
    )
    .await;
    assert_eq!(campaign["campaign"]["status"], "planned");
    assert_eq!(campaign["campaign"]["run_ids"], json!([]));
    assert!(!temp.path().join("native-must-not-exist").exists());
}

#[tokio::test]
async fn corrupt_or_newer_campaign_records_fail_closed() {
    let (_temp, config) = setup();
    let server = server(&config);
    let created = call(
        &server,
        "create_factory_campaign",
        request("corrupt", "1148934299:I_corrupt"),
    )
    .await;
    let id = created["campaign"]["id"].as_str().unwrap().to_owned();
    let connection = rusqlite::Connection::open(&config.database).unwrap();
    let payload: String = connection
        .query_row("SELECT payload FROM campaigns WHERE id=?1", [&id], |r| {
            r.get(0)
        })
        .unwrap();
    // This version has no promotion transition, so an execution link cannot be trusted.
    let mut forged: Value = serde_json::from_str(&payload).unwrap();
    forged["status"] = json!("promoted");
    forged["run_ids"] = json!(["forged-run"]);
    connection
        .execute(
            "UPDATE campaigns SET payload=?2 WHERE id=?1",
            rusqlite::params![id, forged.to_string()],
        )
        .unwrap();
    for (name, args) in [
        ("get_factory_campaign", json!({"campaign_id":id})),
        ("list_factory_campaigns", json!({})),
    ] {
        assert!(
            rejected(&server, name, args)
                .await
                .contains("campaign_record_corrupt")
        );
    }
    // Indexed columns must agree with the payload they constrain.
    let mut moved: Value = serde_json::from_str(&payload).unwrap();
    moved["parent"]["item_id"] = json!("1148934299:I_other");
    connection
        .execute(
            "UPDATE campaigns SET payload=?2 WHERE id=?1",
            rusqlite::params![id, moved.to_string()],
        )
        .unwrap();
    assert!(
        rejected(&server, "get_factory_campaign", json!({"campaign_id":id}))
            .await
            .contains("campaign_record_corrupt")
    );
}
