//! Campaign entity: one parent work item, its planning graph and its execution runs.
//!
//! Planning-safe by construction. A campaign links an existing planning-only graph; it
//! grants no claim, dispatches no native work and never upgrades that graph. Parent
//! fields are supplied source assertions for display, never acceptance evidence.
use crate::{
    config::{Config, valid_alias},
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

/// Forge providers a campaign parent may name. Lowercase on the wire; anything else is rejected.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ForgeProvider {
    Github,
    Local,
    Fixture,
}
impl ForgeProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Github => "github",
            Self::Local => "local",
            Self::Fixture => "fixture",
        }
    }
}

/// Where parent metadata came from. Every parent is `supplied` today: the caller (or
/// ChatGPT) asserted it and Luna has not verified it. Only data Luna itself fetched with
/// the GitHub App may ever be `github_app` (#76); nothing in this version produces it.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Provenance {
    Supplied,
    GithubApp,
}

/// Forge identity of the parent item, shared with #76:
/// `{provider, repository: "owner/repo" (lowercase), number, node_id?}`, canonical key
/// `provider:owner/repo#number`. Separate from the physical identity (local graph
/// repository identity plus alias), which never comes from the caller.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ForgeIdentity {
    pub provider: ForgeProvider,
    pub repository: String,
    pub number: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
}
impl ForgeIdentity {
    pub fn key(&self) -> String {
        format!(
            "{}:{}#{}",
            self.provider.as_str(),
            self.repository,
            self.number
        )
    }
    fn normalized(mut self) -> Self {
        self.repository = self.repository.to_ascii_lowercase();
        self
    }
    fn validate(&self) -> Result<()> {
        let segment = |part: &str| {
            (1..=100).contains(&part.len())
                && !part.starts_with('.')
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-_.".contains(&b))
        };
        ensure!(
            self.repository
                .split_once('/')
                .is_some_and(|(owner, repo)| segment(owner) && segment(repo))
                && (1..=MAX_SAFE_INTEGER).contains(&self.number)
                && self.node_id.as_deref().is_none_or(token),
            "invalid_campaign_parent"
        );
        Ok(())
    }
}

/// A non-blank identifier: bounded, and free of whitespace and control characters.
fn token(value: &str) -> bool {
    bounded_id(value) && !value.chars().any(char::is_whitespace)
}

