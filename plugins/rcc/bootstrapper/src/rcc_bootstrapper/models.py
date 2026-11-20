"""Typed Pydantic models for the RCC Actions & Robot Bootstrapper."""

from __future__ import annotations

from typing import Any, Literal

from pydantic import BaseModel, Field

ProjectKind = Literal["action", "robot"]
RunState = Literal[
    "running",
    "succeeded",
    "failed",
    "cancelled",
    "timed_out",
    "unknown_retained",
]
QueryName = Literal[
    "project_summary",
    "run_summary",
    "work_items_summary",
    "failure_diagnosis",
]


class TemplateInfo(BaseModel):
    """Metadata describing an Action or Robot template."""

    name: str = Field(description="Template identifier accepted by bootstrapper_plan and bootstrapper_scaffold.")
    project_kind: ProjectKind = Field(description="Whether this template produces an Action package or an RCC Robot.")
    source: str = Field(description="Published generator source ('action-server new' or 'rcc robot initialize').")
    summary: str = Field(description="Concise description of the template workflow.")
    expected_files: list[str] = Field(
        default_factory=list,
        description="Relative file paths always present after scaffolding.",
    )
    supports_work_items: bool = Field(
        default=False,
        description="Whether this template includes producer/consumer work-item entrypoints.",
    )
    supports_canvas: bool = Field(
        default=False,
        description="Whether this template includes an Action Canvas MCP App resource/tool.",
    )


class BinaryHealth(BaseModel):
    """Resolved binary path, version, and SHA-256 digest."""

    name: str
    available: bool
    executable_path: str | None = None
    version: str | None = None
    sha256: str | None = None
    pinned_version: str | None = None
    digest_verified: bool = False


class DiscoverReport(BaseModel):
    """Result of bootstrapper_discover."""

    package_name: str = "rcc-actions-bootstrapper"
    package_version: str = "0.6.0"
    approved_roots: list[str] = Field(default_factory=list)
    datadir: str
    action_templates: list[TemplateInfo] = Field(default_factory=list)
    robot_templates: list[TemplateInfo] = Field(default_factory=list)
    binaries: dict[str, BinaryHealth] = Field(default_factory=dict)
    capabilities: dict[str, str] = Field(default_factory=dict)


class PlannedFile(BaseModel):
    """Single file preview in a dry-run scaffold plan."""

    relative_path: str
    description: str
    non_empty_required: bool = True


class PlanResult(BaseModel):
    """Result of bootstrapper_plan (zero filesystem writes)."""

    project_kind: ProjectKind
    template: str
    target_dir: str
    approved_root: str
    destination_exists: bool
    destination_empty: bool
    can_scaffold: bool
    generator_command: list[str]
    planned_files: list[PlannedFile]
    post_scaffold_normalizations: list[str]
    validation_commands: list[str]
    warnings: list[str] = Field(default_factory=list)


class ScaffoldedFile(BaseModel):
    """Materialized file record after atomic scaffolding."""

    relative_path: str
    size_bytes: int
    sha256: str


class ScaffoldResult(BaseModel):
    """Result of bootstrapper_scaffold."""

    project_kind: ProjectKind
    template: str
    target_dir: str
    approved_root: str
    generator_used: str
    atomic_staging_used: bool = True
    files: list[ScaffoldedFile] = Field(default_factory=list)
    normalizations_applied: list[str] = Field(default_factory=list)
    next_step: str


class ValidationIssue(BaseModel):
    """Single validation error or warning."""

    severity: Literal["error", "warning"]
    code: str
    message: str
    path: str | None = None


class ValidateResult(BaseModel):
    """Result of bootstrapper_validate."""

    project_kind: ProjectKind
    project_dir: str
    valid: bool
    manifest_path: str
    manifest_name: str | None = None
    entrypoints: list[str] = Field(default_factory=list)
    checks_run: list[str] = Field(default_factory=list)
    issues: list[ValidationIssue] = Field(default_factory=list)


class ArtifactEntry(BaseModel):
    """Metadata about a generated artifact file."""

    relative_path: str
    size_bytes: int
    sha256: str


class WorkItemBatchSummary(BaseModel):
    """Summary of producer/consumer work item processing."""

    seeded_count: int = 0
    processed_count: int = 0
    done_count: int = 0
    failed_count: int = 0
    outputs_count: int = 0
    db_path: str | None = None
    sample_outputs: list[dict[str, Any]] = Field(default_factory=list)


class RunRecord(BaseModel):
    """Durable record of a bootstrapper_run invocation."""

    run_id: str
    project_kind: ProjectKind
    project_dir: str
    target_name: str
    state: RunState
    exit_code: int | None = None
    pid: int | None = None
    pgid: int | None = None
    started_at: str
    completed_at: str | None = None
    duration_ms: int | None = None
    command_summary: list[str] = Field(default_factory=list)
    stdout_tail: str = ""
    stderr_tail: str = ""
    error_code: str | None = None
    error_message: str | None = None
    secrets_redacted: bool = True
    artifacts: list[ArtifactEntry] = Field(default_factory=list)
    work_items: WorkItemBatchSummary | None = None
    reconciled_note: str | None = None


class CancelResult(BaseModel):
    """Result of bootstrapper_cancel."""

    run_id: str
    previous_state: RunState
    state: RunState
    process_group_signaled: bool
    termination_confirmed: bool
    message: str


class NamedQueryResult(BaseModel):
    """Result of bootstrapper_results bounded named query."""

    query: QueryName
    project_dir: str | None = None
    run_id: str | None = None
    summary: dict[str, Any] = Field(default_factory=dict)
    redacted_logs: dict[str, str] = Field(default_factory=dict)
    artifacts: list[ArtifactEntry] = Field(default_factory=list)
