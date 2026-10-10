use luna_factoryd::{
    config::Config,
    store::{StartRequest, Store},
};
use serde_json::json;
use tempfile::TempDir;

fn setup() -> (TempDir, Config, StartRequest) {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    for args in [
        vec!["init"],
        vec![
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
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
        "codex_binary":"/absent-codex-graph-fixture", "skill_path":temp.path().join("SKILL.md"),
        "repositories":{"test":{"root":repo,"max_finish":"local_candidate"}},
        "profiles":{"default":{"effort":"low"}},
        "limits":{"capacity":2,"repair_attempts":2,"wall_seconds":600}
    }))
    .unwrap();
    let request: StartRequest = serde_json::from_value(json!({
        "repository":"test","objective":"Verify one bounded behavior", "acceptance":["A1: test passes"],
        "non_goals":[],"finish":"local_candidate","profile":"default",
        "capacity":1,"repair_attempts":1,"wall_seconds":60,"idempotency_key":"same-request"
    })).unwrap();
    (temp, config, request)
}

use luna_factoryd::{
    graph::{ApplyChange, ProposeChange},
    lifecycle::Factory,
};

fn proposal(run: &serde_json::Value, key: &str, change: serde_json::Value) -> ProposeChange {
    serde_json::from_value(json!({"run_id":run["graph"]["run_id"],"expected_revision":run["graph"]["revision"],"idempotency_key":key,"change":change})).unwrap()
}
fn node(graph: &serde_json::Value, id: &str, dependencies: Vec<&str>) -> serde_json::Value {
    json!({"id":id,"title":"Candidate work","criterion_ids":["A1"],"dependencies":dependencies,
       "source":{"provider":"local","repository_id":graph["graph"]["repository"]["identity"],"item_id":id,"revision":"v1"}})
}

