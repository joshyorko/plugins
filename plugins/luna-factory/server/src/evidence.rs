//! Observed facts and owner semantic judgments have separate identities.
use crate::control::{Control, Criterion, TaskState, bounded_id};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    path::{Component, Path},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub task_id: String,
    pub attempt_id: String,
    pub intent_generation: u64,
    pub dispatch_generation: u64,
    pub subject: String,
    pub assumptions: BTreeMap<String, String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckSpec {
    pub id: String,
    pub kind: String,
    pub binding: Binding,
    pub path: Option<String>,
    pub sha256: Option<String>,
    pub native_item: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ObservedCheck {
    pub id: String,
    pub binding: Binding,
    pub kind: String,
    pub outcome: String,
    pub reason: String,
    pub path: Option<String>,
    pub expected_sha256: Option<String>,
    pub observed_sha256: Option<String>,
    pub exit_code: Option<i64>,
    pub output_completeness: String,
    pub environment: String,
}
impl ObservedCheck {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            bounded_id(&self.id)
                && bounded_id(&self.binding.task_id)
                && bounded_id(&self.binding.attempt_id)
                && bounded_id(&self.binding.subject)
                && self.binding.assumptions.len() <= 64
                && matches!(self.kind.as_str(), "file_sha256" | "native_command")
                && matches!(
                    self.outcome.as_str(),
                    "passed" | "failed" | "unverified" | "contradictory"
                )
                && self.reason.len() <= 256
                && self.path.as_ref().is_none_or(|p| p.len() <= 512)
                && [&self.expected_sha256, &self.observed_sha256]
                    .iter()
                    .all(|digest| digest.as_ref().is_none_or(|d| valid_digest(d))),
            "invalid_observed_check"
        );
        Ok(())
    }
}
fn valid_digest(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
pub fn binding_current(control: &Control, binding: &Binding) -> bool {
    control.settlement == crate::control::Settlement::Stopped
        && binding.subject == control.current_subject
        && binding.intent_generation == control.intent_generation
        && binding.dispatch_generation == control.dispatch_generation
        && binding
            .assumptions
            .iter()
            .all(|(id, value)| control.assumptions.get(id) == Some(value))
        && control.attempts.last().is_some_and(|attempt| {
            attempt.id == binding.attempt_id
                && attempt.task_id == binding.task_id
                && attempt.intent_generation == binding.intent_generation
                && attempt.dispatch_generation == binding.dispatch_generation
                && attempt.assumptions == binding.assumptions
                && matches!(attempt.phase.as_str(), "returned" | "stopped")
        })
}
pub fn criterion_current(control: &Control, criterion: &Criterion) -> bool {
    criterion.accepted
        && criterion.subject.as_deref() == Some(&control.current_subject)
        && !criterion.check_refs.is_empty()
        && criterion.check_refs.iter().all(|id| {
            let matching: Vec<_> = control
                .checks
                .iter()
                .filter(|check| &check.id == id)
                .collect();
            matching.len() == 1
                && matching[0].outcome == "passed"
                && binding_current(control, &matching[0].binding)
                && control
                    .tasks
                    .get(&matching[0].binding.task_id)
                    .is_some_and(|task| task.criteria.contains(&criterion.id))
        })
}
/// Each directory descriptor is anchored below the approved root. No path-based
/// follow is permitted, including a parent swapped while verification runs.
#[cfg(unix)]
fn open_predicate(root: &Path, relative: &Path) -> Result<std::fs::File> {
    use std::{
        ffi::CString,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::{ffi::OsStrExt, fs::OpenOptionsExt},
        },
    };
    let mut descriptor = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(root.canonicalize()?)?;
    let components: Vec<_> = relative.components().collect();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(name) = component else {
            anyhow::bail!("check_path_escape")
        };
        let name = CString::new(name.as_bytes()).context("invalid_check_path")?;
        let flags = libc::O_RDONLY
            | libc::O_NOFOLLOW
            | libc::O_CLOEXEC
            | libc::O_NONBLOCK
            | if index + 1 < components.len() {
                libc::O_DIRECTORY
            } else {
                0
            };
        // SAFETY: descriptor is live, name is NUL-terminated, and no creation
        // flags are used. The returned descriptor is owned exactly once.
        let fd = unsafe { libc::openat(descriptor.as_raw_fd(), name.as_ptr(), flags) };
        ensure!(fd >= 0, "check_open_rejected");
        descriptor = unsafe { std::fs::File::from_raw_fd(fd) };
    }
    Ok(descriptor)
}
#[cfg(not(unix))]
fn open_predicate(_root: &Path, _relative: &Path) -> Result<std::fs::File> {
    anyhow::bail!("file_predicate_platform_unsupported")
}
/// Hash a bounded regular file without retaining content or executing commands.
pub fn file_digest(root: &Path, relative: &str) -> Result<String> {
    ensure!(
        !relative.is_empty() && relative.len() <= 512,
        "invalid_check_path"
    );
    let relative = Path::new(relative);
    ensure!(
        relative
            .components()
            .all(|component| matches!(component,Component::Normal(name) if name!=".git")),
        "check_path_escape"
    );
    let mut file = open_predicate(root, relative)?;
    let before = file.metadata()?;
    ensure!(
        before.is_file() && before.len() <= 1024 * 1024,
        "check_requires_bounded_regular_file"
    );
    let mut data = vec![];
    (&mut file).take(1024 * 1024 + 1).read_to_end(&mut data)?;
    let after = file.metadata()?;
    ensure!(
        data.len() <= 1024 * 1024
            && before.len() == after.len()
            && before.modified()? == after.modified()?,
        "check_file_changed_during_read"
    );
    Ok(format!("{:x}", Sha256::digest(&data)))
}
fn record(control: &mut Control, mut observation: ObservedCheck) -> Result<()> {
    observation.validate()?;
    if let Some(previous) = control
        .checks
        .iter_mut()
        .find(|check| check.id == observation.id)
    {
        if previous.binding != observation.binding
            || previous.kind != observation.kind
            || previous.path != observation.path
            || previous.expected_sha256 != observation.expected_sha256
            || previous.observed_sha256 != observation.observed_sha256
            || previous.exit_code != observation.exit_code
        {
            observation.outcome = "contradictory".into();
            observation.reason = "check_identity_conflict".into();
        }
        if previous.outcome == "contradictory" {
            observation.outcome = "contradictory".into();
            observation.reason = "check_identity_conflict".into();
        }
        *previous = observation;
    } else {
        ensure!(control.checks.len() < 1000, "check_bound_reached");
        control.checks.push(observation);
    }
    Ok(())
}
/// Native exit is execution evidence. Upstream does not attest output completeness or clean environment.
pub fn observe_native(control: &mut Control, thread: &str, turn: &str, item: &Value) -> Result<()> {
    if item["type"] != "commandExecution" {
        return Ok(());
    }
    let Some(attempt) = control.attempts.last() else {
        return Ok(());
    };
    if attempt.turn_id.as_deref() != Some(turn) {
        return Ok(());
    }
    let Some(item_id) = item["id"].as_str() else {
        return Ok(());
    };
    let exit = item["exitCode"].as_i64();
    if !matches!(item["status"].as_str(), Some("completed" | "failed")) || exit.is_none() {
        return Ok(());
    }
    let binding = Binding {
        task_id: attempt.task_id.clone(),
        attempt_id: attempt.id.clone(),
        intent_generation: attempt.intent_generation,
        dispatch_generation: attempt.dispatch_generation,
        subject: control.current_subject.clone(),
        assumptions: attempt.assumptions.clone(),
    };
    record(
        control,
        ObservedCheck {
            id: format!("native:{thread}:{item_id}"),
            binding,
            kind: "native_command".into(),
            outcome: if exit == Some(0) {
                "unverified"
            } else {
                "failed"
            }
            .into(),
            reason: if exit == Some(0) {
                "output_and_environment_unverified"
            } else {
                "native_exit_nonzero"
            }
            .into(),
            path: None,
            expected_sha256: None,
            observed_sha256: None,
            exit_code: exit,
            output_completeness: "unverified".into(),
            environment: "unverified".into(),
        },
    )
}
pub fn reconcile_report(control: &mut Control, report: &Value, root: &Path) -> Result<()> {
    let checks: Vec<CheckSpec> = match report.get("checks") {
        Some(value) => serde_json::from_value(value.clone())?,
        None => vec![],
    };
    ensure!(checks.len() <= 32, "report_check_bound_reached");
    let mut ids = BTreeSet::new();
    for spec in checks {
        ensure!(
            bounded_id(&spec.id) && ids.insert(spec.id.clone()),
            "duplicate_or_invalid_check_id"
        );
        if spec.kind == "native_command" {
            ensure!(
                spec.path.is_none() && spec.sha256.is_none(),
                "invalid_native_check_spec"
            );
            // Owner declarations cannot manufacture command observations or environment attestations.
            continue;
        }
        ensure!(
            spec.kind == "file_sha256" && spec.native_item.is_none(),
            "unsupported_check_kind"
        );
        let expected = spec.sha256.as_deref().context("missing_check_digest")?;
        ensure!(valid_digest(expected), "invalid_check_digest");
        let path = spec.path.as_deref().context("missing_check_path")?;
        let current = binding_current(control, &spec.binding);
        let observed = if current {
            Some(file_digest(root, path)?)
        } else {
            None
        };
        let passed = observed.as_deref() == Some(expected);
        record(
            control,
            ObservedCheck {
                id: spec.id,
                binding: spec.binding,
                kind: spec.kind,
                outcome: if !current {
                    "unverified"
                } else if passed {
                    "passed"
                } else {
                    "failed"
                }
                .into(),
                reason: if !current {
                    "stale_or_foreign_binding"
                } else if passed {
                    "predicate_observed"
                } else {
                    "file_digest_mismatch"
                }
                .into(),
                path: Some(path.into()),
                expected_sha256: Some(expected.into()),
                observed_sha256: observed,
                exit_code: None,
                output_completeness: "not_applicable".into(),
                environment: "not_applicable".into(),
            },
        )?;
    }
    let receipts = report["acceptance"]
        .as_array()
        .context("missing_acceptance_receipts")?;
    for criterion in &mut control.criteria {
        criterion.accepted = false;
        criterion.reason = "evidence_missing".into();
        criterion.check_refs.clear();
        criterion.subject = None;
        if let Some(receipt) = receipts
            .iter()
            .find(|receipt| receipt["id"].as_str() == Some(&criterion.id))
        {
            let refs: Vec<String> = match receipt.get("check_refs") {
                Some(refs) => serde_json::from_value(refs.clone())?,
                None => vec![],
            };
            ensure!(
                refs.len() <= 32
                    && refs.iter().all(|id| bounded_id(id))
                    && refs.iter().collect::<BTreeSet<_>>().len() == refs.len(),
                "invalid_check_references"
            );
            criterion.accepted = receipt["accepted"] == true && receipt["passed"] == true;
            criterion.check_refs = refs;
            criterion.subject = Some(control.current_subject.clone());
            if !criterion.accepted {
                criterion.reason = "owner_acceptance_missing".into();
            }
        }
    }
    let reasons: Vec<_> = control
        .criteria
        .iter()
        .map(|criterion| {
            if criterion_current(control, criterion) {
                "criterion_proven"
            } else if criterion.check_refs.iter().any(|id| {
                control.checks.iter().any(|check| {
                    &check.id == id && matches!(check.outcome.as_str(), "failed" | "contradictory")
                })
            }) {
                "failed_or_contradictory_check"
            } else if criterion.accepted {
                "current_observed_check_missing"
            } else {
                "owner_acceptance_missing"
            }
        })
        .collect();
    for (criterion, reason) in control.criteria.iter_mut().zip(reasons) {
        criterion.reason = reason.into();
    }
    let proven: std::collections::BTreeSet<_> = control
        .criteria
        .iter()
        .filter(|criterion| criterion_current(control, criterion))
        .map(|criterion| criterion.id.clone())
        .collect();
    for task in control.tasks.values_mut() {
        if !task.criteria.is_empty()
            && task.necessity != "unverified_native_child"
            && task.criteria.iter().all(|id| proven.contains(id))
        {
            task.state = TaskState::Done;
            task.subject = control.current_subject.clone();
        }
    }

    if let Some(attempt) = control.attempts.last() {
        if control.certified_count() > attempt.certified_before {
            control.no_progress_attempts = 0;
        } else {
            control.no_progress_attempts = control.no_progress_attempts.saturating_add(1);
        }
    }
    Ok(())
}

