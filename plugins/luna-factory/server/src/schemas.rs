//! Public structured-output contracts, shared by tool discovery and conformance tests.
//! Objects permit additive fields; known fields and bounded collections remain typed.
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;

fn object(properties: Value) -> Value {
    let required = properties
        .as_object()
        .expect("schema properties")
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    json!({"type":"object","properties":properties,"required":required})
}
fn optional(mut schema: Value, fields: &[&str]) -> Value {
    schema["required"]
        .as_array_mut()
        .unwrap()
        .retain(|key| !fields.contains(&key.as_str().unwrap()));
    schema
}
fn string() -> Value {
    json!({"type":"string"})
}
fn text(max: usize) -> Value {
    json!({"type":"string","maxLength":max})
}
fn id() -> Value {
    json!({"type":"string","minLength":1,"maxLength":256})
}
fn boolean() -> Value {
    json!({"type":"boolean"})
}
fn count() -> Value {
    json!({"type":"integer","minimum":0})
}
fn enumeration(values: &[&str]) -> Value {
    json!({"type":"string","enum":values})
}
fn finish() -> Value {
    enumeration(&["local_candidate", "push", "pr"])
}
fn nullable(schema: Value) -> Value {
    json!({"anyOf":[schema,{"type":"null"}]})
}
fn array(items: Value, max: usize) -> Value {
    json!({"type":"array","items":items,"maxItems":max})
}
fn reference(name: &str) -> Value {
    json!({"$ref":format!("#/$defs/{name}")})
}