#[tokio::test]
async fn no_op_changes_reject_without_revision_or_journal_writes() {
    let (_temp, config, request) = setup();
    let factory = Factory::new(config.clone()).unwrap();
    let created = factory.create_graph(request).await.unwrap();
    let id = created["graph"]["run_id"].as_str().unwrap();
    let empty = proposal(
        &created,
        "empty-prerequisites",
        json!({"kind":"set_dependencies","node_id":"objective","dependencies":[]}),
    );
    let error = factory
        .propose_graph_change(empty.clone())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("graph_change_noop"));
    assert!(factory.propose_graph_change(empty).await.is_err());
    assert_eq!(factory.graph(id).await.unwrap(), created);
    let view = factory.get(id).await.unwrap();
    assert_eq!(view["presentation"]["result"]["kind"], "unverified");
    assert_eq!(view["planning_only"], true);
    assert_eq!(
        view["presentation"]["budget"]["time_remaining_seconds"],
        serde_json::Value::Null
    );

    let imported = factory.propose_graph_change(proposal(&created, "import", json!({"kind":"import_candidates","nodes":[node(&created,"a",vec![]),node(&created,"b",vec![]),node(&created,"c",vec!["a","b"])]}))).await.unwrap();
    let apply = ApplyChange {
        run_id: id.into(),
        change_id: imported["proposal"]["id"].as_str().unwrap().into(),
        expected_revision: 1,
    };
    let applied = factory.apply_graph_change(apply.clone()).await.unwrap();
    assert_eq!(factory.apply_graph_change(apply).await.unwrap(), applied);
    for (key, change) in [
        (
            "same-order",
            json!({"kind":"set_dependencies","node_id":"c","dependencies":["a","b"]}),
        ),
        (
            "reordered",
            json!({"kind":"set_dependencies","node_id":"c","dependencies":["b","a"]}),
        ),
    ] {
        let error = factory
            .propose_graph_change(proposal(&applied, key, change))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("graph_change_noop"));
    }
    assert_eq!(factory.graph(id).await.unwrap()["graph"], applied["graph"]);
    let mut stale = proposal(
        &applied,
        "stale",
        json!({"kind":"set_dependencies","node_id":"objective","dependencies":[]}),
    );
    stale.expected_revision = 0;
    assert!(
        factory
            .propose_graph_change(stale)
            .await
            .unwrap_err()
            .to_string()
            .contains("stale_control_revision")
    );
    let raw = Store::open(&config).unwrap().get(id).unwrap();
    assert_eq!(raw.control.as_ref().unwrap().graph_changes.len(), 1);
    assert!(
        raw.thread_id.is_none()
            && !raw.claim_held
            && raw.control.as_ref().unwrap().attempts.is_empty()
    );
}
#[tokio::test]
async fn planning_roundtrip_replay_conflicts_and_no_execution() {
    let (_temp, config, request) = setup();
    let factory = Factory::new(config.clone()).unwrap();
    let graph = factory.create_graph(request.clone()).await.unwrap();
    let id = graph["graph"]["run_id"].as_str().unwrap();
    assert_eq!(graph["graph"]["planning_only"], true);
    assert_eq!(graph["graph"]["nodes"][0]["state"], "candidate");
    assert_eq!(graph["graph"]["claim"]["held"], false);
    assert_eq!(factory.create_graph(request.clone()).await.unwrap(), graph);
    assert!(factory.start(request).await.is_err());
    assert!(factory.resume(id).await.is_err());
    assert!(factory.cancel(id).await.is_err());
    assert!(factory.reconcile(id, None).await.is_err());
    assert!(factory.steer(id, "turn", "message").await.is_err());
    let change = proposal(
        &graph,
        "import-1",
        json!({"kind":"import_candidates","nodes":[node(&graph,"one",vec![]),node(&graph,"two",vec!["one"])]}),
    );
    let proposed = factory.propose_graph_change(change.clone()).await.unwrap();
    assert_eq!(proposed["graph"]["revision"], 1);
    assert_eq!(
        factory.propose_graph_change(change.clone()).await.unwrap(),
        proposed
    );
    let mut conflict = change.clone();
    conflict.change = serde_json::from_value(
        json!({"kind":"set_dependencies","node_id":"objective","dependencies":[]}),
    )
    .unwrap();
    assert!(factory.propose_graph_change(conflict).await.is_err());
    let apply = ApplyChange {
        run_id: id.into(),
        change_id: proposed["proposal"]["id"].as_str().unwrap().into(),
        expected_revision: 1,
    };
    let applied = factory.apply_graph_change(apply.clone()).await.unwrap();
    assert_eq!(applied["graph"]["revision"], 2);
    assert_eq!(factory.apply_graph_change(apply).await.unwrap(), applied);
    assert_eq!(applied["graph"]["nodes"].as_array().unwrap().len(), 3);
    assert!(applied["graph"]["attempts"].as_array().unwrap().is_empty());
    let raw = Store::open(&config).unwrap().get(id).unwrap();
    assert!(!raw.claim_held && raw.thread_id.is_none() && raw.dispatch_id.is_none());
    assert_eq!(raw.control.as_ref().unwrap().dispatch_generation, 0);
    assert!(
        raw.control
            .as_ref()
            .unwrap()
            .tasks
            .values()
            .all(|t| t.effects.is_empty() && t.claim == "unknown")
    );
    drop(factory);
    let restarted = Factory::new(config).unwrap();
    assert_eq!(
        restarted.graph(id).await.unwrap()["graph"],
        applied["graph"]
    );
}