/// A display URL is shown only when it parses as an `https` URL with a host, carries no
/// userinfo, is already in canonical form, and, for GitHub, is exactly the forge item's
/// `https://github.com/{owner}/{repo}/issues/{number}`. Invalid URLs are never echoed.
pub fn valid_display_url(forge: &ForgeIdentity, raw: &str) -> bool {
    if raw.len() > 2048
        || raw
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || "\"'<>\\`".contains(c))
    {
        return false;
    }
    let Ok(url) = reqwest::Url::parse(raw) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    if url.scheme() != "https"
        || host.is_empty()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.as_str() != raw
    {
        return false;
    }
    match forge.provider {
        ForgeProvider::Github => {
            host == "github.com"
                && url.port().is_none()
                && url.query().is_none()
                && url.fragment().is_none()
                && url
                    .path()
                    .eq_ignore_ascii_case(&format!("/{}/issues/{}", forge.repository, forge.number))
        }
        ForgeProvider::Local | ForgeProvider::Fixture => true,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ParentDisplay {
    #[serde(default)]
    pub url: Option<String>,
}
/// The caller-supplied parent: a forge identity, its source revision and optional display.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ParentInput {
    pub forge: ForgeIdentity,
    pub revision: String,
    #[serde(default)]
    pub display: Option<ParentDisplay>,
}
impl ParentInput {
    fn normalized(mut self) -> Self {
        self.forge = self.forge.normalized();
        if self.display.as_ref().is_some_and(|d| d.url.is_none()) {
            self.display = None;
        }
        self
    }
    fn validate(&self) -> Result<()> {
        self.forge.validate()?;
        ensure!(token(&self.revision), "invalid_campaign_parent");
        if let Some(url) = self.display.as_ref().and_then(|d| d.url.as_deref()) {
            ensure!(
                valid_display_url(&self.forge, url),
                "invalid_campaign_parent_display"
            );
        }
        Ok(())
    }
}
/// The recorded parent: the supplied assertion plus its explicit provenance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Parent {
    pub forge: ForgeIdentity,
    pub revision: String,
    pub display: Option<ParentDisplay>,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CreateCampaign {
    pub repository: String,
    pub parent: ParentInput,
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
            token(&self.idempotency_key) && self.idempotency_key.len() <= 128,
            "invalid_campaign_idempotency_key"
        );
        // The planning admission's own checks: approved alias and profile, criteria bounds,
        // and `finish` capped by the repository's `max_finish` (`authority_exceeded`).
        crate::store::validate_request(config, &self.planning_request())
    }
    /// Typed serialization fixes field order; normalization makes empty display absent.
    pub fn fingerprint(&self) -> Result<String> {
        let payload = serde_json::to_vec(&json!({"campaign_v":2,"request":self}))?;
        Ok(format!("{:x}", Sha256::digest(payload)))
    }
    /// The record linking this request to its admitted planning graph. Parent metadata is
    /// recorded as `supplied`: Luna has not fetched or verified it.
    pub fn record(
        &self,
        repository_identity: &str,
        fingerprint: &str,
        planning_run_id: &str,
        timestamp: u64,
    ) -> Campaign {
        Campaign {
            id: uuid::Uuid::new_v4().to_string(),
            repository: self.repository.clone(),
            repository_identity: repository_identity.to_owned(),
            parent: Parent {
                forge: self.parent.forge.clone(),
                revision: self.parent.revision.clone(),
                display: self.parent.display.clone(),
                provenance: Provenance::Supplied,
            },
            title: self.title.clone(),
            planning_run_id: planning_run_id.to_owned(),
            run_ids: vec![],
            status: CampaignStatus::Planned,
            idempotency_key: self.idempotency_key.clone(),
            fingerprint: fingerprint.to_owned(),
            created_at: timestamp,
            updated_at: timestamp,
        }
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

/// A thin reference: the parent assertion, the physical repository, the canonical
/// planning run and any execution runs. Plan content (objective, criteria, finish, tasks)
/// lives only in the planning run and is read from there.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Campaign {
    pub id: String,
    /// Physical identity: the approved alias and the opaque local graph repository
    /// identity. Never a filesystem path, and never derived from the forge identity.
    pub repository: String,
    pub repository_identity: String,
    pub parent: Parent,
    pub title: String,
    pub planning_run_id: String,
    pub run_ids: Vec<String>,
    pub status: CampaignStatus,
    pub idempotency_key: String,
    pub fingerprint: String,
    pub created_at: u64,
    pub updated_at: u64,
}
impl Campaign {
    pub fn forge_key(&self) -> String {
        self.parent.forge.key()
    }
    pub fn validate(&self) -> Result<()> {
        let input = ParentInput {
            forge: self.parent.forge.clone(),
            revision: self.parent.revision.clone(),
            display: self.parent.display.clone(),
        };
        ensure!(
            input.validate().is_ok()
                && input.clone().normalized() == input
                && bounded_id(&self.id)
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
                && token(&self.idempotency_key)
                && self.fingerprint.len() == 64
                && self.created_at <= self.updated_at,
            "campaign_record_corrupt"
        );
        // No promotion transition exists yet, so a recorded execution link is corrupt, and
        // nothing in this version fetches parents, so a non-supplied provenance is forged.
        ensure!(
            self.status == CampaignStatus::Planned
                && self.run_ids.is_empty()
                && self.parent.provenance == Provenance::Supplied,
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
                && token(&self.idempotency_key)
                && self.expected_revision <= MAX_SAFE_INTEGER,
            "invalid_campaign_promotion"
        );
        Ok(())
    }
}

/// The graph identity the planning graph will carry, computed before admission. Admission
/// recomputes it inside its write transaction and rejects a mismatch, so a repository
/// replaced in between cannot leave an orphan planning graph.
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

/// Display URLs are shown only when valid for the forge item and free of any
/// credential-like marker; anything else is projected as null, never echoed.
fn public_url<'a>(forge: &ForgeIdentity, url: &'a str) -> Option<&'a str> {
    (valid_display_url(forge, url) && crate::lifecycle::safe_summary(url, 2048) == url)
        .then_some(url)
}

/// Read projection. It never claims execution: the planning summary is planning-only,
/// linked runs are listed as recorded, and promotion stays disallowed until qualified.
/// Forge identity (`parent.forge`, `parent.key`) and physical identity (`repository`)
/// are projected separately; `parent.provenance` says who asserted the parent.
pub fn project(campaign: &Campaign, planning: &Run, runs: &[Run]) -> Result<Value> {
    ensure!(
        planning.id == campaign.planning_run_id
            && planning.planning_only
            && planning.request.repository == campaign.repository
            && crate::graph::repository_id(planning) == campaign.repository_identity,
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
    let parent = &campaign.parent;
    let display = parent.display.as_ref().map(|display| {
        json!({"url":display.url.as_deref().and_then(|url| public_url(&parent.forge, url))})
    });
    Ok(json!({
        "id":campaign.id,
        "repository":{"alias":campaign.repository,"identity":campaign.repository_identity},
        "parent":{
            "forge":{"provider":parent.forge.provider,"repository":parent.forge.repository,"number":parent.forge.number,"node_id":parent.forge.node_id},
            "key":parent.forge.key(),"revision":parent.revision,"display":display,"provenance":parent.provenance},
        "title":crate::lifecycle::safe_summary(&campaign.title,1000),"finish":planning.request.finish,"status":campaign.status,
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
