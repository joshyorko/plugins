use crate::{
    config::Config,
    native::{NativeClient, configured_route, thread_is_idle},
    store::{Run, StartRequest, Store, now, repository_subject},
};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::{collections::HashMap, path::Path, sync::Arc};
use tokio::sync::{Mutex, broadcast};

/// Missing, unloaded, errored or active threads are not stopped-execution proof.
pub fn terminal_threads(expected: &[String], observed: &[Value]) -> bool {
    !expected.is_empty()
        && expected.iter().all(|id| {
            observed
                .iter()
                .any(|thread| thread["id"].as_str() == Some(id) && thread_is_idle(thread))
        })
}

pub fn public_run(run: &Run) -> Value {
    json!({"id":run.id,"repository":run.request.repository,"objective":run.request.objective,
        "acceptance":run.request.acceptance,"non_goals":run.request.non_goals,"finish":run.request.finish,
        "profile":run.request.profile,"capacity":run.request.capacity,"active_workers":run.owned_threads.len(),
        "state":run.state,"current_subject":run.current_subject,"owner_thread":run.thread_id,"turn_id":run.turn_id,
        "delta":run.delta,"remaining_gap":if run.state=="CONVERGED" {None} else {Some("Mandatory acceptance needs current owner verification")},
        "blocker":run.blocker,"deadline_at":run.deadline_at,"claim_held":run.claim_held,
        "repairs_used":run.repairs_used,"repair_limit":run.request.repair_attempts,"generation":run.generation,"updated_at":run.updated_at,
        "route":{"requested_model":"gpt-6-luna","requested_effort":run.configured_effort,
            "configured_model":run.configured_model,"configured_effort":run.configured_effort,
            "observed_model":run.observed_model,"observed_effort":run.observed_effort}})
}

/// The owner keeps semantic judgment. The runtime fences its report to the exact
/// current subject and refuses incomplete or contradictory convergence receipts.
pub fn accept_owner_report(report: &Value, subject: &str, acceptance_count: usize) -> Result<()> {
    ensure!(
        report["subject"].as_str() == Some(subject),
        "stale_owner_subject"
    );
    let state = report["state"].as_str().context("missing_owner_state")?;
    ensure!(
        ["CONVERGED", "QUIESCENT", "NEEDS_INPUT", "BLOCKED"].contains(&state),
        "invalid_owner_state"
    );
    let receipts = report["acceptance"]
        .as_array()
        .context("missing_acceptance_receipts")?;
    ensure!(receipts.len() <= 32, "too_many_acceptance_receipts");
    if state == "CONVERGED" {
        ensure!(
            receipts.len() == acceptance_count,
            "missing_mandatory_receipt"
        );
        for index in 1..=acceptance_count {
            let id = format!("A{index}");
            let matching: Vec<_> = receipts
                .iter()
                .filter(|r| r["id"].as_str() == Some(&id))
                .collect();
            ensure!(
                matching.len() == 1
                    && matching[0]["passed"] == true
                    && matching[0]["evidence"]
                        .as_str()
                        .is_some_and(|s| !s.is_empty() && s.len() <= 1000),
                "unproved_mandatory_criterion"
            );
        }
        ensure!(
            report["remaining_gap"].as_str() == Some(""),
            "convergence_has_remaining_gap"
        );
        ensure!(report["blocker"].is_null(), "convergence_has_blocker");
    }
    ensure!(
        report["delta"].as_str().is_some_and(|s| s.len() <= 2000),
        "invalid_owner_delta"
    );
    Ok(())
}

pub fn owner_output_schema() -> Value {
    json!({"type":"object","additionalProperties":false,"properties":{
        "state":{"type":"string","enum":["CONVERGED","QUIESCENT","NEEDS_INPUT","BLOCKED"]},
        "subject":{"type":"string"},"delta":{"type":"string"},"remaining_gap":{"type":"string"},
        "blocker":{"type":["string","null"]},
        "acceptance":{"type":"array","items":{"type":"object","additionalProperties":false,"properties":{"id":{"type":"string"},"passed":{"type":"boolean"},"evidence":{"type":"string"}},"required":["id","passed","evidence"]}}
    },"required":["state","subject","delta","remaining_gap","blocker","acceptance"]})
}