#[tokio::test]
async fn historical_noop_journal_remains_readable_but_new_apply_is_rejected() {
    use luna_factoryd::control::{Event, EventEnvelope};
    let (_temp, config, request) = setup();
    let factory = Factory::new(config.clone()).unwrap();
    let created = factory.create_graph(request).await.unwrap();
    let id = created["graph"]["run_id"].as_str().unwrap();
    let request = proposal(
        &created,
        "legacy-noop",
        json!({"kind":"set_dependencies","node_id":"objective","dependencies":[]}),
    );
    let legacy = luna_factoryd::graph::Proposal {
        id: "legacy-noop".into(),
        idempotency_key: request.idempotency_key.clone(),
        fingerprint: luna_factoryd::graph::fingerprint(&request).unwrap(),
        actor: "local_operator".into(),
        base_revision: 0,
        subject: created["graph"]["repository"]["subject"]
            .as_str()
            .unwrap()
            .into(),
        change: request.change.clone(),
        status: "proposed".into(),
        applied_revision: None,
    };
    // Simulate a pre-fix event through the unchanged durable reducer, only in this disposable DB.
    let mut store = Store::open(&config).unwrap();
    let mut run = store.get(id).unwrap();
    store
        .apply_event(
            &mut run,
            &EventEnvelope {
                id: "legacy-propose".into(),
                expected_revision: 0,
                event: Event::GraphProposed { proposal: legacy },
            },
        )
        .unwrap();
    drop(store);
    drop(factory);
    let factory = Factory::new(config.clone()).unwrap();
    assert_eq!(factory.graph(id).await.unwrap()["graph"]["revision"], 1);
    let apply = ApplyChange {
        run_id: id.into(),
        change_id: "legacy-noop".into(),
        expected_revision: 1,
    };
    assert!(
        factory
            .apply_graph_change(apply.clone())
            .await
            .unwrap_err()
            .to_string()
            .contains("graph_change_noop")
    );
    assert_eq!(factory.graph(id).await.unwrap()["graph"]["revision"], 1);
    let mut store = Store::open(&config).unwrap();
    let mut run = store.get(id).unwrap();
    store
        .apply_event(
            &mut run,
            &EventEnvelope {
                id: "legacy-apply".into(),
                expected_revision: 1,
                event: Event::GraphApplied {
                    change_id: "legacy-noop".into(),
                },
            },
        )
        .unwrap();
    drop(store);
    drop(factory);
    let factory = Factory::new(config).unwrap();
    let replay = factory.apply_graph_change(apply).await.unwrap();
    assert_eq!(replay["graph"]["revision"], 2);
    assert_eq!(replay["proposal"]["status"], "applied");
}

#[tokio::test]
async fn unchanged_target_note_does_not_write_and_successful_requests_still_replay() {
    let (_temp, config, request) = setup();
    let factory = Factory::new(config).unwrap();
    let created = factory.create_graph(request).await.unwrap();
    let change = json!({"kind":"set_target","node_id":"objective","target_id":"native-local"});
    let command = proposal(&created, "target-note", change.clone());
    let proposed = factory.propose_graph_change(command.clone()).await.unwrap();
    let id = created["graph"]["run_id"].as_str().unwrap();
    let apply = ApplyChange {
        run_id: id.into(),
        change_id: proposed["proposal"]["id"].as_str().unwrap().into(),
        expected_revision: 1,
    };
    let applied = factory.apply_graph_change(apply.clone()).await.unwrap();
    assert!(
        factory
            .propose_graph_change(proposal(&applied, "unchanged-target", change))
            .await
            .unwrap_err()
            .to_string()
            .contains("graph_change_noop")
    );
    assert_eq!(factory.graph(id).await.unwrap()["graph"], applied["graph"]);
    assert_eq!(
        factory.propose_graph_change(command).await.unwrap(),
        applied
    );
    assert_eq!(factory.apply_graph_change(apply).await.unwrap(), applied);
    assert!(applied["graph"]["attempts"].as_array().unwrap().is_empty());
}
#[tokio::test]
async fn candidates_reject_foreign_sources_missing_dependencies_cycles_and_stale_revisions() {
    let (_temp, config, request) = setup();
    let factory = Factory::new(config).unwrap();
    let graph = factory.create_graph(request).await.unwrap();
    let mut foreign = node(&graph, "foreign", vec![]);
    foreign["source"]["repository_id"] = json!("foreign");
    for nodes in [
        vec![foreign],
        vec![node(&graph, "one", vec!["missing"])],
        vec![
            node(&graph, "one", vec!["two"]),
            node(&graph, "two", vec!["one"]),
        ],
    ] {
        assert!(
            factory
                .propose_graph_change(proposal(
                    &graph,
                    "invalid",
                    json!({"kind":"import_candidates","nodes":nodes})
                ))
                .await
                .is_err()
        );
    }
    let proposed = factory
        .propose_graph_change(proposal(
            &graph,
            "valid",
            json!({"kind":"import_candidates","nodes":[node(&graph,"one",vec![])]}),
        ))
        .await
        .unwrap();
    assert!(
        factory
            .propose_graph_change(proposal(
                &graph,
                "stale",
                json!({"kind":"set_dependencies","node_id":"objective","dependencies":[]})
            ))
            .await
            .is_err()
    );
    assert!(
        factory
            .apply_graph_change(ApplyChange {
                run_id: graph["graph"]["run_id"].as_str().unwrap().into(),
                change_id: proposed["proposal"]["id"].as_str().unwrap().into(),
                expected_revision: 0
            })
            .await
            .is_err()
    );
}
#[tokio::test]
async fn planning_does_not_touch_an_existing_unknown_claim() {
    let (_temp, config, mut request) = setup();
    let factory = Factory::new(config.clone()).unwrap();
    let mut store = Store::open(&config).unwrap();
    let existing = store.admit(&config, &request).unwrap().run;
    request.idempotency_key = "planning".into();
    let graph = factory.create_graph(request).await.unwrap();
    assert_eq!(graph["graph"]["claim"]["status"], "foreign");
    factory
        .propose_graph_change(proposal(
            &graph,
            "target",
            json!({"kind":"set_target","node_id":"objective","target_id":"native-local"}),
        ))
        .await
        .unwrap();
    let unchanged = store.get(&existing.id).unwrap();
    assert_eq!(
        serde_json::to_value(existing).unwrap(),
        serde_json::to_value(unchanged).unwrap()
    );
}

