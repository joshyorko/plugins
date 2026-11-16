"""Model-facing MCP @tool, @resource, and @prompt entrypoints for rcc-actions-bootstrapper."""

import json
import sys
from typing import Any

from actions import ActionError, Response, Secret
from actions.mcp import prompt, resource, tool

from .models import (
    CancelResult,
    DiscoverReport,
    NamedQueryResult,
    PlanResult,
    ProjectKind,
    QueryName,
    RunRecord,
    ScaffoldResult,
    ValidateResult,
)
from .policy import (
    PolicyViolationError,
    default_datadir,
    get_approved_roots,
    inspect_binary_health,
)
from .runner import (
    cancel_project_run,
    execute_named_query,
    execute_project_run,
    load_run_record,
)
from .scaffold import (
    ACTION_TEMPLATES,
    ROBOT_TEMPLATES,
    plan_scaffold,
    scaffold_project,
    validate_project,
)


def _parse_json_arg(raw: str | dict[str, Any] | list[Any] | None, *, default: Any, field_name: str) -> Any:
    if raw is None or raw == "":
        return default
    if isinstance(raw, (dict, list)):
        return raw
    try:
        return json.loads(raw)
    except Exception as exc:
        raise PolicyViolationError(
            f"invalid_json_{field_name}",
            f"Argument '{field_name}' must be valid JSON.",
        ) from exc


@tool(
    read_only_hint=True,
    destructive_hint=False,
    idempotent_hint=True,
    open_world_hint=False,
)
def bootstrapper_discover() -> Response[DiscoverReport]:
    """Discover published Action and Robot templates, approved workspace roots, and binary health.

    Returns:
        Structured DiscoverReport with templates, approved roots, datadir, and binary digests.
    """
    roots = [str(p) for p in get_approved_roots()]
    datadir = str(default_datadir())
    as_health = inspect_binary_health("action-server", pinned_version="1.1.1")
    rcc_health = inspect_binary_health("rcc", pinned_version="18.19.11")

    capabilities = {
        "mcp_streamable_http": "qualified",
        "action_scaffolding": "qualified",
        "robot_scaffolding": "qualified",
        "work_items_producer_consumer": "qualified",
        "native_action_canvas_mcp": "qualified_linux_with_api_key" if sys.platform == "linux" else "not_qualified_non_linux",
        "managed_work_items_v1_http": "qualified_linux_with_api_key" if sys.platform == "linux" else "not_qualified_non_linux",
        "chatgpt_developer_mode_e1": "not_qualified",
        "macos_plugin_runtime": "not_qualified",
    }

    report = DiscoverReport(
        approved_roots=roots,
        datadir=datadir,
        action_templates=list(ACTION_TEMPLATES.values()),
        robot_templates=list(ROBOT_TEMPLATES.values()),
        binaries={
            "action-server": as_health,
            "rcc": rcc_health,
        },
        capabilities=capabilities,
    )
    return Response(result=report)


@tool(
    read_only_hint=True,
    destructive_hint=False,
    idempotent_hint=True,
    open_world_hint=False,
)
def bootstrapper_plan(
    project_kind: ProjectKind,
    template: str,
    target_dir: str,
) -> Response[PlanResult]:
    """Preview creating an Action package or RCC Robot project without modifying disk.

    Args:
        project_kind: 'action' or 'robot'.
        template: Published template identifier (e.g. 'basic', 'mcp-v2-showcase', 'workflow-producer-consumer', 'canvas-query', '01-python', '06-python-uv-native').
        target_dir: Destination directory path inside an approved workspace root.

    Returns:
        Structured dry-run PlanResult showing planned files, generator command, and whether target_dir is empty.
    """
    try:
        plan = plan_scaffold(project_kind=project_kind, template=template, target_dir=target_dir)
        return Response(result=plan)
    except PolicyViolationError as exc:
        return Response(error=f"[{exc.code}] {exc.message}")


@tool(
    read_only_hint=False,
    destructive_hint=False,
    idempotent_hint=False,
    open_world_hint=False,
)
def bootstrapper_scaffold(
    project_kind: ProjectKind,
    template: str,
    target_dir: str,
) -> Response[ScaffoldResult]:
    """Atomically scaffold a complete, runnable Action package or RCC Robot into an empty approved directory.

    Args:
        project_kind: 'action' or 'robot'.
        template: Published template identifier.
        target_dir: Empty destination directory inside an approved workspace root.

    Returns:
        ScaffoldResult with materialized files, sizes, SHA-256 digests, and applied normalizations.
    """
    try:
        result = scaffold_project(project_kind=project_kind, template=template, target_dir=target_dir)
        return Response(result=result)
    except PolicyViolationError as exc:
        return Response(error=f"[{exc.code}] {exc.message}")