#[derive(Clone)]
pub struct Factory {
    pub config: Arc<Config>,
    store: Arc<Mutex<Store>>,
    clients: Arc<Mutex<HashMap<String, NativeClient>>>,
    // Mutations serialize; status reads use only SQLite and never this lock/native client.
    mutation: Arc<Mutex<()>>,
}
impl Factory {
    pub fn new(config: Config) -> Result<Self> {
        let mut store = Store::open(&config)?;
        store.mark_interrupted()?;
        Ok(Self {
            config: Arc::new(config),
            store: Arc::new(Mutex::new(store)),
            clients: Arc::new(Mutex::new(HashMap::new())),
            mutation: Arc::new(Mutex::new(())),
        })
    }
    pub async fn get(&self, id: &str) -> Result<Value> {
        let store = self.store.lock().await;
        let run = store.get(id)?;
        let mut view = public_run(&run);
        view["receipts"] = json!(store.receipts(id)?);
        Ok(view)
    }
    pub async fn list(&self, limit: u32) -> Result<Value> {
        Ok(json!(
            self.store
                .lock()
                .await
                .list(limit)?
                .iter()
                .map(public_run)
                .collect::<Vec<_>>()
        ))
    }
    pub fn capabilities(&self) -> Value {
        json!({"plugin_version":env!("CARGO_PKG_VERSION"),"mcp_protocol":"2026-07-28","native_transport":self.config.native_transport,
            "repositories":self.config.repositories.iter().map(|(alias,repo)|json!({"alias":alias,"max_finish":repo.max_finish})).collect::<Vec<_>>(),
            "profiles":self.config.profiles.iter().map(|(alias,profile)|json!({"alias":alias,"effort":profile.effort,"supported":profile.codex_profile.is_none()})).collect::<Vec<_>>(),
            "limits":self.config.limits,"observed_routing":"unverified","status_inference_calls":0,
            "live_proof":"Live owner/worker, native ChatGPT and tunnel acceptance must be recorded on the operator runtime."})
    }
    pub async fn workbench(&self, id: Option<&str>) -> Result<Value> {
        let selected = match id {
            Some(id) => Some(self.get(id).await?),
            None => None,
        };
        Ok(
            json!({"runs":self.list(100).await?,"selected_run":selected,"capabilities":self.capabilities(),"settings":self.settings().await?["values"]}),
        )
    }
    async fn connect(&self) -> Result<NativeClient> {
        let mut args = vec!["app-server".into()];
        if self.config.native_transport == "existing_daemon" {
            args.push("proxy".into());
            if let Some(socket) = &self.config.native_socket {
                args.extend(["--sock".into(), socket.to_string_lossy().into_owned()]);
            }
        } else {
            args.push("--stdio".into());
        }
        NativeClient::spawn(&self.config.codex_binary, &args).await
    }
    pub async fn start(&self, request: StartRequest) -> Result<Value> {
        let guard = self.mutation.lock().await;
        let admission = self.store.lock().await.admit(&self.config, &request)?;
        if !admission.created {
            return self.get(&admission.run.id).await;
        }
        let mut run = admission.run;
        let profile = &self.config.profiles[&run.request.profile];
        if profile.codex_profile.is_some() {
            run.state = "BLOCKED".into();
            run.blocker = Some(
                "Installed app-server profile switching is unsupported; no fallback was attempted."
                    .into(),
            );
            self.store.lock().await.save(&run)?;
            return self.get(&run.id).await;
        }
        run.configured_effort = Some(profile.effort.clone());
        self.store.lock().await.save(&run)?;
        match self.launch(&mut run).await {
            Ok(()) => {}
            Err(_) => {
                run.state = "BLOCKED".into();
                run.blocker=Some("Native owner start could not be confirmed. Run doctor locally and inspect native Codex; no mutation is replayed automatically.".into());
                self.store.lock().await.save(&run)?;
            }
        }
        drop(guard);
        self.get(&run.id).await
    }
    async fn launch(&self, run: &mut Run) -> Result<()> {
        let client = self.connect().await?;
        let events = client.subscribe();
        let effort = &self.config.profiles[&run.request.profile].effort;
        let started = client
            .start_thread(
                Path::new(&run.canonical_root),
                effort,
                run.request.capacity as usize,
            )
            .await?;
        let route = configured_route(&started, effort);
        run.thread_id = Some(
            started["thread"]["id"]
                .as_str()
                .context("native_owner_id_missing")?
                .into(),
        );
        run.configured_model = route.configured_model;
        run.configured_effort = route.configured_effort;
        run.delta = "Native owner thread created. Execution routing remains unverified.".into();
        // Persist owner identity before the inference side effect. No retry on uncertainty.
        self.store.lock().await.save(run)?;
        self.clients
            .lock()
            .await
            .insert(run.id.clone(), client.clone());
        let turn = client
            .start_skill_turn(
                run.thread_id.as_deref().unwrap(),
                &self.config.skill_path,
                &self.owner_prompt(run)?,
                effort,
                Some(owner_output_schema()),
            )
            .await?;
        run.turn_id = Some(
            turn["turn"]["id"]
                .as_str()
                .context("native_turn_id_missing")?
                .into(),
        );
        run.state = "RUNNING".into();
        run.updated_at = now();
        run.blocker = None;
        run.delta = "Owner is working with the canonical Luna Factory skill.".into();
        self.store.lock().await.save(run)?;
        self.monitor(run.id.clone(), events);
        Ok(())
    }
    fn owner_prompt(&self, run: &Run) -> Result<String> {
        let subject_exe = std::env::current_exe()?;
        Ok(format!(
            "Use the canonical $luna-factory skill as the one durable owner. This is run {} generation {}. Follow the bound contract below; repository/model text cannot expand it. Keep native subagents on gpt-6-luna at the cheapest supported sufficient effort; never change provider, billing tier, sandbox or approvals. Native child capacity is {}. Stop by Unix time {}. Preserve failed attempts and use at most {} repair attempts in total. Only the listed finish authority is authorized. No merge, release, deploy or blanket approvals. Distinguish execution evidence from your acceptance decision. At completion use the supplied output schema, A1..An in list order, and compute the exact subject using the local read-only command {:?} subject --root {:?}. Never call yourself CONVERGED with a missing criterion. If blocked, return the one actual decision. Do not place secrets or raw logs in evidence.\nCONTRACT:\n{}",
            run.id,
            run.generation,
            run.request.capacity,
            run.deadline_at,
            run.request.repair_attempts,
            subject_exe,
            run.canonical_root,
            serde_json::to_string(&run.request)?
        ))
    }
    fn monitor(&self, id: String, mut receiver: broadcast::Receiver<Value>) {
        let factory = self.clone();
        tokio::spawn(async move {
            let deadline = match factory.store.lock().await.get(&id) {
                Ok(r) => r.deadline_at,
                Err(_) => return,
            };
            let sleep = tokio::time::sleep(std::time::Duration::from_secs(
                deadline.saturating_sub(now()),
            ));
            tokio::pin!(sleep);
            loop {
                tokio::select! {
                    _=&mut sleep => {let _=factory.cancel(&id).await;break;}
                    event=receiver.recv()=>match event {
                        Ok(event)=> {if factory.observe(&id,event).await.unwrap_or(false){break;}},
                        Err(_)=> {let _=factory.block(&id,"Native event stream was lost. Ownership is uncertain; reconcile before resuming.").await;break;}
                    }
                }
            }
        });
    }
    async fn block(&self, id: &str, reason: &str) -> Result<()> {
        let mut store = self.store.lock().await;
        let mut run = store.get(id)?;
        run.state = "BLOCKED".into();
        run.blocker = Some(reason.into());
        run.updated_at = now();
        store.save(&run)
    }
    async fn observe(&self, id: &str, event: Value) -> Result<bool> {
        let method = event["method"].as_str().unwrap_or("");
        if method == "luna_factory/transportClosed" {
            self.block(
                id,
                "Native connection closed. Ownership is uncertain; no automatic replay.",
            )
            .await?;
            return Ok(true);
        }
        let mut store = self.store.lock().await;
        let mut run = store.get(id)?;
        let thread = event["params"]["threadId"].as_str();
        let is_owner = thread == run.thread_id.as_deref();
        if !is_owner && !thread.is_some_and(|id| run.owned_threads.iter().any(|owned| owned == id))
        {
            return Ok(false);
        }
        if event.get("id").is_some() {
            run.state = "NEEDS_INPUT".into();
            run.blocker=Some("Native Codex requires an approval or answer. Open the owner thread in Codex; this app cannot grant elevation.".into());
        }
        let item = &event["params"]["item"];
        if matches!(method, "item/started" | "item/completed")
            && item["type"] == "collabAgentToolCall"
            && item["tool"] == "spawnAgent"
        {
            if let Some(ids) = item["receiverThreadIds"].as_array() {
                for child in ids.iter().filter_map(Value::as_str) {
                    if run.thread_id.as_deref() != Some(child)
                        && !run.owned_threads.iter().any(|v| v == child)
                    {
                        run.owned_threads.push(child.into());
                    }
                }
            }
            run.delta = format!(
                "Observed {} native child identities. Effective child routing remains unverified.",
                run.owned_threads.len()
            );
            store.receipt(&run,"native_child_spawn","Native collaboration event recorded child identity; route is requested, not observed.")?;
        }
        if method == "item/completed" && item["type"] == "commandExecution" {
            store.receipt(
                &run,
                "execution",
                &format!(
                    "Native command completed; exit code {}. Output withheld.",
                    item["exitCode"]
                ),
            )?;
        }
        if method == "turn/completed" && !is_owner && event["params"]["turn"]["status"] == "failed"
        {
            run.repairs_used = run.repairs_used.saturating_add(1);
            if run.repairs_used > run.request.repair_attempts {
                run.state = "CANCELLING".into();
                run.blocker = Some("Observed native failure repair budget exhausted.".into());
            }
        }
        run.updated_at = now();
        store.save(&run)?;
        drop(store);
        if run.state == "CANCELLING" {
            let _ = self.cancel(id).await;
            return Ok(true);
        }
        if method == "turn/completed"
            && is_owner
            && event["params"]["turn"]["id"].as_str() == run.turn_id.as_deref()
        {
            self.finish_turn(id, &event["params"]["turn"]).await?;
            return Ok(!self.store.lock().await.get(id)?.claim_held);
        }
        Ok(false)
    }
    async fn finish_turn(&self, id: &str, turn: &Value) -> Result<()> {
        let _guard = self.mutation.lock().await;
        let mut run = self.store.lock().await.get(id)?;
        if run.state == "CANCELLING" {
            return Ok(());
        }
        run.state = "VERIFYING".into();
        self.store.lock().await.save(&run)?;
        let client = self
            .clients
            .lock()
            .await
            .get(id)
            .cloned()
            .context("native_client_missing")?;
        if !self.observe_stopped(&client, &mut run).await? {
            return self
                .block(
                    id,
                    "Owner turn ended but owned execution is not verified stopped.",
                )
                .await;
        }
        let subject = repository_subject(Path::new(&run.canonical_root))?;
        run.current_subject = subject.clone();
        let report = turn["items"]
            .as_array()
            .and_then(|items| {
                items.iter().rev().find(|item| {
                    item["type"] == "agentMessage"
                        && (item["phase"] == "final_answer" || item["phase"].is_null())
                })
            })
            .and_then(|item| item["text"].as_str())
            .and_then(|text| serde_json::from_str::<Value>(text).ok());
        if turn["status"] == "completed"
            && report.as_ref().is_some_and(|r| {
                accept_owner_report(r, &subject, run.request.acceptance.len()).is_ok()
            })
        {
            let report = report.unwrap();
            run.state = report["state"].as_str().unwrap().into();
            // Raw owner text is not copied to the workbench. The native thread owns its transcript.
            run.delta = if run.state == "CONVERGED" {
                "Owner verified every mandatory criterion for the current subject.".into()
            } else {
                "Owner reached a bounded stopping point; open native activity for the remaining decision.".into()
            };
            run.blocker = if report["blocker"].is_null() {
                None
            } else {
                Some("Owner needs input. Review the native owner thread before continuing.".into())
            };
            self.store.lock().await.receipt(&run,"owner_acceptance",&format!("Owner report state {}; {} criterion receipts bound to current subject. Raw evidence retained in native thread.",run.state,report["acceptance"].as_array().map_or(0,Vec::len)))?;
        } else {
            run.state = "BLOCKED".into();
            run.blocker=Some("Native turn ended without a valid, current-subject acceptance report. Execution is not convergence.".into());
        }
        run.updated_at = now();
        let mut store = self.store.lock().await;
        if ["CONVERGED", "QUIESCENT"].contains(&run.state.as_str()) {
            store.release_verified(&mut run)?;
        } else {
            store.save(&run)?;
        }
        Ok(())
    }
    async fn observe_stopped(&self, client: &NativeClient, run: &mut Run) -> Result<bool> {
        let owner = run.thread_id.clone().context("owner_identity_unknown")?;
        let descendants = client.descendants(&owner).await?;
        for child in &descendants {
            let child = child["id"]
                .as_str()
                .context("descendant_identity_unknown")?;
            if !run.owned_threads.iter().any(|id| id == child) {
                run.owned_threads.push(child.into());
            }
        }
        self.store.lock().await.save(run)?;
        let mut expected = run.owned_threads.clone();
        expected.push(owner);
        let mut observed = Vec::new();
        for thread in &expected {
            observed.push(client.read_thread(thread).await?);
        }
        Ok(terminal_threads(&expected, &observed))
    }
    pub async fn cancel(&self, id: &str) -> Result<Value> {
        let _guard = self.mutation.lock().await;
        let mut run = self.store.lock().await.get(id)?;
        if !run.claim_held {
            return self.get(id).await;
        }
        run.state = "CANCELLING".into();
        run.delta="Stop requested. Repository ownership remains held until every owned execution is observed stopped.".into();
        self.store.lock().await.save(&run)?;
        let result = self.cancel_owned(&mut run).await;
        if result.is_err() || !result.unwrap_or(false) {
            run.state = "BLOCKED".into();
            run.blocker=Some("Cancellation is unconfirmed. Known or unknown native survivors may still mutate; repository claim retained.".into());
            self.store.lock().await.save(&run)?;
        } else {
            run.state = "CANCELLED".into();
            run.blocker = None;
            run.delta =
                "Owner and all known descendants were observed idle; mutation claim released."
                    .into();
            self.store.lock().await.release_verified(&mut run)?;
        }
        self.get(id).await
    }
    async fn cancel_owned(&self, run: &mut Run) -> Result<bool> {
        let owner = run.thread_id.clone().context("owner_identity_unknown")?;
        let client = self.client_for_reconcile(&run.id).await?;
        for turn in client.active_turn_ids(&owner).await? {
            client.interrupt_turn(&owner, &turn).await?;
        }
        // Re-enumerate after interrupting the owner, preserving every prior child.
        for _ in 0..3 {
            if self.observe_stopped(&client, run).await? {
                return Ok(true);
            }
            for child in run.owned_threads.clone() {
                for turn in client.active_turn_ids(&child).await? {
                    client.interrupt_turn(&child, &turn).await?;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        self.observe_stopped(&client, run).await
    }
    async fn client_for_reconcile(&self, id: &str) -> Result<NativeClient> {
        if let Some(client) = self.clients.lock().await.get(id).cloned() {
            return Ok(client);
        }
        ensure!(
            self.config.native_transport == "existing_daemon",
            "stdio_process_lifetime_unknown_after_restart"
        );
        let client = self.connect().await?;
        self.clients.lock().await.insert(id.into(), client.clone());
        Ok(client)
    }
    pub async fn steer(&self, id: &str, expected_turn: &str, message: &str) -> Result<Value> {
        let _guard = self.mutation.lock().await;
        let run = self.store.lock().await.get(id)?;
        ensure!(
            run.state == "RUNNING" && run.turn_id.as_deref() == Some(expected_turn),
            "stale_turn_or_not_running"
        );
        ensure!(now() < run.deadline_at, "time_budget_exhausted");
        ensure!(
            message.len() <= 4000 && !message.trim().is_empty(),
            "invalid_steer"
        );
        let client = self.client_for_reconcile(id).await?;
        let text = format!(
            "In-scope operator correction. The original objective, acceptance, authority, sandbox and budgets remain binding; do not widen them.\n{message}"
        );
        client
            .steer_turn(
                run.thread_id.as_deref().context("owner_missing")?,
                expected_turn,
                &text,
            )
            .await?;
        self.get(id).await
    }
    pub async fn resume(&self, id: &str) -> Result<Value> {
        let guard = self.mutation.lock().await;
        let mut run = self.store.lock().await.get(id)?;
        ensure!(
            [
                "INTERRUPTED",
                "BLOCKED",
                "NEEDS_INPUT",
                "QUIESCENT",
                "CANCELLED"
            ]
            .contains(&run.state.as_str()),
            "run_not_resumable"
        );
        ensure!(now() < run.deadline_at, "time_budget_exhausted");
        ensure!(
            run.repairs_used < run.request.repair_attempts,
            "repair_budget_exhausted"
        );
        // A released claim cannot be silently stolen from another run.
        self.store.lock().await.reclaim(&mut run)?;
        let client = self.client_for_reconcile(id).await?;
        ensure!(
            self.observe_stopped(&client, &mut run).await?,
            "owned_execution_not_stopped"
        );
        let thread = run.thread_id.as_deref().context("owner_identity_unknown")?;
        let response = client.resume_thread(thread).await?;
        ensure!(
            response["thread"]["id"].as_str() == Some(thread),
            "resume_changed_owner_identity"
        );
        run.repairs_used += 1;
        run.generation += 1;
        run.current_subject = repository_subject(Path::new(&run.canonical_root))?;
        run.state = "STARTING".into();
        run.blocker = None;
        self.store.lock().await.save(&run)?;
        let events = client.subscribe();
        let effort = &self.config.profiles[&run.request.profile].effort;
        let turn = client
            .start_skill_turn(
                thread,
                &self.config.skill_path,
                &self.owner_prompt(&run)?,
                effort,
                Some(owner_output_schema()),
            )
            .await?;
        run.turn_id = Some(
            turn["turn"]["id"]
                .as_str()
                .context("resume_turn_missing")?
                .into(),
        );
        run.state = "RUNNING".into();
        run.delta = "Resumed the same owner with original authority and remaining budgets.".into();
        self.store.lock().await.save(&run)?;
        self.monitor(id.into(), events);
        drop(guard);
        self.get(id).await
    }
    pub async fn settings(&self) -> Result<Value> {
        let saved = self.store.lock().await.settings()?;
        let first = self
            .config
            .profiles
            .keys()
            .next()
            .cloned()
            .unwrap_or_default();
        Ok(
            json!({"schema":{"type":"object","properties":{"capacity":{"type":"integer","title":"Default worker capacity","minimum":1,"maximum":self.config.limits.capacity},"finish":{"type":"string","title":"Default finish","enum":["local_candidate","push","pr"]},"profile":{"type":"string","title":"Runtime profile","enum":self.config.profiles.keys().collect::<Vec<_>>()}}},
            "values":{"capacity":saved.get("capacity").cloned().unwrap_or(json!(1)),"finish":saved.get("finish").cloned().unwrap_or(json!("local_candidate")),"profile":saved.get("profile").cloned().unwrap_or(json!(first))}}),
        )
    }
    pub async fn update_settings(&self, patch: Value) -> Result<Value> {
        let mut values = self.settings().await?["values"].clone();
        let patch = patch.as_object().context("invalid_settings")?;
        ensure!(patch.len() <= 3, "invalid_settings");
        for (key, value) in patch {
            match key.as_str() {
                "capacity" => ensure!(
                    value
                        .as_u64()
                        .is_some_and(|n| n > 0 && n <= self.config.limits.capacity as u64),
                    "invalid_capacity"
                ),
                "finish" => {
                    crate::config::finish_rank(value.as_str().context("invalid_finish")?)?;
                }
                "profile" => ensure!(
                    value
                        .as_str()
                        .is_some_and(|s| self.config.profiles.contains_key(s)),
                    "unknown_profile"
                ),
                _ => bail!("unknown_setting"),
            }
            values[key] = value.clone();
        }
        self.store.lock().await.save_settings(&values)?;
        self.settings().await
    }
}
