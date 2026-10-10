//! Validate the advertised MCP contract against real local responses; no native workers.
use luna_factoryd::{config::Config, http::McpServer, lifecycle::Factory, mcp::tool_definitions};
use serde_json::{Value, json};
use std::{path::Path, process::Command, sync::Arc};

fn repository(path: &Path) {
    std::fs::create_dir_all(path).unwrap();
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
            "fixture",
        ],
    ] {
        assert!(
            Command::new("git")
                .current_dir(path)
                .args(args)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
}
fn setup(profiles: bool) -> (tempfile::TempDir, McpServer) {
    let temp = tempfile::tempdir().unwrap();
    repository(&temp.path().join("repo"));
    repository(&temp.path().join("discovery/sample"));
    let config: Config = serde_json::from_value(json!({
        "listen":"127.0.0.1:8787", "database":temp.path().join("state/runs.sqlite"),
        "codex_binary":"/absent-output-schema-native", "skill_path":temp.path().join("SKILL.md"),
        "repositories":{"test":{"root":temp.path().join("repo"),"max_finish":"local_candidate"}},
        "discovery_roots":{"local":{"root":temp.path().join("discovery"),"max_finish":"local_candidate"}},
        "profiles":if profiles {json!({"default":{"effort":"low"}})} else {json!({})},
        "limits":{"capacity":2,"repair_attempts":2,"wall_seconds":600}
    })).unwrap();
    let server = McpServer {
        factory: Factory::new(config).unwrap(),
        html: Arc::new(String::new()),
    };
    (temp, server)
}
fn validator(name: &str) -> jsonschema::Validator {
    let tool = tool_definitions()
        .into_iter()
        .find(|t| t.name == name)
        .unwrap();
    let schema = serde_json::to_value(
        tool.output_schema
            .expect("every structured tool advertises an output schema"),
    )
    .unwrap();
    assert_eq!(schema["type"], "object", "{name}");
    jsonschema::validator_for(&schema).unwrap()
}
fn valid(name: &str, output: &Value) {
    let validator = validator(name);
    let errors: Vec<_> = validator
        .iter_errors(output)
        .map(|e| e.to_string())
        .collect();
    assert!(errors.is_empty(), "{name}: {}", errors.join("; "));
}
async fn invoke(server: &McpServer, name: &str, args: Value) -> Value {
    let output = server.invoke(name, args).await.unwrap();
    valid(name, &output);
    output
}
#[test]
fn every_tool_has_a_concrete_valid_schema() {
    let tools = tool_definitions();
    assert_eq!(tools.len(), 26);
    for tool in tools {
        let schema = serde_json::to_value(tool.output_schema.as_ref().unwrap()).unwrap();
        assert!(
            !schema["required"].as_array().unwrap().is_empty(),
            "{}",
            tool.name
        );
        assert!(!validator(&tool.name).is_valid(&json!({})), "{}", tool.name);
    }
}
#[tokio::test]
async fn read_settings_onboarding_and_graph_responses_match_their_contracts() {
    let (_temp, server) = setup(true);
    for name in [
        "get_factory_capabilities",
        "get_factory_backends",
        "read_factory_settings",
        "list_factory_runs",
        "open_factory",
        "open_factory_panel",
        "refresh_factory",
    ] {
        invoke(&server, name, json!({})).await;
    }
    invoke(
        &server,
        "update_factory_settings",
        json!({"set":{"capacity":2,"finish":"local_candidate","profile":"default"}}),
    )
    .await;
    let discovery = invoke(&server, "discover_factory_repositories", json!({})).await;
    invoke(&server,"request_factory_repository",json!({"candidate_id":discovery["candidates"][0]["id"],"alias":"discovered","max_finish":"local_candidate"})).await;
    invoke(&server, "discover_factory_repositories", json!({})).await;
    let graph = invoke(&server,"create_factory_graph",json!({
        "repository":"test","objective":"Validate typed outputs","acceptance":["A1: contract valid"],"non_goals":[],
        "finish":"local_candidate","profile":"default","capacity":1,"repair_attempts":1,"wall_seconds":60,"idempotency_key":"schemas"
    })).await;
    let run_id = graph["graph"]["run_id"].clone();
    let run = invoke(&server, "get_factory_run", json!({"run_id":run_id})).await;
    invoke(&server, "list_factory_runs", json!({})).await;
    invoke(&server, "refresh_factory", json!({"run_id":run_id})).await;
    invoke(&server, "get_factory_graph", json!({"run_id":run_id})).await;
    let proposal = invoke(&server,"propose_factory_change",json!({"run_id":run_id,"expected_revision":graph["graph"]["revision"],"idempotency_key":"import","change":{"kind":"import_candidates","nodes":[{"id":"candidate","title":"Check contract","criterion_ids":["A1"],"dependencies":["objective"],"source":{"provider":"local","repository_id":graph["graph"]["repository"]["identity"],"item_id":"candidate","revision":"v1"}}]}})).await;
    let applied = invoke(&server,"apply_factory_change",json!({"run_id":run_id,"change_id":proposal["proposal"]["id"],"expected_revision":proposal["graph"]["revision"]})).await;
    // Mutation tools return this same public run shape; planning cannot dispatch them.
    for name in [
        "start_factory",
        "steer_factory_run",
        "cancel_factory_run",
        "resume_factory_run",
        "reconcile_factory_run",
    ] {
        valid(name, &run);
    }
    for (pointer, wrong) in [
        ("/control/tasks/0/state", json!("invented")),
        ("/control/criteria/0/status", json!(false)),
        ("/presentation/primary_action/allowed", json!("yes")),
        ("/route/requested_model", json!(42)),
    ] {
        let mut bad = run.clone();
        *bad.pointer_mut(pointer).unwrap() = wrong;
        assert!(!validator("get_factory_run").is_valid(&bad), "{pointer}");
    }
    let mut bad = run.clone();
    bad["control"]["tasks"][0]
        .as_object_mut()
        .unwrap()
        .remove("criterion_ids");
    assert!(!validator("get_factory_run").is_valid(&bad));
    let mut bad = applied;
    bad["graph"]["nodes"][0]["dependencies"] = json!([12]);
    assert!(!validator("get_factory_graph").is_valid(&bad));
}
#[tokio::test]
async fn settings_contract_supports_empty_profile_configuration_and_rejects_wrong_types() {
    let (_temp, server) = setup(false);
    let settings = invoke(&server, "read_factory_settings", json!({})).await;
    assert!(settings["values"].get("profile").is_none());
    invoke(
        &server,
        "update_factory_settings",
        json!({"set":{"capacity":2}}),
    )
    .await;
    invoke(&server, "open_factory", json!({})).await;
    let mut bad = settings;
    bad["values"]["capacity"] = json!("2");
    assert!(!validator("read_factory_settings").is_valid(&bad));
    let mut bad = invoke(&server, "get_factory_backends", json!({})).await;
    bad["targets"][0]["operations"]["stop"]["qualified"] = json!("unknown");
    assert!(!validator("get_factory_backends").is_valid(&bad));
}

#[test]
fn public_run_contract_covers_legacy_nulls_and_lifecycle_projections() {
    use luna_factoryd::{
        control::{Attempt, Control, RunControl, Settlement, TaskState},
        lifecycle::public_run,
        store::{ObservedClaim, PendingDecision, Run},
    };
    // The legacy fixture intentionally lacks the optional control ledger and routing evidence.
    let mut run: Run = serde_json::from_value(json!({"id":"run","request":{"repository":"test","objective":"work","acceptance":["passes"],"non_goals":[],"finish":"local_candidate","profile":"default","capacity":1,"repair_attempts":1,"wall_seconds":60,"idempotency_key":"key"},"canonical_root":"/private/repo","repository_identity":"/private/repo/.git","state":"RUNNING","base_head":"abc","current_subject":"abc:123","thread_id":"owner","turn_id":"turn","owned_threads":["child"],"generation":1,"repairs_used":0,"created_at":0,"updated_at":0,"deadline_at":60,"delta":"Started","blocker":null,"observed_model":null,"observed_effort":null,"configured_model":"gpt-6-luna","configured_effort":"low","claim_held":true})).unwrap();
    valid("get_factory_run", &public_run(&run));
    run.control = Some(Control::new(&run.current_subject, &run.request.acceptance, 1, 60).unwrap());
    run.pending_decision = Some(PendingDecision {
        id: "decision".into(),
        question: "Which option?".into(),
    });
    run.route_observations.push(serde_json::from_value(json!({"thread_id":"owner","turn_id":"turn","from_model":"gpt-6-luna","to_model":"gpt-6-sol","reason":"model_unavailable","source":"model/rerouted"})).unwrap());
    run.observed_model = Some("gpt-6-sol".into());
    for state in [
        RunControl::Starting,
        RunControl::Running,
        RunControl::NeedsInput,
        RunControl::Verifying,
        RunControl::Blocked,
        RunControl::Interrupted,
        RunControl::Cancelling,
        RunControl::Cancelled,
        RunControl::Quiescent,
        RunControl::Converged,
        RunControl::Failed,
        RunControl::Unknown,
    ] {
        run.control.as_mut().unwrap().run_control = state;
        for settlement in [Settlement::Live, Settlement::Stopped, Settlement::Unknown] {
            let control = run.control.as_mut().unwrap();
            control.settlement = settlement;
            control.owner_liveness = settlement;
            control.child_liveness.insert("child".into(), settlement);
            valid("get_factory_run", &public_run(&run));
        }
    }
    for task_state in [
        TaskState::Candidate,
        TaskState::Ready,
        TaskState::Running,
        TaskState::Verify,
        TaskState::Done,
        TaskState::Blocked,
    ] {
        run.control
            .as_mut()
            .unwrap()
            .tasks
            .get_mut("objective")
            .unwrap()
            .state = task_state;
        valid("get_factory_run", &public_run(&run));
    }
    for phase in ["intent_unknown", "active", "returned", "stopped"] {
        let control = run.control.as_mut().unwrap();
        control.attempts = vec![Attempt {
            id: "attempt".into(),
            task_id: "objective".into(),
            parent: None,
            intent_generation: 1,
            dispatch_generation: 1,
            source_subject: run.current_subject.clone(),
            assumptions: Default::default(),
            phase: phase.into(),
            turn_id: Some("turn".into()),
            certified_before: 0,
            diagnosis: None,
        }];
        valid("get_factory_run", &public_run(&run));
    }
    for claim in [
        ObservedClaim::Owned,
        ObservedClaim::Released,
        ObservedClaim::Foreign,
        ObservedClaim::Unknown,
    ] {
        run.observed_claim = claim;
        valid("get_factory_run", &public_run(&run));
    }
    let output = public_run(&run);
    let mut bad = output.clone();
    bad["control"]["attempts"][0]["intent_generation"] = json!(-1);
    assert!(!validator("get_factory_run").is_valid(&bad));
    let mut bad = output.clone();
    bad["control"]["tasks"] = json!(vec![output["control"]["tasks"][0].clone(); 129]);
    assert!(!validator("get_factory_run").is_valid(&bad));
    let mut bad = output;
    bad.as_object_mut().unwrap().remove("route");
    assert!(!validator("get_factory_run").is_valid(&bad));
}