@tool(
    read_only_hint=False,
    destructive_hint=False,
    idempotent_hint=True,
    open_world_hint=False,
)
def bootstrapper_validate(
    project_kind: ProjectKind,
    project_dir: str,
    run_tool_diagnostics: bool = False,
) -> Response[ValidateResult]:
    """Validate an Action package (package.yaml v2) or RCC Robot (robot.yaml + conda.yaml + tasks.py).

    Args:
        project_kind: 'action' or 'robot'.
        project_dir: Existing project directory inside an approved workspace root.
        run_tool_diagnostics: Optional flag to run 'rcc configuration diagnostics -r robot.yaml'.

    Returns:
        ValidateResult listing discovered entrypoints, checks run, and any validation issues.
    """
    try:
        result = validate_project(
            project_kind=project_kind,
            project_dir=project_dir,
            run_tool_diagnostics=run_tool_diagnostics,
        )
        return Response(result=result)
    except PolicyViolationError as exc:
        return Response(error=f"[{exc.code}] {exc.message}")


@tool(
    read_only_hint=False,
    destructive_hint=False,
    idempotent_hint=False,
    open_world_hint=False,
)
def bootstrapper_run(
    project_kind: ProjectKind,
    project_dir: str,
    target_name: str = "default",
    inputs_json: str = "{}",
    work_items_json: str = "[]",
    execution_mode: str = "local_python",
    timeout_seconds: float = 60.0,
    simulate_failure: bool = False,
    trust_project: bool = False,
    secret_token: Secret = Secret.model_validate(""),
) -> Response[RunRecord]:
    """Execute an approved Action or Robot workload with process-group ownership, work-items support, and secret redaction.

    Args:
        project_kind: 'action' or 'robot'.
        project_dir: Validated project directory inside an approved workspace root.
        target_name: Action function or Robot task name to run (or 'default' for the primary entrypoint).
        inputs_json: JSON object of keyword arguments for the entrypoint.
        work_items_json: Optional JSON array of input work item payloads for producer/consumer batch runs.
        execution_mode: 'local_python' (default fast deterministic runner) or 'rcc_cli' (full rcc run).
        timeout_seconds: Maximum runtime in seconds before process-group termination (0.1 to 600).
        simulate_failure: Trigger a controlled non-zero exit (code 2) and diagnostic artifact for failure/retry testing.
        trust_project: Explicit operator confirmation that the target project code is trusted to execute.
        secret_token: Optional Secret injected as RCC_BOOTSTRAPPER_SECRET_TOKEN in child env and redacted from logs.

    Returns:
        RunRecord with state, exit_code, redacted log tails, generated artifacts, and work-item summary.
    """
    try:
        inputs = _parse_json_arg(inputs_json, default={}, field_name="inputs_json")
        if not isinstance(inputs, dict):
            raise PolicyViolationError("invalid_inputs_json", "inputs_json must decode to a JSON object.")
        work_items = _parse_json_arg(work_items_json, default=[], field_name="work_items_json")
        if not isinstance(work_items, list):
            raise PolicyViolationError("invalid_work_items_json", "work_items_json must decode to a JSON array.")

        secrets_map: dict[str, str] = {}
        if secret_token is not None:
            try:
                sec_val = secret_token.value
                if sec_val:
                    secrets_map["RCC_BOOTSTRAPPER_SECRET_TOKEN"] = sec_val
            except Exception:
                # When no x-action-context secret is supplied, _SecretInActionContext.value
                # raises NoSecretInActionContextError; treat as no secret provided.
                pass

        record = execute_project_run(
            project_kind=project_kind,
            project_dir=project_dir,
            target_name=target_name,
            inputs=inputs,
            work_items=work_items,
            secrets=secrets_map,
            execution_mode=execution_mode,
            timeout_seconds=timeout_seconds,
            simulate_failure=simulate_failure,
            trust_project=trust_project,
        )
        if record.state != "succeeded":
            return Response(
                result=record,
                error=f"[{record.error_code or 'run_failed'}] {record.error_message or 'Run did not succeed.'}",
            )
        return Response(result=record)
    except PolicyViolationError as exc:
        return Response(error=f"[{exc.code}] {exc.message}")


