//! Campaign entity: one parent work item, its planning graph and its execution runs.
//!
//! Planning-safe by construction. A campaign links an existing planning-only graph; it
//! grants no claim, dispatches no native work and never upgrades that graph. Parent
//! fields are supplied source assertions for display, never acceptance evidence.
use crate::{
    config::{Config, finish_rank, valid_alias},
    control::{TaskState, bounded_id},
    store::{Run, StartRequest},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::Path;

/// Execution runs linked to one campaign. Empty until qualified promotion exists (#71).
pub const MAX_RUN_IDS: usize = 64;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
/// The campaign planning graph is inert: planning records never dispatch, so its budget
/// fields are fixed, always within any valid operator limit, and stable for replay.
const PLANNING_CAPACITY: u32 = 1;
const PLANNING_REPAIRS: u32 = 0;
const PLANNING_WALL_SECONDS: u64 = 30;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ParentDisplay {
    #[serde(default)]
    pub number: Option<u64>,
    #[serde(default)]
    pub url: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ParentSource {
    pub provider: String,
    pub item_id: String,
    pub revision: String,
    #[serde(default)]
    pub display: Option<ParentDisplay>,
}
impl ParentSource {
    fn normalized(mut self) -> Self {
        if self
            .display
            .as_ref()
            .is_some_and(|d| d.number.is_none() && d.url.is_none())
        {
            self.display = None;
        }
        self
    }
    fn validate(&self) -> Result<()> {
        ensure!(
            [&self.provider, &self.item_id, &self.revision]
                .iter()
                .all(|value| bounded_id(value)),
            "invalid_campaign_parent"
        );
        if let Some(display) = &self.display {
            ensure!(
                display
                    .number
                    .is_none_or(|n| (1..=MAX_SAFE_INTEGER).contains(&n)),
                "invalid_campaign_parent_display"
            );
            if let Some(url) = &display.url {
                ensure!(
                    url.len() <= 2048
                        && url.len() > "https://".len()
                        && url.starts_with("https://")
                        && !url
                            .chars()
                            .any(|c| c.is_control() || c.is_whitespace() || "\"'<>\\`".contains(c)),
                    "invalid_campaign_parent_display"
                );
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CreateCampaign {
    pub repository: String,
    pub parent: ParentSource,
    pub title: String,
    pub objective: String,
    pub acceptance: Vec<String>,
    pub non_goals: Vec<String>,
    pub finish: String,
    pub profile: String,
    pub idempotency_key: String,
}
impl CreateCampaign {
    pub fn normalized(mut self) -> Self {
        self.parent = self.parent.normalized();
        self
    }
    /// The existing `create_factory_graph` request this campaign plans with.
    pub fn planning_request(&self) -> StartRequest {
        let mut key = Sha256::new();
        key.update(b"luna-campaign-plan-v1\0");
        key.update(self.idempotency_key.as_bytes());
        StartRequest {
            repository: self.repository.clone(),
            objective: self.objective.clone(),
            acceptance: self.acceptance.clone(),
            non_goals: self.non_goals.clone(),
            finish: self.finish.clone(),
            profile: self.profile.clone(),
            capacity: PLANNING_CAPACITY,
            repair_attempts: PLANNING_REPAIRS,
            wall_seconds: PLANNING_WALL_SECONDS,
            idempotency_key: format!("campaign-plan-{:x}", key.finalize()),
        }
    }
    pub fn validate(&self, config: &Config) -> Result<()> {
        self.parent.validate()?;
        ensure!(
            !self.title.trim().is_empty()
                && self.title.len() <= 1000
                && !self.title.chars().any(char::is_control),
            "invalid_campaign_title"
        );
        ensure!(
            bounded_id(&self.idempotency_key) && self.idempotency_key.len() <= 128,
            "invalid_campaign_idempotency_key"
        );
        // The planning admission's own checks: approved alias and profile, criteria bounds,
        // and `finish` capped by the repository's `max_finish` (`authority_exceeded`).
        crate::store::validate_request(config, &self.planning_request())
    }
    /// Typed serialization fixes field order; normalization makes empty display absent.
    pub fn fingerprint(&self) -> Result<String> {
        let payload = serde_json::to_vec(&json!({"campaign_v":1,"request":self}))?;
        Ok(format!("{:x}", Sha256::digest(payload)))
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CampaignStatus {
    Planned,
    Promoted,
    Finished,
    Stopped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Campaign {
    pub id: String,
    pub repository: String,
    /// Opaque local graph repository identity, never a filesystem path.
    pub repository_identity: String,
    pub parent: ParentSource,
    pub title: String,
    pub planning_run_id: String,
    pub run_ids: Vec<String>,
    pub finish: String,
    pub status: CampaignStatus,
    pub idempotency_key: String,
    pub fingerprint: String,
    pub created_at: u64,
    pub updated_at: u64,
}
impl Campaign {
    pub fn validate(&self) -> Result<()> {
        self.parent.validate()?;
        ensure!(
            bounded_id(&self.id)
                && valid_alias(&self.repository)
                && self.repository_identity.len() == 64
                && self
                    .repository_identity
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit())
                && bounded_id(&self.planning_run_id)
                && self.run_ids.len() <= MAX_RUN_IDS
                && self.run_ids.iter().all(|id| bounded_id(id))
                && !self.run_ids.contains(&self.planning_run_id)
                && !self.title.trim().is_empty()
                && self.title.len() <= 1000
                && bounded_id(&self.idempotency_key)
                && self.fingerprint.len() == 64
                && self.created_at <= self.updated_at,
            "campaign_record_corrupt"
        );
        finish_rank(&self.finish)?;
        // No promotion transition exists yet, so a recorded execution link is corrupt.
        ensure!(
            self.status == CampaignStatus::Planned && self.run_ids.is_empty(),
            "campaign_record_corrupt"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromoteCampaign {
    pub campaign_id: String,
    pub expected_revision: u64,
    pub idempotency_key: String,
}
impl PromoteCampaign {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            bounded_id(&self.campaign_id)
                && bounded_id(&self.idempotency_key)
                && self.expected_revision <= MAX_SAFE_INTEGER,
            "invalid_campaign_promotion"
        );
        Ok(())
    }
}

/// The graph identity the planning graph will carry, computed before any write so a
/// duplicate parent is rejected without creating an orphan planning record.
pub fn repository_identity(config: &Config, alias: &str) -> Result<String> {
    let root = &config
        .repositories
        .get(alias)
        .context("unknown_repository")?
        .root;
    let toplevel = crate::store::git(root, &["rev-parse", "--show-toplevel"])?;
    ensure!(
        Path::new(&toplevel).canonicalize()? == *root,
        "repository_root_mismatch"
    );
    let common = crate::store::git(
        root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    let common = Path::new(&common)
        .canonicalize()?
        .to_string_lossy()
        .into_owned();
    crate::graph::repository_stamp(root, &common)
}

/// Display URLs are shown only when they carry no credential-like marker.
fn public_url(url: &str) -> Option<&str> {
    (crate::lifecycle::safe_summary(url, 2048) == url).then_some(url)
}

/// Read projection. It never claims execution: the planning summary is planning-only,
/// linked runs are listed as recorded, and promotion stays disallowed until qualified.
pub fn project(campaign: &Campaign, planning: &Run, runs: &[Run]) -> Result<Value> {
    ensure!(
        planning.id == campaign.planning_run_id && planning.planning_only,
        "campaign_planning_run_invalid"
    );
    let control = planning.control.as_ref().context("control_missing")?;
    let mut counts = [0usize; 6];
    for task in control.tasks.values() {
        counts[match task.state {
            TaskState::Candidate => 0,
            TaskState::Ready => 1,
            TaskState::Running => 2,
            TaskState::Verify => 3,
            TaskState::Done => 4,
            TaskState::Blocked => 5,
        }] += 1;
    }
    let display = campaign.parent.display.as_ref().map(|display| {
        json!({"number":display.number,"url":display.url.as_deref().and_then(public_url)})
    });
    Ok(json!({
        "id":campaign.id,"repository":campaign.repository,
        "parent":{"provider":campaign.parent.provider,"item_id":campaign.parent.item_id,"revision":campaign.parent.revision,"display":display},
        "title":crate::lifecycle::safe_summary(&campaign.title,1000),"finish":campaign.finish,"status":campaign.status,
        "planning_run_id":campaign.planning_run_id,"run_ids":campaign.run_ids,
        "created_at":campaign.created_at,"updated_at":campaign.updated_at,
        "planning":{"run_id":planning.id,"revision":control.revision,"planning_only":planning.planning_only,
            "tasks":{"total":control.tasks.len(),"candidate":counts[0],"ready":counts[1],"running":counts[2],"verify":counts[3],"done":counts[4],"blocked":counts[5]}},
        "runs":runs.iter().map(|run|{let public=crate::lifecycle::public_run(run);json!({"id":run.id,"state":public["state"],"updated_at":run.updated_at})}).collect::<Vec<_>>(),
        "promotion":{"allowed":false,"reason":promotion_blocker()}
    }))
}

/// Why promotion is refused. Even qualified execution would not make promotion allowed:
/// this version defines no promotion transition, so it fails closed either way.
pub fn promotion_blocker() -> &'static str {
    if crate::lifecycle::execution_qualification().0 {
        "campaign_promotion_unavailable"
    } else {
        "execution_not_qualified"
    }
}