#[tokio::test]
async fn alias_remap_and_git_directory_replacement_cannot_rebind_graph() {
    let (temp, config, request) = setup();
    let factory = Factory::new(config.clone()).unwrap();
    let graph = factory.create_graph(request.clone()).await.unwrap();
    let change = proposal(
        &graph,
        "target",
        json!({"kind":"set_target","node_id":"objective","target_id":"native-local"}),
    );
    let old_git = config.repositories["test"].root.join(".git");
    let saved_git = temp.path().join("original-git");
    std::fs::rename(&old_git, &saved_git).unwrap();
    assert!(
        std::process::Command::new("cp")
            .args(["-a"])
            .arg(&saved_git)
            .arg(&old_git)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        factory
            .propose_graph_change(change.clone())
            .await
            .unwrap_err()
            .to_string()
            .contains("repository_identity_changed")
    );
    assert!(factory.create_graph(request).await.is_err());
    drop(factory);
    let remapped = temp.path().join("remapped");
    assert!(
        std::process::Command::new("git")
            .arg("clone")
            .arg(&config.repositories["test"].root)
            .arg(&remapped)
            .output()
            .unwrap()
            .status
            .success()
    );
    let mut new_config = config;
    new_config.repositories.get_mut("test").unwrap().root = remapped;
    let factory = Factory::new(new_config).unwrap();
    assert!(
        factory
            .propose_graph_change(change)
            .await
            .unwrap_err()
            .to_string()
            .contains("repository_alias_was_remapped")
    );
}
#[tokio::test]
async fn source_movement_and_intervening_proposals_fence_apply() {
    let (_temp, config, request) = setup();
    let factory = Factory::new(config.clone()).unwrap();
    let graph = factory.create_graph(request).await.unwrap();
    let first = factory
        .propose_graph_change(proposal(
            &graph,
            "first",
            json!({"kind":"set_target","node_id":"objective","target_id":"native-local"}),
        ))
        .await
        .unwrap();
    factory
        .propose_graph_change(proposal(
            &first,
            "second",
            json!({"kind":"import_candidates","nodes":[node(&first,"intervening",vec![])]}),
        ))
        .await
        .unwrap();
    let apply = ApplyChange {
        run_id: graph["graph"]["run_id"].as_str().unwrap().into(),
        change_id: first["proposal"]["id"].as_str().unwrap().into(),
        expected_revision: 2,
    };
    assert!(
        factory
            .apply_graph_change(apply.clone())
            .await
            .unwrap_err()
            .to_string()
            .contains("stale_graph_proposal")
    );
    std::fs::write(config.repositories["test"].root.join("changed"), "moved").unwrap();
    assert!(
        factory
            .apply_graph_change(apply)
            .await
            .unwrap_err()
            .to_string()
            .contains("stale_graph_subject")
    );
}
#[tokio::test]
async fn unknown_execution_is_read_only_and_existing_attempts_survive_planning_edits() {
    use luna_factoryd::control::{Event, EventEnvelope, RunControl, Settlement};
    let (_temp, config, request) = setup();
    let factory = Factory::new(config.clone()).unwrap();
    let mut store = Store::open(&config).unwrap();
    let mut run = store.admit(&config, &request).unwrap().run;
    let graph = factory.graph(&run.id).await.unwrap();
    assert!(
        factory
            .propose_graph_change(proposal(
                &graph,
                "unknown",
                json!({"kind":"import_candidates","nodes":[node(&graph,"one",vec![])]})
            ))
            .await
            .is_err()
    );
    store
        .apply_event(
            &mut run,
            &EventEnvelope {
                id: "fixture-dispatch".into(),
                expected_revision: 0,
                event: Event::Dispatch {
                    id: "attempt-1".into(),
                    generation: 1,
                    repair: false,
                },
            },
        )
        .unwrap();
    store
        .apply_event(
            &mut run,
            &EventEnvelope {
                id: "fixture-return".into(),
                expected_revision: 1,
                event: Event::Returned {
                    id: "attempt-1".into(),
                },
            },
        )
        .unwrap();
    run.control.as_mut().unwrap().settlement = Settlement::Stopped;
    run.control.as_mut().unwrap().owner_liveness = Settlement::Stopped;
    run.set_state(RunControl::Quiescent);
    store.release_verified(&mut run).unwrap();
    let before = store.get(&run.id).unwrap();
    let graph = factory.graph(&run.id).await.unwrap();
    let proposed = factory
        .propose_graph_change(proposal(
            &graph,
            "import",
            json!({"kind":"import_candidates","nodes":[node(&graph,"one",vec![])]}),
        ))
        .await
        .unwrap();
    let applied = factory
        .apply_graph_change(ApplyChange {
            run_id: run.id.clone(),
            change_id: proposed["proposal"]["id"].as_str().unwrap().into(),
            expected_revision: proposed["graph"]["revision"].as_u64().unwrap(),
        })
        .await
        .unwrap();
    let after = store.get(&run.id).unwrap();
    assert_eq!(
        before.control.as_ref().unwrap().attempts,
        after.control.as_ref().unwrap().attempts
    );
    assert_eq!(
        before.control.as_ref().unwrap().criteria,
        after.control.as_ref().unwrap().criteria
    );
    assert_eq!(before.deadline_at, after.deadline_at);
    assert_eq!(before.generation, after.generation);
    assert_eq!(before.repairs_used, after.repairs_used);
    assert!(
        factory
            .propose_graph_change(proposal(
                &applied,
                "attempt-edit",
                json!({"kind":"set_dependencies","node_id":"objective","dependencies":["one"]})
            ))
            .await
            .is_err()
    );
    let mut duplicate = node(&graph, "two", vec![]);
    duplicate["source"]["item_id"] = json!("one");
    duplicate["source"]["revision"] = json!("v2");
    assert!(
        factory
            .propose_graph_change(proposal(
                &applied,
                "duplicate",
                json!({"kind":"import_candidates","nodes":[duplicate]})
            ))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn dependency_and_target_changes_are_metadata_only_and_journaled_once() {
    let (_temp, config, request) = setup();
    let factory = Factory::new(config.clone()).unwrap();
    let initial = factory.create_graph(request).await.unwrap();
    let id = initial["graph"]["run_id"].as_str().unwrap().to_owned();
    let mut graph = initial.clone();
    for (index, change) in [
        json!({"kind":"import_candidates","nodes":[node(&initial,"one",vec![])]}),
        json!({"kind":"set_dependencies","node_id":"objective","dependencies":["one"]}),
        json!({"kind":"set_target","node_id":"one","target_id":"native-local"}),
    ]
    .into_iter()
    .enumerate()
    {
        let request = proposal(&graph, &format!("change-{index}"), change);
        let proposed = factory.propose_graph_change(request.clone()).await.unwrap();
        factory.propose_graph_change(request).await.unwrap();
        let apply = ApplyChange {
            run_id: id.clone(),
            change_id: proposed["proposal"]["id"].as_str().unwrap().into(),
            expected_revision: proposed["graph"]["revision"].as_u64().unwrap(),
        };
        graph = factory.apply_graph_change(apply.clone()).await.unwrap();
        assert_eq!(factory.apply_graph_change(apply).await.unwrap(), graph);
    }
    assert_eq!(graph["graph"]["revision"], 6);
    let raw = Store::open(&config).unwrap().get(&id).unwrap();
    let control = raw.control.unwrap();
    assert_eq!(control.tasks["objective"].dependencies, vec!["one"]);
    assert_eq!(control.target_preferences["one"], "native-local");
    assert_eq!(control.dispatch_generation, 0);
    assert!(control.attempts.is_empty() && control.effects.is_empty());
    let db = rusqlite::Connection::open(&config.database).unwrap();
    let count: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM control_events WHERE run_id=?1",
            [&id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 7);
}
#[tokio::test]
async fn planning_mode_and_source_binding_are_immutable_at_store_boundary() {
    let (_temp, config, request) = setup();
    let factory = Factory::new(config.clone()).unwrap();
    let graph = factory.create_graph(request).await.unwrap();
    let id = graph["graph"]["run_id"].as_str().unwrap();
    let mut store = Store::open(&config).unwrap();
    let mut run = store.get(id).unwrap();
    run.planning_only = false;
    assert!(
        store
            .save(&mut run)
            .unwrap_err()
            .to_string()
            .contains("immutable_run_contract")
    );
    let mut run = store.get(id).unwrap();
    assert!(
        store
            .reclaim(&mut run)
            .unwrap_err()
            .to_string()
            .contains("planning_graph_execution_denied")
    );
    assert!(
        store
            .apply_event(
                &mut run,
                &luna_factoryd::control::EventEnvelope {
                    id: "dispatch-denied".into(),
                    expected_revision: 0,
                    event: luna_factoryd::control::Event::Dispatch {
                        id: "attempt".into(),
                        generation: 1,
                        repair: false
                    }
                }
            )
            .is_err()
    );
    let proposed = factory
        .propose_graph_change(proposal(
            &graph,
            "import",
            json!({"kind":"import_candidates","nodes":[node(&graph,"one",vec![])]}),
        ))
        .await
        .unwrap();
    factory
        .apply_graph_change(ApplyChange {
            run_id: id.into(),
            change_id: proposed["proposal"]["id"].as_str().unwrap().into(),
            expected_revision: 1,
        })
        .await
        .unwrap();
    let mut run = store.get(id).unwrap();
    run.control
        .as_mut()
        .unwrap()
        .graph_sources
        .get_mut("one")
        .unwrap()
        .revision = "changed".into();
    assert!(
        store
            .save(&mut run)
            .unwrap_err()
            .to_string()
            .contains("immutable_graph_source")
    );
}

#[tokio::test]
async fn revoked_planning_alias_keeps_history_and_unrelated_held_run_visible() {
    let (temp, mut config, request) = setup();
    let other = temp.path().join("other");
    assert!(
        std::process::Command::new("git")
            .arg("clone")
            .arg(&config.repositories["test"].root)
            .arg(&other)
            .output()
            .unwrap()
            .status
            .success()
    );
    config.repositories.insert(
        "other".into(),
        luna_factoryd::config::Repository {
            root: other,
            max_finish: "local_candidate".into(),
        },
    );
    let factory = Factory::new(config.clone()).unwrap();
    let graph = factory.create_graph(request.clone()).await.unwrap();
    let planning_id = graph["graph"]["run_id"].as_str().unwrap().to_owned();
    let mut other_request = request;
    other_request.repository = "other".into();
    other_request.idempotency_key = "held-other".into();
    let held_id = Store::open(&config)
        .unwrap()
        .admit(&config, &other_request)
        .unwrap()
        .run
        .id;
    drop(factory);
    config.repositories.remove("test");
    let factory = Factory::new(config.clone()).unwrap();
    let store = Store::open(&config).unwrap();
    let held_before = serde_json::to_value(store.get(&held_id).unwrap()).unwrap();
    let list = factory
        .list(100)
        .await
        .expect("revoked graph must not hide unrelated runs");
    assert_eq!(list.as_array().unwrap().len(), 2);
    assert!(
        list.as_array()
            .unwrap()
            .iter()
            .any(|run| run["id"] == held_id && run["claim_held"] == true)
    );
    let planning = factory.get(&planning_id).await.unwrap();
    assert_eq!(planning["control"]["criteria"][0]["status"], "unproved");
    assert_eq!(
        planning["control"]["criteria"][0]["reason"],
        "unknown_repository"
    );
    assert_eq!(planning["control"]["tasks"][0]["state"], "candidate");
    assert_eq!(
        planning["control"]["tasks"][0]["reason"],
        "unknown_repository"
    );
    assert!(factory.graph(&planning_id).await.is_err());
    let workbench = factory.workbench(Some(&held_id)).await.unwrap();
    assert_eq!(workbench["selected_run"]["id"], held_id);
    assert_eq!(workbench["runs"].as_array().unwrap().len(), 2);
    assert_eq!(
        factory.workbench(Some(&planning_id)).await.unwrap()["selected_run"]["id"],
        planning_id
    );
    let stable = serde_json::to_value(store.get(&planning_id).unwrap()).unwrap();
    let journal = rusqlite::Connection::open(&config.database).unwrap();
    let count_before: i64 = journal
        .query_row("SELECT COUNT(*) FROM control_events", [], |row| row.get(0))
        .unwrap();
    factory.list(100).await.unwrap();
    factory.get(&planning_id).await.unwrap();
    assert_eq!(
        serde_json::to_value(store.get(&planning_id).unwrap()).unwrap(),
        stable
    );
    assert_eq!(
        serde_json::to_value(store.get(&held_id).unwrap()).unwrap(),
        held_before
    );
    let count_after: i64 = journal
        .query_row("SELECT COUNT(*) FROM control_events", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count_before, count_after);
    assert_eq!(
        store
            .claim_owner(&config.repositories["other"].root.to_string_lossy())
            .unwrap(),
        Some(held_id)
    );
}

#[tokio::test]
async fn retained_graph_evidence_is_unproved_when_current_repository_binding_fails() {
    use luna_factoryd::control::{Event, EventEnvelope, RunControl, Settlement};
    use sha2::{Digest, Sha256};
    let (_temp, mut config, request) = setup();
    let root = config.repositories["test"].root.clone();
    std::fs::write(root.join("result.txt"), "expected").unwrap();
    let factory = Factory::new(config.clone()).unwrap();
    let mut store = Store::open(&config).unwrap();
    let mut run = store.admit(&config, &request).unwrap().run;
    store
        .apply_event(
            &mut run,
            &EventEnvelope {
                id: "fixture-dispatch".into(),
                expected_revision: 0,
                event: Event::Dispatch {
                    id: "attempt".into(),
                    generation: 1,
                    repair: false,
                },
            },
        )
        .unwrap();
    store
        .apply_event(
            &mut run,
            &EventEnvelope {
                id: "fixture-return".into(),
                expected_revision: 1,
                event: Event::Returned {
                    id: "attempt".into(),
                },
            },
        )
        .unwrap();
    run.control.as_mut().unwrap().settlement = Settlement::Stopped;
    run.control.as_mut().unwrap().owner_liveness = Settlement::Stopped;
    run.set_state(RunControl::Quiescent);
    store.release_verified(&mut run).unwrap();
    let graph = factory.graph(&run.id).await.unwrap();
    factory
        .propose_graph_change(proposal(
            &graph,
            "history",
            json!({"kind":"import_candidates","nodes":[node(&graph,"candidate",vec![])]}),
        ))
        .await
        .unwrap();
    run = store.get(&run.id).unwrap();
    let subject = run.current_subject.clone();
    luna_factoryd::evidence::reconcile_report(run.control.as_mut().unwrap(),&json!({
       "checks":[{"id":"file-check","kind":"file_sha256","path":"result.txt","sha256":format!("{:x}",Sha256::digest(b"expected")),"native_item":null,
       "binding":{"task_id":"objective","attempt_id":"attempt","intent_generation":1,"dispatch_generation":1,"subject":subject,"assumptions":{}}}],
       "acceptance":[{"id":"A1","passed":true,"accepted":true,"check_refs":["file-check"]}]
    }),&root).unwrap();
    assert!(run.control.as_ref().unwrap().converged());
    store.save(&mut run).unwrap();
    assert_eq!(
        factory.get(&run.id).await.unwrap()["control"]["criteria"][0]["status"],
        "proven"
    );
    drop(factory);
    config.profiles.remove("default");
    let factory = Factory::new(config.clone()).unwrap();
    let before = serde_json::to_value(store.get(&run.id).unwrap()).unwrap();
    let view = factory.get(&run.id).await.unwrap();
    assert_eq!(view["control"]["criteria"][0]["status"], "unproved");
    assert_eq!(view["control"]["criteria"][0]["reason"], "unknown_profile");
    assert_ne!(view["presentation"]["result"]["kind"], "finished_verified");
    assert_eq!(view["current_subject"], before["current_subject"]);
    assert_eq!(
        factory.list(100).await.unwrap().as_array().unwrap().len(),
        1
    );
    assert_eq!(
        serde_json::to_value(store.get(&run.id).unwrap()).unwrap(),
        before
    );
}

#[cfg(unix)]
#[test]
fn final_factory_drop_releases_lease_even_during_an_inherited_pre_exec_window() {
    // A concurrent Command spawn can fork before exec closes CLOEXEC descriptors.
    // Only async-signal-safe libc calls run in this deliberately paused child.
    struct ForkWindow {
        pid: libc::pid_t,
        wake: libc::c_int,
    }
    impl Drop for ForkWindow {
        fn drop(&mut self) {
            unsafe {
                libc::close(self.wake);
                libc::waitpid(self.pid, std::ptr::null_mut(), 0);
            }
        }
    }
    let (_temp, config, _request) = setup();
    let factory = Factory::new(config.clone()).unwrap();
    let clone = factory.clone();
    let mut pipe = [0; 2];
    assert_eq!(unsafe { libc::pipe(pipe.as_mut_ptr()) }, 0);
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0);
    if pid == 0 {
        unsafe {
            libc::close(pipe[1]);
            let mut byte = 0_u8;
            libc::read(pipe[0], (&mut byte as *mut u8).cast(), 1);
            libc::_exit(0);
        }
    }
    unsafe {
        libc::close(pipe[0]);
    }
    let window = ForkWindow { pid, wake: pipe[1] };
    drop(factory);
    assert!(
        Factory::new(config.clone()).is_err(),
        "a live Factory clone must keep exclusive ownership"
    );
    drop(clone);
    let restarted = Factory::new(config);
    drop(window);
    assert!(
        restarted.is_ok(),
        "final owner drop must release the lease despite a pre-exec child: {:?}",
        restarted.err()
    );
}
