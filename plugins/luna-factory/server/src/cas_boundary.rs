//! LF-08 is an observation and durable planning boundary, not an execution adapter.
//! No CAS mutation route is implemented; prepared plans grant no dispatch authority.
use crate::{cas::PreparedDispatch, control::bounded_id};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CasRequest {
    pub prepared: PreparedDispatch,
    pub target_alias: String,
    pub task_id: String,
    pub subject: String,
    pub repository_stamp: String,
}
impl CasRequest {
    pub fn validate(&self) -> Result<()> {
        self.prepared.validate()?;
        ensure!(
            crate::config::valid_alias(&self.target_alias),
            "invalid_cas_alias"
        );
        ensure!(
            bounded_id(&self.task_id) && bounded_id(&self.subject),
            "invalid_cas_plan"
        );
        ensure!(
            self.repository_stamp.len() == 64
                && self.repository_stamp.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid_cas_repository_stamp"
        );
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InspectCas {
    pub target_alias: String,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub request_id: Option<String>,
}
