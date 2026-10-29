//! Disposable source/SQLite/transport proofs only. Never operator qualification.
#[allow(dead_code)]
mod support;
use luna_factoryd::{
    campaign::{CreateCampaign, PromoteCampaign},
    config::Config,
    graph::{ApplyChange, GraphChange, ProposeChange},
    lifecycle::Factory,
    store::Store,
};
use serde_json::json;

fn fixture() -> (tempfile::TempDir, Config, CreateCampaign) {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    for args in [
        vec!["init", "-q"],
        vec![
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@localhost",
            "commit",
            "--allow-empty",
            "-qm",
            "fixture",
        ],
    ] {
        assert!(
            std::process::Command::new("git")
                .args(args)
                .current_dir(&repo)
                .status()
                .unwrap()
                .success()
        );
    }
    let binary = temp.path().join("native-fixture");
    std::fs::copy(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/fixtures/native/fake_factory.py"),
        &binary,
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let skill = temp.path().join("skill/SKILL.md");
    std::fs::create_dir(skill.parent().unwrap()).unwrap();
    std::fs::write(&skill, "Synthetic fixture only").unwrap();
    let config: Config = serde_json::from_value(json!({"listen":"127.0.0.1:8787","database":temp.path().join("state/runs.sqlite"),"codex_binary":binary,"skill_path":skill,
        "repositories":{"test":{"root":repo,"max_finish":"local_candidate"}},"profiles":{"default":{"effort":"high"}},"limits":{"capacity":1,"repair_attempts":1,"wall_seconds":300}})).unwrap();
    let create = serde_json::from_value(json!({"repository":"test","parent":{"forge":{"provider":"fixture","repository":"local/canary","number":71},"revision":"fixture-v1"},"title":"Execution fixture","objective":"Original objective","acceptance":["Current bounded proof"],"non_goals":["No live inference"],"finish":"local_candidate","profile":"default","idempotency_key":"campaign"})).unwrap();
    (temp, config, create)
}
fn promote(campaign_id: &str, revision: u64, key: &str) -> PromoteCampaign {
    serde_json::from_value(
        json!({"campaign_id":campaign_id,"expected_revision":revision,"idempotency_key":key}),
    )
    .unwrap()
}
fn rows(config: &Config) -> (i64, i64, i64) {
    let db = rusqlite::Connection::open(&config.database).unwrap();
    let count = |table: &str| {
        db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    };
    (count("runs"), count("claims"), count("campaign_promotions"))
}
#[tokio::test]
async fn exact_reviewed_plan_becomes_a_new_run_and_replay_never_readmits() {
    let (_temp, config, create) = fixture();
    let factory = Factory::new(config.clone()).unwrap();
    let created = factory.create_campaign(create).await.unwrap();
    let campaign = created["campaign"]["id"].as_str().unwrap();
    let plan_id = created["graph"]["run_id"].as_str().unwrap();
    // Atomic nullable edits are unchanged. Explicitly replace the objective, and import
    // a prerequisite so promotion must select a ready leaf rather than the objective.
    let identity = created["graph"]["repository"]["identity"].as_str().unwrap();
    let changes: Vec<GraphChange> = vec![
        serde_json::from_value(json!({"kind":"import_candidates","nodes":[{"id":"leaf","title":"Approved prerequisite","criterion_ids":["A1"],"dependencies":[],"source":{"provider":"fixture","repository_id":identity,"item_id":"71","revision":"v1"}}]})).unwrap(),
        serde_json::from_value(json!({"kind":"update_task","node_id":"objective","title":"Exact reviewed objective","dependencies":["leaf"],"target_id":"native-local"})).unwrap(),
    ];
    let mut revision = 0;
    for (index, change) in changes.into_iter().enumerate() {
        let proposed = factory
            .propose_graph_change(ProposeChange {
                run_id: plan_id.into(),
                expected_revision: revision,
                idempotency_key: format!("edit-{index}"),
                change,
            })
            .await
            .unwrap();
        revision += 1;
        factory
            .apply_graph_change(ApplyChange {
                run_id: plan_id.into(),
                change_id: proposed["proposal"]["id"].as_str().unwrap().into(),
                expected_revision: revision,
            })
            .await
            .unwrap();
        revision += 1;
    }
    let mut store = Store::open(&config).unwrap();
    let before = serde_json::to_value(store.get(plan_id).unwrap()).unwrap();
    let request = promote(campaign, revision, "approval");
    let admission = store.admit_promotion(&config, &request).unwrap();
    let run = admission.run;
    assert!(admission.created && !run.planning_only && run.claim_held);
    assert_ne!(run.id, plan_id);
    let binding = run.approved_plan.as_ref().unwrap();
    assert_eq!(binding.revision, revision);
    assert_eq!(binding.planning_run_id, plan_id);
    assert_eq!(binding.subject, run.current_subject);
    assert_eq!(binding.snapshot_sha256.len(), 64);
    assert_eq!(run.request.objective, "Exact reviewed objective");
    assert_eq!(
        run.request.acceptance,
        store.get(plan_id).unwrap().request.acceptance
    );
    let control = run.control.as_ref().unwrap();
    assert_eq!(control.selected_task, "leaf");
    assert!(control.attempts.is_empty() && control.checks.is_empty());
    assert!(control.criteria.iter().all(|c| !c.accepted));
    assert_eq!(control.tasks["objective"].dependencies, vec!["leaf"]);
    assert_eq!(rows(&config), (2, 1, 1));
    assert_eq!(
        serde_json::to_value(store.get(plan_id).unwrap()).unwrap(),
        before,
        "planning bytes unchanged"
    );
    drop(store);
    let mut restarted = Store::open(&config).unwrap();
    let replay = restarted.admit_promotion(&config, &request).unwrap();
    assert!(!replay.created);
    assert_eq!(replay.run.id, run.id);
    assert_eq!(rows(&config), (2, 1, 1));
    assert_eq!(
        restarted
            .admit_promotion(&config, &promote(campaign, revision - 1, "approval"))
            .err()
            .unwrap()
            .to_string(),
        "campaign_promotion_idempotency_conflict"
    );
}
#[tokio::test]
async fn refused_promotions_leave_no_run_claim_or_key_and_roll_back_failed_link() {
    let (_temp, config, create) = fixture();
    let factory = Factory::new(config.clone()).unwrap();
    let created = factory.create_campaign(create).await.unwrap();
    let id = created["campaign"]["id"].as_str().unwrap();
    assert_eq!(
        factory
            .promote_campaign(promote(id, 0, "denied"))
            .await
            .unwrap_err()
            .to_string(),
        "execution_not_qualified"
    );
    assert_eq!(rows(&config), (1, 0, 0));
    assert_eq!(
        factory
            .promote_campaign(promote(id, 1, "stale"))
            .await
            .unwrap_err()
            .to_string(),
        "stale_revision"
    );
    let mut store = Store::open(&config).unwrap();
    let mut excessive = promote(id, 0, "too-much");
    excessive.budget = Some(
        serde_json::from_value(json!({"capacity":2,"repair_attempts":0,"wall_seconds":30}))
            .unwrap(),
    );
    assert!(store.admit_promotion(&config, &excessive).is_err());
    // Force failure after run and claim insertion. Every row must roll back.
    let db = rusqlite::Connection::open(&config.database).unwrap();
    db.execute_batch("CREATE TRIGGER fail_promotion BEFORE INSERT ON campaign_promotions BEGIN SELECT RAISE(ABORT,'fixture crash window'); END;").unwrap();
    assert!(
        store
            .admit_promotion(&config, &promote(id, 0, "rollback"))
            .is_err()
    );
    assert_eq!(rows(&config), (1, 0, 0));
    db.execute_batch("DROP TRIGGER fail_promotion").unwrap();
    assert!(
        store
            .admit_promotion(&config, &promote(id, 0, "rollback"))
            .unwrap()
            .created
    );
    assert_eq!(rows(&config), (2, 1, 1));
}
#[tokio::test]
async fn qualified_promotion_reuses_launch_and_duplicate_never_replays_native_effects() {
    let (_temp, config, create) = fixture();
    let config = support::qualified(config);
    let factory = Factory::new(config.clone()).unwrap();
    let created = factory.create_campaign(create).await.unwrap();
    let id = created["campaign"]["id"].as_str().unwrap();
    let plan = created["graph"]["run_id"].as_str().unwrap();
    let before = serde_json::to_value(Store::open(&config).unwrap().get(plan).unwrap()).unwrap();
    let first = factory
        .promote_campaign(promote(id, 0, "execute"))
        .await
        .unwrap();
    assert_eq!(first["campaign"]["status"], "promoted");
    let run_id = first["campaign"]["run_ids"][0].as_str().unwrap();
    let run = factory.get(run_id).await.unwrap();
    assert!(run["owner_thread"].is_string(), "{run}");
    let dispatch = run["control"]["attempts"].as_array().unwrap().len();
    assert_eq!(dispatch, 1, "{run}");
    let replay = factory
        .promote_campaign(promote(id, 0, "execute"))
        .await
        .unwrap();
    assert_eq!(replay["campaign"]["run_ids"], json!([run_id]));
    assert_eq!(
        factory.get(run_id).await.unwrap()["control"]["attempts"]
            .as_array()
            .unwrap()
            .len(),
        dispatch
    );
    assert_eq!(
        serde_json::to_value(Store::open(&config).unwrap().get(plan).unwrap()).unwrap(),
        before
    );
    assert_eq!(rows(&config).0, 2);
    let schema = serde_json::to_value(
        luna_factoryd::mcp::tool_definitions()
            .into_iter()
            .find(|t| t.name == "promote_factory_campaign")
            .unwrap()
            .output_schema
            .unwrap(),
    )
    .unwrap();
    assert!(
        jsonschema::validator_for(&schema)
            .unwrap()
            .is_valid(&replay)
    );
}