fn definitions() -> Map<String, Value> {
    let mut defs = Map::new();
    defs.insert("criterion".into(),object(json!({
        "id":id(),"description":text(1000),"status":enumeration(&["proven","failed","unproved"]),
        "reason":nullable(string()),"check_refs":array(id(),32)
    })));
    defs.insert("task".into(),object(json!({
        "id":id(),"title":text(1000),"criterion_ids":array(id(),32),"dependencies":array(id(),128),
        "state":enumeration(&["candidate","ready","running","verify","done","blocked"]),
        "admission":enumeration(&["candidate","unverified","rejected","admitted"]),"reason":nullable(string()),
        "owner_thread":nullable(id()),"attempt_ids":array(id(),64)
    })));
    defs.insert(
        "attempt".into(),
        object(json!({
            "id":id(),"task_id":id(),"intent_generation":count(),"dispatch_generation":count(),
            "subject":id(),"thread_id":nullable(id()),"turn_id":nullable(id()),"status":enumeration(&["intent_unknown","active","returned","stopped"])
        })),
    );
    defs.insert("control".into(),object(json!({
        "schema_version":{"const":1},"revision":count(),"intent_generation":count(),"dispatch_generation":count(),
        "criteria":array(reference("criterion"),32),"tasks":array(reference("task"),128),
        "attempts":array(reference("attempt"),256),"child_policy":string(),
        "effects":array(object(json!({"id":id(),"kind":string(),"status":{"const":"unknown"},"reason":string()})),1032)
    })));
    defs.insert("action".into(),object(json!({
        "kind":enumeration(&["wait","refresh","answer","steer","cancel","resume","reconcile","inspect"]),
        "label":string(),"reason":string(),"tool":nullable(string()),"allowed":boolean()
    })));
    defs.insert("claim".into(),object(json!({"held":boolean(),"status":enumeration(&["owned","released","foreign","unknown"])})));
    let liveness = enumeration(&["active", "idle", "unknown"]);
    defs.insert("presentation".into(),object(json!({
        "revision":count(),"primary_action":reference("action"),"actions":array(reference("action"),8),
        "criteria":object(json!({"proven":count(),"failed":count(),"unproved":count(),"mandatory":count()})),
        "result":object(json!({"kind":enumeration(&["finished_verified","working","needs_input","stopped_unresolved","unverified"]),"label":string()})),
        "owner":object(json!({"thread_id":nullable(id()),"turn_id":nullable(id()),"liveness":liveness})),
        "workers":array(object(json!({"thread_id":id(),"liveness":liveness})),64),
        "budget":object(json!({"time_remaining_seconds":count(),"repair_attempts_remaining":count(),"repairs_used":count()})),
        "claim":reference("claim"),
        "deliverable":object(json!({"kind":enumeration(&["local_candidate","push","pr_ready"]),"status":enumeration(&["verified","unproved"]),"subject":id(),"reference":nullable(string())}))
    })));
    defs.insert("route".into(),object(json!({
        "requested_model":{"const":"gpt-6-luna"},"requested_effort":nullable(string()),
        "configured_model":nullable(string()),"configured_effort":nullable(string()),
        "requested_provider":{"const":"inherited"},"configured_provider":nullable(string()),
        "observed_model":nullable(string()),"observed_effort":nullable(string()),
        "observed_provider":{"type":"null"},"observed_model_source":nullable(json!({"const":"model/rerouted"})),
        "reroutes":{"type":"array","items":object(json!({"thread_id":id(),"turn_id":id(),"from_model":string(),"to_model":string(),"reason":string(),"source":string()}))}
    })));
    defs.insert("run".into(),optional(object(json!({
        "id":id(),"repository":text(64),"objective":text(8000),"acceptance":array(text(1000),32),"non_goals":array(text(1000),32),
        "finish":finish(),"profile":text(64),"capacity":{"type":"integer","minimum":1,"maximum":8},
        "active_workers":count(),"owned_workers":count(),
        "state":enumeration(&["STARTING","RUNNING","NEEDS_INPUT","VERIFYING","BLOCKED","INTERRUPTED","CANCELLING","CANCELLED","QUIESCENT","CONVERGED","FAILED"]),
        "current_subject":id(),"owner_thread":nullable(id()),"turn_id":nullable(id()),"delta":string(),
        "remaining_gap":nullable(string()),"skill_sha256":nullable(string()),"blocker":nullable(string()),
        "deadline_at":count(),"claim_held":boolean(),"pending_decision":nullable(object(json!({"id":id(),"question":string()}))),
        "repairs_used":count(),"repair_limit":{"type":"integer","minimum":0,"maximum":10},"generation":count(),"updated_at":count(),
        "control":nullable(reference("control")),"presentation":nullable(reference("presentation")),"route":reference("route"),
        "receipts":array(object(json!({"subject":string(),"kind":string(),"summary":string(),"created_at":count()})),20)
    })),&["receipts"]));
    defs.insert("settings_values".into(),optional(object(json!({
        "capacity":{"type":"integer","minimum":1,"maximum":8},"finish":finish(),"profile":text(64)
    })),&["profile"]));
    let string_setting = object(
        json!({"type":{"const":"string"},"title":string(),"enum":{"type":"array","items":string()}}),
    );
    defs.insert("settings_schema".into(),object(json!({
        "type":{"const":"object"},"additionalProperties":{"const":false},
        "required":{"type":"array","items":enumeration(&["capacity","finish","profile"]),"minItems":2,"maxItems":3,"uniqueItems":true},
        "properties":optional(object(json!({
            "capacity":object(json!({"type":{"const":"integer"},"title":string(),"minimum":{"const":1},"maximum":{"type":"integer","minimum":1,"maximum":8}})),
            "finish":string_setting,"profile":string_setting
        })),&["profile"])
    })));
    defs.insert("capabilities".into(),object(json!({
        "plugin_version":string(),"mcp_protocol":string(),"native_transport":enumeration(&["stdio","existing_daemon"]),
        "repositories":{"type":"array","items":object(json!({"alias":text(64),"max_finish":finish()}))},
        "repository_onboarding":object(json!({"enabled":boolean(),"approval":{"const":"local_operator"}})),
        "profiles":{"type":"array","items":object(json!({"alias":text(64),"effort":string(),"supported":boolean()}))},
        "limits":object(json!({"capacity":{"type":"integer","minimum":1,"maximum":8},"repair_attempts":{"type":"integer","minimum":0,"maximum":10},"wall_seconds":{"type":"integer","minimum":30,"maximum":86400}})),
        "observed_routing":{"const":"unverified"},"status_inference_calls":{"const":0},
        "routing_telemetry":object(json!({"model":string(),"effort":string(),"provider":string()})),
        "control_policy":object(json!({"wire_schema":{"const":1},"sqlite_schema":{"const":2},"managed_admission":string(),"native_child_policy":string(),"semantic_acceptance":string(),"independent_checks":array(string(),32),"native_output_completeness":string(),"native_environment":string(),"delivery_certification":string()})),
        "live_proof":string()
    })));
    defs.insert("candidate".into(),object(json!({"id":{"type":"string","pattern":"^[a-f0-9]{64}$"},"name":string(),"root_alias":text(64),"max_finish":finish()})));
    defs.insert("registration".into(),object(json!({"id":id(),"alias":text(64),"root_alias":text(64),"name":string(),"max_finish":finish(),"status":enumeration(&["pending","approved"])})));
    defs.insert(
        "source".into(),
        object(json!({"provider":id(),"repository_id":id(),"item_id":id(),"revision":id()})),
    );
    defs.insert("graph_candidate".into(),object(json!({"id":id(),"title":text(4000),"criterion_ids":array(id(),32),"dependencies":array(id(),128),"source":reference("source")})));
    defs.insert("change".into(),json!({"oneOf":[
        object(json!({"kind":{"const":"import_candidates"},"nodes":array(reference("graph_candidate"),32)})),
        object(json!({"kind":{"const":"set_dependencies"},"node_id":id(),"dependencies":array(id(),128)})),
        object(json!({"kind":{"const":"set_target"},"node_id":id(),"target_id":id()}))
    ]}));
    defs.insert("proposal".into(),object(json!({"id":id(),"idempotency_key":id(),"fingerprint":{"type":"string","pattern":"^[a-f0-9]{64}$"},"actor":{"const":"local_operator"},"base_revision":count(),"subject":id(),"change":reference("change"),"status":enumeration(&["proposed","applied"]),"applied_revision":nullable(count())})));
    defs.insert("graph_node".into(),json!({"allOf":[reference("task"),object(json!({"source":nullable(reference("source")),"target_preference":nullable(id())}))]}));
    defs.insert("graph".into(),object(json!({
        "run_id":id(),"revision":count(),"repository":object(json!({"alias":text(64),"identity":{"type":"string","pattern":"^[a-f0-9]{64}$"},"base_head":string(),"subject":id()})),
        "planning_only":boolean(),"nodes":array(reference("graph_node"),128),"criteria":array(reference("criterion"),32),"attempts":array(reference("attempt"),256),"claim":reference("claim"),"changes":array(reference("proposal"),128)
    })));
    defs.insert(
        "operation".into(),
        object(json!({"advertised":boolean(),"enabled":boolean(),"qualified":boolean()})),
    );
    defs.insert("backend".into(),object(json!({
        "id":id(),"label":string(),"kind":string(),"namespace":string(),"operator_enabled":boolean(),"planning_eligible":boolean(),"execution_eligible":boolean(),
        "qualification":enumeration(&["unverified","unsupported"]),"authentication":{"const":"unknown"},"entitlement":{"const":"unknown"},"reason":string(),
        "operations":object(json!({"discover":reference("operation"),"start":reference("operation"),"observe":reference("operation"),"steer":reference("operation"),"stop":reference("operation"),"reconcile":reference("operation")})),
        "limits":array(string(),32)
    })));
    defs
}