/// A recognized command is an observation of a possible external effect, not
/// shell analysis or pre-action enforcement. Exit zero cannot settle delivery.
pub fn observe_external_command(control: &mut Control, thread: &str, item: &Value) -> Result<()> {
    if item["type"] != "commandExecution" {
        return Ok(());
    }
    let Some(command) = item["command"].as_str() else {
        return Ok(());
    };
    let kind = if command.contains("git push") {
        Some(crate::control::EffectKind::Push)
    } else if command.contains("gh pr create") || command.contains("gh pr edit") {
        Some(crate::control::EffectKind::Pr)
    } else {
        None
    };
    if let Some(kind) = kind {
        let id = item["id"].as_str().context("command_identity_missing")?;
        control.observe_effect(&format!("native-effect:{thread}:{id}"), kind)?;
    }
    Ok(())
}
/// Git subjects exclude ignored artifacts. Current file predicates therefore
/// need their own bounded observations before a read can advertise current proof.
pub fn revalidate_files(control: &mut Control, root: &Path) -> Result<bool> {
    let mut changed = false;
    let indices: Vec<_> = control
        .checks
        .iter()
        .enumerate()
        .filter(|(_, check)| {
            check.kind == "file_sha256"
                && check.outcome == "passed"
                && binding_current(control, &check.binding)
        })
        .map(|(index, _)| index)
        .collect();
    for index in indices {
        let check = &mut control.checks[index];
        let digest = check
            .path
            .as_deref()
            .and_then(|path| file_digest(root, path).ok());
        if digest != check.observed_sha256 {
            check.outcome = if digest.is_some() {
                "failed"
            } else {
                "unverified"
            }
            .into();
            check.reason = "observed_file_changed_or_missing".into();
            check.observed_sha256 = digest;
            changed = true;
        }
    }
    if changed {
        control.invalidate("observed_file_changed_or_missing");
        for criterion in &mut control.criteria {
            if criterion.check_refs.iter().any(|id| {
                control
                    .checks
                    .iter()
                    .any(|check| &check.id == id && check.outcome == "failed")
            }) {
                criterion.reason = "failed_or_contradictory_check".into();
            }
        }
    }
    Ok(changed)
}