@tool(
    read_only_hint=True,
    destructive_hint=False,
    idempotent_hint=True,
    open_world_hint=False,
)
def bootstrapper_status(run_id: str) -> Response[RunRecord]:
    """Inspect the live or crash-reconciled status of a tracked bootstrapper run.

    Args:
        run_id: Run identifier returned by bootstrapper_run.

    Returns:
        RunRecord with current or crash-reconciled state.
    """
    try:
        record = load_run_record(run_id)
        return Response(result=record)
    except PolicyViolationError as exc:
        return Response(error=f"[{exc.code}] {exc.message}")


@tool(
    read_only_hint=False,
    destructive_hint=True,
    idempotent_hint=True,
    open_world_hint=False,
)
def bootstrapper_cancel(run_id: str) -> Response[CancelResult]:
    """Cancel an active owned run and reap its process group.

    Args:
        run_id: Run identifier to cancel.

    Returns:
        CancelResult indicating whether the process group was signaled and termination confirmed.
    """
    try:
        result = cancel_project_run(run_id)
        return Response(result=result)
    except PolicyViolationError as exc:
        return Response(error=f"[{exc.code}] {exc.message}")


@tool(
    read_only_hint=True,
    destructive_hint=False,
    idempotent_hint=True,
    open_world_hint=False,
)
def bootstrapper_results(
    query: QueryName,
    project_dir: str = "",
    run_id: str = "",
) -> Response[NamedQueryResult]:
    """Execute a bounded named query over a scaffolded project or run journal.

    Args:
        query: One of 'project_summary', 'run_summary', 'work_items_summary', or 'failure_diagnosis'.
        project_dir: Optional approved project directory path.
        run_id: Optional run identifier.

    Returns:
        NamedQueryResult containing structured summary, redacted logs, and artifact metadata.
    """
    try:
        result = execute_named_query(
            query=query,
            project_dir=project_dir or None,
            run_id=run_id or None,
        )
        return Response(result=result)
    except PolicyViolationError as exc:
        return Response(error=f"[{exc.code}] {exc.message}")


@resource(
    "bootstrapper://catalog/templates",
    mime_type="application/json",
)
def bootstrapper_template_catalog() -> str:
    """Return the JSON catalog of published Action Server and RCC Robot templates."""
    payload = {
        "action_templates": [t.model_dump() for t in ACTION_TEMPLATES.values()],
        "robot_templates": [t.model_dump() for t in ROBOT_TEMPLATES.values()],
    }
    return json.dumps(payload, indent=2)


@resource(
    "bootstrapper://runs/{run_id}",
    mime_type="application/json",
)
def bootstrapper_run_record(run_id: str) -> str:
    """Return redacted JSON RunRecord for run_id.

    Args:
        run_id: Run identifier returned by bootstrapper_run.

    Returns:
        JSON string containing the redacted RunRecord.
    """
    try:
        rec = load_run_record(run_id)
        return rec.model_dump_json(indent=2)
    except PolicyViolationError as exc:
        raise ActionError(f"[{exc.code}] {exc.message}") from exc


@prompt
def bootstrapper_dogfood_workflow(project_kind: str = "action", template: str = "workflow-producer-consumer") -> str:
    """Generate a guided operator prompt for scaffolding and verifying a project.

    Args:
        project_kind: 'action' or 'robot'.
        template: Published template identifier to scaffold.

    Returns:
        Step-by-step prompt text for the model.
    """
    return (
        f"Use the rcc-bootstrapper MCP tools to create and verify a '{project_kind}' project "
        f"from template '{template}':\n"
        "1. Call bootstrapper_discover() to inspect approved roots and available templates.\n"
        f"2. Call bootstrapper_plan(project_kind='{project_kind}', template='{template}', target_dir=...) "
        "to preview the planned file tree without modifying disk.\n"
        "3. Call bootstrapper_scaffold(...) to atomically materialize the project in an empty directory.\n"
        "4. Call bootstrapper_validate(...) to verify package.yaml/robot.yaml schema and entrypoints.\n"
        "5. Call bootstrapper_run(...) to execute a harmless run or work-item batch, then inspect "
        "bootstrapper_results(query='run_summary') and bootstrapper_results(query='work_items_summary')."
    )