/// Every successful structured response has an object contract. Errors are MCP isError results.
/// Unknown tool names are programmer errors, rather than silently receiving a permissive schema.
pub fn output_schema(tool_name: &str) -> Value {
    let definitions = definitions();
    let mut schema = match tool_name {
        "start_factory"
        | "get_factory_run"
        | "steer_factory_run"
        | "cancel_factory_run"
        | "resume_factory_run"
        | "reconcile_factory_run" => definitions["run"].clone(),
        "list_factory_runs" => object(json!({"runs":array(reference("run"),100)})),
        "get_factory_capabilities" => definitions["capabilities"].clone(),
        "open_factory" | "open_factory_panel" | "refresh_factory" => object(
            json!({"runs":array(reference("run"),100),"selected_run":nullable(reference("run")),"capabilities":reference("capabilities"),"settings":reference("settings_values")}),
        ),
        "read_factory_settings" => object(
            json!({"schema":reference("settings_schema"),"values":reference("settings_values")}),
        ),
        "update_factory_settings" => object(json!({"values":reference("settings_values")})),
        "discover_factory_repositories" => object(
            json!({"candidates":array(reference("candidate"),100),"requests":array(reference("registration"),100),"approval":{"const":"local_operator"}}),
        ),
        "request_factory_repository" => definitions["registration"].clone(),
        "create_factory_graph"
        | "get_factory_graph"
        | "propose_factory_change"
        | "apply_factory_change" => {
            object(json!({"graph":reference("graph"),"proposal":nullable(reference("proposal"))}))
        }
        "get_factory_backends" => object(
            json!({"schema_version":{"const":1},"discovery":{"const":"configuration_only"},"policy":{"const":"subscription_only"},"targets":array(reference("backend"),5)}),
        ),
        _ => panic!("missing output schema for {tool_name}"),
    };
    // Include only reachable definitions, keeping discovery payloads compact.
    fn collect(value: &Value, definitions: &Map<String, Value>, used: &mut BTreeSet<String>) {
        match value {
            Value::Object(map) => {
                if let Some(name) = map
                    .get("$ref")
                    .and_then(Value::as_str)
                    .and_then(|s| s.strip_prefix("#/$defs/"))
                    && used.insert(name.to_owned())
                {
                    collect(&definitions[name], definitions, used);
                }
                for child in map.values() {
                    collect(child, definitions, used);
                }
            }
            Value::Array(items) => {
                for child in items {
                    collect(child, definitions, used);
                }
            }
            _ => {}
        }
    }
    let mut used = BTreeSet::new();
    collect(&schema, &definitions, &mut used);
    if !used.is_empty() {
        schema["$defs"] = Value::Object(
            used.into_iter()
                .map(|name| {
                    let value = definitions[&name].clone();
                    (name, value)
                })
                .collect(),
        );
    }
    schema
}
