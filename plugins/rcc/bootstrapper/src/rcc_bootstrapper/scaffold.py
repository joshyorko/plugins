"""Template catalog, dry-run planning, atomic scaffolding, and project validation."""

from __future__ import annotations

import ast
import os
import re
import shutil
import subprocess
import uuid
from pathlib import Path
from typing import Any

import yaml

from .models import (
    PlannedFile,
    PlanResult,
    ProjectKind,
    ScaffoldedFile,
    ScaffoldResult,
    TemplateInfo,
    ValidateResult,
    ValidationIssue,
)
from .policy import (
    PolicyViolationError,
    check_destination_empty,
    resolve_approved_target,
    resolve_binary,
    sha256_file,
    validate_safe_identifier,
    verify_binary_before_execution,
)

ACTION_TEMPLATES: dict[str, TemplateInfo] = {
    "basic": TemplateInfo(
        name="basic",
        project_kind="action",
        source="action-server new --template basic",
        summary="Offline word-count action using Actions Core (spec-version v2).",
        expected_files=["package.yaml", "text_actions.py", "tests/test_template.py"],
        supports_work_items=False,
        supports_canvas=False,
    ),
    "mcp-v2-showcase": TemplateInfo(
        name="mcp-v2-showcase",
        project_kind="action",
        source="action-server new --template mcp-v2-showcase",
        summary="Stateless core MCP protocol showcase with @tool, @resource, and @prompt.",
        expected_files=["package.yaml", "showcase_actions.py", "tests/test_showcase_actions.py"],
        supports_work_items=False,
        supports_canvas=False,
    ),
    "workflow-producer-consumer": TemplateInfo(
        name="workflow-producer-consumer",
        project_kind="action",
        source="action-server new --template workflow-producer-consumer",
        summary="Producer-consumer workflow pattern using actions-work-items (producer, consumer, queue_status).",
        expected_files=["package.yaml", "workflow_actions.py", "README.md"],
        supports_work_items=True,
        supports_canvas=False,
    ),
    "canvas-query": TemplateInfo(
        name="canvas-query",
        project_kind="action",
        source="action-server new --template canvas-query",
        summary="Local typed Action rendered in the Action Canvas MCP App View (ui://action-canvas/v1/canvas.html).",
        expected_files=["package.yaml", "canvas_query.py", "canvas.html", "tests/test_canvas_query.py"],
        supports_work_items=False,
        supports_canvas=True,
    ),
}

ROBOT_TEMPLATES: dict[str, TemplateInfo] = {
    "01-python": TemplateInfo(
        name="01-python",
        project_kind="robot",
        source="rcc robot initialize -t 01-python",
        summary="Minimal Python RCC robot with robocorp.tasks and output artifact writing.",
        expected_files=["robot.yaml", "conda.yaml", "tasks.py"],
        supports_work_items=False,
        supports_canvas=False,
    ),
    "02-python-browser": TemplateInfo(
        name="02-python-browser",
        project_kind="robot",
        source="rcc robot initialize -t 02-python-browser",
        summary="Python browser automation RCC robot template.",
        expected_files=["robot.yaml", "conda.yaml", "tasks.py"],
        supports_work_items=False,
        supports_canvas=False,
    ),
    "03-python-work-items": TemplateInfo(
        name="03-python-work-items",
        project_kind="robot",
        source="rcc robot initialize -t 03-python-work-items",
        summary="Full multi-stage producer/consumer/reporter work-items RCC robot.",
        expected_files=["robot.yaml", "conda.yaml", "tasks.py"],
        supports_work_items=True,
        supports_canvas=False,
    ),
    "04-python-assistant-ai": TemplateInfo(
        name="04-python-assistant-ai",
        project_kind="robot",
        source="rcc robot initialize -t 04-python-assistant-ai",
        summary="Python AI assistant RCC robot template.",
        expected_files=["robot.yaml", "conda.yaml", "tasks.py"],
        supports_work_items=False,
        supports_canvas=False,
    ),
    "05-python-action-server-work-items": TemplateInfo(
        name="05-python-action-server-work-items",
        project_kind="robot",
        source="rcc robot initialize -t 05-python-action-server-work-items",
        summary="RCC robot integrated with Action Server work items.",
        expected_files=["robot.yaml", "conda.yaml", "tasks.py"],
        supports_work_items=True,
        supports_canvas=False,
    ),
    "06-python-uv-native": TemplateInfo(
        name="06-python-uv-native",
        project_kind="robot",
        source="rcc robot initialize -t 06-python-uv-native",
        summary="Fast uv-native Python RCC robot (no micromamba/conda-forge channels).",
        expected_files=["robot.yaml", "conda.yaml", "tasks.py"],
        supports_work_items=False,
        supports_canvas=False,
    ),
}

_ALPHA_ACTIONS_CORE_RE = re.compile(r"actions-core\s*=\s*1\.1\.0a\d+")


def get_template_info(project_kind: ProjectKind, template: str) -> TemplateInfo:
    """Resolve and validate template name for the given project_kind."""
    clean_template = validate_safe_identifier(template, field_name="template")
    catalog = ACTION_TEMPLATES if project_kind == "action" else ROBOT_TEMPLATES
    if clean_template not in catalog:
        supported = ", ".join(sorted(catalog.keys()))
        raise PolicyViolationError(
            "unknown_template",
            f"Unknown {project_kind} template '{clean_template}'. Supported templates: {supported}.",
        )
    return catalog[clean_template]


def plan_scaffold(
    project_kind: ProjectKind,
    template: str,
    target_dir: str | Path,
    *,
    approved_roots: list[str | Path] | None = None,
) -> PlanResult:
    """Preview scaffolding an Action or Robot project without modifying the filesystem."""
    tpl_info = get_template_info(project_kind, template)
    canonical_target, matching_root = resolve_approved_target(
        target_dir,
        approved_roots=approved_roots,
        allow_root_itself=False,
    )
    exists, is_empty, existing_entries = check_destination_empty(canonical_target)

    warnings: list[str] = []
    if exists and not is_empty:
        warnings.append(
            f"Destination already contains {len(existing_entries)} entry/entries "
            f"(e.g. {', '.join(existing_entries[:3])}); bootstrapper_scaffold will refuse to overwrite."
        )

    if project_kind == "action":
        generator_cmd = ["action-server", "new", "--name", str(canonical_target), "--template", tpl_info.name]
        normalizations = [
            "Normalize unreleased alpha pin actions-core=1.1.0a8 (if present) to released actions-core=1.1.0.",
            "Verify no file is named actions.py (preventing actions-core module shadowing).",
            "Verify all scaffolded files are non-empty before atomic rename publication.",
        ]
        validation_cmds = [
            f"action-server import --dir {canonical_target} --datadir <isolated_datadir>",
            "pytest tests",
        ]
    else:
        generator_cmd = ["rcc", "robot", "initialize", "-t", tpl_info.name, "-d", str(canonical_target)]
        normalizations = [
            "Run rcc robot initialize without -f in an isolated sibling staging directory.",
            "Verify robot.yaml, conda.yaml, and tasks.py are non-empty and syntactically valid.",
        ]
        validation_cmds = [
            f"rcc configuration diagnostics -r {canonical_target / 'robot.yaml'} --json",
            f"rcc run -r {canonical_target / 'robot.yaml'} --task RunTask",
        ]

    planned_files = [
        PlannedFile(
            relative_path=rel,
            description=f"Published {tpl_info.name} {project_kind} file ({rel})",
            non_empty_required=True,
        )
        for rel in tpl_info.expected_files
    ]

    return PlanResult(
        project_kind=project_kind,
        template=tpl_info.name,
        target_dir=str(canonical_target),
        approved_root=str(matching_root),
        destination_exists=exists,
        destination_empty=is_empty,
        can_scaffold=is_empty,
        generator_command=generator_cmd,
        planned_files=planned_files,
        post_scaffold_normalizations=normalizations,
        validation_commands=validation_cmds,
        warnings=warnings,
    )


def _normalize_action_package(staging_dir: Path, template_name: str) -> list[str]:
    """Apply safe post-generation normalizations to a staged Action package."""
    normalizations: list[str] = []
    pkg_yaml = staging_dir / "package.yaml"
    if pkg_yaml.is_file():
        raw_text = pkg_yaml.read_text(encoding="utf-8")
        if _ALPHA_ACTIONS_CORE_RE.search(raw_text):
            updated = _ALPHA_ACTIONS_CORE_RE.sub("actions-core=1.1.0", raw_text)
            pkg_yaml.write_text(updated, encoding="utf-8")
            normalizations.append("Replaced unreleased actions-core alpha pin with actions-core=1.1.0 in package.yaml.")

    # Guard against legacy actions.py module shadowing if any template ever produces actions.py
    shadow_file = staging_dir / "actions.py"
    if shadow_file.is_file():
        renamed = staging_dir / f"{template_name.replace('-', '_')}_actions.py"
        shadow_file.rename(renamed)
        normalizations.append(f"Renamed actions.py to {renamed.name} to prevent shadowing actions-core.")

    return normalizations


def _fallback_stage_action_template(staging_dir: Path, template_name: str) -> str:
    """Fallback generator only when action-server binary is absent in unit-test environments."""
    staging_dir.mkdir(parents=True, exist_ok=True)
    tests_dir = staging_dir / "tests"
    tests_dir.mkdir(parents=True, exist_ok=True)

    if template_name == "workflow-producer-consumer":
        (staging_dir / "package.yaml").write_text(
            "spec-version: v2\n"
            "name: Workflow Producer-Consumer Template\n"
            "description: Producer-consumer workflow pattern using work items.\n"
            "version: 0.0.1\n"
            "dependencies:\n"
            "  conda-forge:\n"
            "    - python=3.12.12\n"
            "    - uv=0.8.17\n"
            "  pypi:\n"
            "    - actions-core=1.1.0\n"
            "    - actions-work-items=0.5.0\n",
            encoding="utf-8",
        )
        (staging_dir / "workflow_actions.py").write_text(
            '"""Producer-Consumer Workflow Actions Template."""\n'
            "import json\nimport os\nfrom pathlib import Path\n"
            "from actions import Response, action, workitems\n"
            "from actions.work_items import State, get_context, init, seed_input\n\n"
            "def _ensure_init():\n"
            "    try:\n"
            "        get_context()\n"
            "    except RuntimeError:\n"
            "        if not os.environ.get('RC_WORKITEM_DB_PATH'):\n"
            "            datadir = os.environ.get('ACTION_SERVER_DATADIR') or os.environ.get('ACTIONS_RUNTIME_DATADIR', '.')\n"
            "            os.environ['RC_WORKITEM_DB_PATH'] = str(Path(datadir) / 'workitems.db')\n"
            "        init()\n\n"
            "@action\n"
            "def producer(items: str) -> Response[dict]:\n"
            "    _ensure_init()\n"
            "    parsed = json.loads(items)\n"
            "    created = [seed_input(payload=item) for item in parsed]\n"
            "    return Response(result={'created_count': len(created), 'item_ids': created})\n\n"
            "@action\n"
            "def consumer() -> Response[dict]:\n"
            "    _ensure_init()\n"
            "    processed = 0\n"
            "    for item in workitems.inputs:\n"
            "        with item:\n"
            "            workitems.outputs.create(payload={'processed': item.payload})\n"
            "            processed += 1\n"
            "    return Response(result={'processed_count': processed})\n\n"
            "@action\n"
            "def queue_status() -> Response[dict]:\n"
            "    _ensure_init()\n"
            "    ctx = get_context()\n"
            "    return Response(result={'done': len(ctx.adapter.list_items(state=State.DONE))})\n",
            encoding="utf-8",
        )
        (staging_dir / "README.md").write_text("# Workflow Producer-Consumer Template\n", encoding="utf-8")
    elif template_name == "mcp-v2-showcase":
        (staging_dir / "package.yaml").write_text(
            "spec-version: v2\n"
            "name: MCP v2 Showcase\n"
            "description: A local example of Action Server's stateless core MCP protocol.\n"
            "version: 0.0.1\n"
            "dependencies:\n"
            "  conda-forge:\n"
            "    - python=3.12.12\n"
            "    - uv=0.8.17\n"
            "  pypi:\n"
            "    - actions-core=1.1.0\n",
            encoding="utf-8",
        )
        (staging_dir / "showcase_actions.py").write_text(
            "from actions import Response\nfrom actions.mcp import prompt, resource, tool\n\n"
            "@tool(read_only_hint=True, idempotent_hint=True)\n"
            "def search_demo_catalog(query: str, limit: int = 10) -> Response[dict]:\n"
            "    return Response(result={'query': query.strip(), 'limit': limit, 'rows': [['alpha', 'Alpha guide']]})\n\n"
            "@resource('demo://catalog/overview')\n"
            "def catalog_overview() -> str:\n"
            "    return 'alpha field guide'\n\n"
            "@prompt\n"
            "def explain_demo_item(item_id: str) -> str:\n"
            "    return f'Explain {item_id}'\n",
            encoding="utf-8",
        )
        (tests_dir / "test_showcase_actions.py").write_text(
            "from showcase_actions import catalog_overview\n\ndef test_overview():\n    assert 'alpha' in catalog_overview()\n",
            encoding="utf-8",
        )
    elif template_name == "canvas-query":
        (staging_dir / "package.yaml").write_text(
            "spec-version: v2\n"
            "name: Canvas Query\n"
            "description: A local searchable catalog shown in the Canvas MCP App View.\n"
            "version: 0.0.1\n"
            "dependencies:\n"
            "  conda-forge:\n"
            "    - python=3.12.12\n"
            "    - uv=0.9.26\n"
            "  pypi:\n"
            "    - actions-core=1.1.0\n",
            encoding="utf-8",
        )
        (staging_dir / "canvas.html").write_text("<!doctype html><html><body>Canvas View</body></html>\n", encoding="utf-8")
        (staging_dir / "canvas_query.py").write_text(
            "from pathlib import Path\nfrom actions.mcp import resource, tool\n\n"
            "UI_URI = 'ui://action-canvas/v1/canvas.html?query-fixture=0.1'\n\n"
            "@resource(UI_URI, mime_type='text/html;profile=mcp-app', meta={'ui': {'csp': {}}})\n"
            "def canvas_view() -> str:\n"
            "    return Path(__file__).with_name('canvas.html').read_text(encoding='utf-8')\n\n"
            "@tool(read_only_hint=True, destructive_hint=False, idempotent_hint=True, open_world_hint=False, meta={'ui': {'resourceUri': UI_URI, 'visibility': ['model', 'app']}})\n"
            "def canvas_fixture_search(query: str) -> dict[str, object]:\n"
            "    return {'rows': [{'id': 'record-001', 'title': 'Alpha guide', 'category': 'Guide'}], 'artifact': None, 'error': None}\n",
            encoding="utf-8",
        )
        (tests_dir / "test_canvas_query.py").write_text(
            "from canvas_query import canvas_fixture_search\n\ndef test_search():\n    assert canvas_fixture_search('alpha')['error'] is None\n",
            encoding="utf-8",
        )
    else:
        (staging_dir / "package.yaml").write_text(
            "spec-version: v2\n"
            "name: Basic Template Package\n"
            "description: Action package description\n"
            "version: 0.0.1\n"
            "dependencies:\n"
            "  conda-forge:\n"
            "    - python=3.12.12\n"
            "    - uv=0.8.17\n"
            "  pypi:\n"
            "    - actions-core=1.1.0\n",
            encoding="utf-8",
        )
        (staging_dir / "text_actions.py").write_text(
            "from actions import Response, action\n\n"
            "@action\n"
            "def count_words(text: str) -> Response[int]:\n"
            "    return Response(result=len((text or '').split()))\n",
            encoding="utf-8",
        )
        (tests_dir / "test_template.py").write_text(
            "from text_actions import count_words\n\ndef test_count():\n    assert count_words('hello world').result == 2\n",
            encoding="utf-8",
        )
    return f"built-in-fallback:{template_name}"


def _fallback_stage_robot_template(staging_dir: Path, template_name: str) -> str:
    """Fallback generator only when rcc binary is absent in unit-test environments."""
    staging_dir.mkdir(parents=True, exist_ok=True)
    (staging_dir / "robot.yaml").write_text(
        "tasks:\n"
        "  RunTask:\n"
        "    shell: python -m robocorp.tasks run tasks.py -t minimal_task\n\n"
        "environmentConfigs:\n"
        "  - conda.yaml\n\n"
        "artifactsDir: output\n"
        "PATH:\n  - .\nPYTHONPATH:\n  - .\n",
        encoding="utf-8",
    )
    (staging_dir / "conda.yaml").write_text(
        "channels:\n  - conda-forge\ndependencies:\n  - python=3.12.11\n  - uv=0.13.8\n  - pip:\n    - robocorp==3.1.7\n",
        encoding="utf-8",
    )
    (staging_dir / "tasks.py").write_text(
        "from pathlib import Path\nimport os\n\n"
        "def minimal_task():\n"
        "    out = Path(os.environ.get('ROBOT_ARTIFACTS', 'output'))\n"
        "    out.mkdir(parents=True, exist_ok=True)\n"
        "    (out / 'greeting.txt').write_text('Hello from the minimal Python robot.\\n', encoding='utf-8')\n\n"
        "if __name__ == '__main__':\n"
        "    minimal_task()\n",
        encoding="utf-8",
    )
    return f"built-in-fallback:{template_name}"


def scaffold_project(
    project_kind: ProjectKind,
    template: str,
    target_dir: str | Path,
    *,
    approved_roots: list[str | Path] | None = None,
    action_server_bin: str | Path | None = None,
    rcc_bin: str | Path | None = None,
) -> ScaffoldResult:
    """Atomically scaffold a complete Action or Robot project into an empty approved directory."""
    tpl_info = get_template_info(project_kind, template)
    canonical_target, matching_root = resolve_approved_target(
        target_dir,
        approved_roots=approved_roots,
        allow_root_itself=False,
    )

    exists, is_empty, existing_entries = check_destination_empty(canonical_target)
    if exists and not is_empty:
        raise PolicyViolationError(
            "destination_not_empty",
            f"Refusing to scaffold into non-empty directory '{canonical_target}' "
            f"(contains {len(existing_entries)} entry/entries including {', '.join(existing_entries[:3])}).",
        )

    parent_dir = canonical_target.parent
    parent_dir.mkdir(parents=True, exist_ok=True)
    staging_dir = parent_dir / f".staging-{canonical_target.name}-{uuid.uuid4().hex[:8]}"

    normalizations: list[str] = []
    generator_used = ""
    pre_existing_empty_dir = exists and is_empty

    try:
        if project_kind == "action":
            resolved_as = resolve_binary("action-server", action_server_bin)
            if resolved_as:
                verified_as, _ = verify_binary_before_execution("action-server", resolved_as)
                cmd = [
                    str(verified_as),
                    "new",
                    "--name",
                    str(staging_dir),
                    "--template",
                    tpl_info.name,
                ]
                proc = subprocess.run(
                    cmd,
                    capture_output=True,
                    text=True,
                    timeout=60,
                    check=False,
                )
                if proc.returncode != 0:
                    raise PolicyViolationError(
                        "generator_failed",
                        f"action-server new failed with exit code {proc.returncode}.",
                    )
                generator_used = f"{verified_as} new --template {tpl_info.name}"
            else:
                if os.environ.get("RCC_BOOTSTRAPPER_TEST_MODE") != "1":
                    raise PolicyViolationError(
                        "generator_binary_missing",
                        "A verified action-server binary is required to generate published Action templates.",
                    )
                generator_used = _fallback_stage_action_template(staging_dir, tpl_info.name)

            normalizations.extend(_normalize_action_package(staging_dir, tpl_info.name))
        else:
            resolved_rcc = resolve_binary("rcc", rcc_bin)
            if resolved_rcc:
                verified_rcc, _ = verify_binary_before_execution("rcc", resolved_rcc)
                cmd = [
                    str(verified_rcc),
                    "robot",
                    "initialize",
                    "-t",
                    tpl_info.name,
                    "-d",
                    str(staging_dir),
                ]
                proc = subprocess.run(
                    cmd,
                    capture_output=True,
                    text=True,
                    timeout=60,
                    check=False,
                )
                if proc.returncode != 0:
                    raise PolicyViolationError(
                        "generator_failed",
                        f"rcc robot initialize failed with exit code {proc.returncode}.",
                    )
                generator_used = f"{verified_rcc} robot initialize -t {tpl_info.name}"
            else:
                if os.environ.get("RCC_BOOTSTRAPPER_TEST_MODE") != "1":
                    raise PolicyViolationError(
                        "generator_binary_missing",
                        "A verified rcc binary is required to generate published Robot templates.",
                    )
                generator_used = _fallback_stage_robot_template(staging_dir, tpl_info.name)

        # Verify expected files exist and are non-empty (fixing the legacy zero-byte wipe bug)
        for rel_expected in tpl_info.expected_files:
            expected_path = staging_dir / rel_expected
            if not expected_path.is_file():
                raise PolicyViolationError(
                    "staging_missing_expected_file",
                    f"Staged template did not produce expected file '{rel_expected}'.",
                )
            if expected_path.stat().st_size == 0:
                raise PolicyViolationError(
                    "staging_empty_file_detected",
                    f"Staged file '{rel_expected}' is 0 bytes; refusing to publish corrupted scaffold.",
                )

        # Re-check destination emptiness right before atomic publication
        exists_now, is_empty_now, entries_now = check_destination_empty(canonical_target)
        if exists_now and not is_empty_now:
            raise PolicyViolationError(
                "destination_not_empty",
                f"Destination '{canonical_target}' became non-empty during staging ({entries_now[:3]}).",
            )
        if exists_now and is_empty_now:
            canonical_target.rmdir()

        os.replace(staging_dir, canonical_target)
    except Exception:
        if staging_dir.exists():
            shutil.rmtree(staging_dir, ignore_errors=True)
        if pre_existing_empty_dir and not canonical_target.exists():
            canonical_target.mkdir(parents=True, exist_ok=True)
        raise

    materialized_files: list[ScaffoldedFile] = []
    for file_path in sorted(canonical_target.rglob("*")):
        if file_path.is_file():
            rel_str = file_path.relative_to(canonical_target).as_posix()
            materialized_files.append(
                ScaffoldedFile(
                    relative_path=rel_str,
                    size_bytes=file_path.stat().st_size,
                    sha256=sha256_file(file_path),
                )
            )

    return ScaffoldResult(
        project_kind=project_kind,
        template=tpl_info.name,
        target_dir=str(canonical_target),
        approved_root=str(matching_root),
        generator_used=generator_used,
        atomic_staging_used=True,
        files=materialized_files,
        normalizations_applied=normalizations,
        next_step=f"Call bootstrapper_validate(project_kind='{project_kind}', project_dir='{canonical_target}')",
    )


def validate_project(
    project_kind: ProjectKind,
    project_dir: str | Path,
    *,
    approved_roots: list[str | Path] | None = None,
    run_tool_diagnostics: bool = False,
    rcc_bin: str | Path | None = None,
) -> ValidateResult:
    """Validate an Action package or RCC Robot project inside an approved root."""
    canonical_dir, _ = resolve_approved_target(
        project_dir,
        approved_roots=approved_roots,
        allow_root_itself=False,
    )
    if not canonical_dir.is_dir():
        raise PolicyViolationError(
            "project_dir_not_found",
            f"Project directory '{canonical_dir}' does not exist.",
        )

    issues: list[ValidationIssue] = []
    checks_run: list[str] = ["canonical_path_check"]
    entrypoints: list[str] = []
    manifest_name: str | None = None

    if project_kind == "action":
        manifest_path = canonical_dir / "package.yaml"
        checks_run.append("package_yaml_schema")
        if not manifest_path.is_file() or manifest_path.stat().st_size == 0:
            issues.append(
                ValidationIssue(
                    severity="error",
                    code="missing_or_empty_package_yaml",
                    message="package.yaml is missing or empty.",
                    path=str(manifest_path),
                )
            )
        else:
            try:
                parsed: dict[str, Any] = yaml.safe_load(manifest_path.read_text(encoding="utf-8")) or {}
            except Exception as exc:
                parsed = {}
                issues.append(
                    ValidationIssue(
                        severity="error",
                        code="invalid_package_yaml",
                        message=f"Failed to parse package.yaml: {exc}",
                        path=str(manifest_path),
                    )
                )
            if isinstance(parsed, dict) and parsed:
                manifest_name = str(parsed.get("name") or "")
                spec_ver = str(parsed.get("spec-version") or "")
                if spec_ver != "v2":
                    issues.append(
                        ValidationIssue(
                            severity="error",
                            code="unsupported_spec_version",
                            message=f"Expected spec-version: v2 in package.yaml, found '{spec_ver}'.",
                            path=str(manifest_path),
                        )
                    )
                deps = parsed.get("dependencies") or {}
                pypi_deps = deps.get("pypi") if isinstance(deps, dict) else []
                for dep in pypi_deps or []:
                    if _ALPHA_ACTIONS_CORE_RE.search(str(dep)):
                        issues.append(
                            ValidationIssue(
                                severity="error",
                                code="unreleased_actions_core_pin",
                                message=f"Package pins unreleased alpha dependency '{dep}'; use actions-core=1.1.0.",
                                path=str(manifest_path),
                            )
                        )

        checks_run.append("actions_shadowing_check")
        shadow_path = canonical_dir / "actions.py"
        if shadow_path.is_file():
            issues.append(
                ValidationIssue(
                    severity="error",
                    code="actions_module_shadowing",
                    message="File 'actions.py' shadows the 'actions' package from actions-core; rename to '*_actions.py'.",
                    path=str(shadow_path),
                )
            )

        checks_run.append("python_ast_and_decorator_scan")
        py_files = [
            p
            for p in sorted(canonical_dir.rglob("*.py"))
            if "tests" not in p.parts and ".venv" not in p.parts and "venv" not in p.parts
        ]
        for py_file in py_files:
            if py_file.stat().st_size == 0:
                issues.append(
                    ValidationIssue(
                        severity="error",
                        code="empty_python_file",
                        message=f"Python module '{py_file.name}' is 0 bytes.",
                        path=str(py_file),
                    )
                )
                continue
            try:
                tree = ast.parse(py_file.read_text(encoding="utf-8"), filename=str(py_file))
                for node in ast.walk(tree):
                    if isinstance(node, ast.FunctionDef):
                        for dec in node.decorator_list:
                            dec_name = ""
                            if isinstance(dec, ast.Name):
                                dec_name = dec.id
                            elif isinstance(dec, ast.Attribute):
                                dec_name = dec.attr
                            elif isinstance(dec, ast.Call):
                                if isinstance(dec.func, ast.Name):
                                    dec_name = dec.func.id
                                elif isinstance(dec.func, ast.Attribute):
                                    dec_name = dec.func.attr
                            if dec_name in {"action", "tool", "resource", "prompt"}:
                                entrypoints.append(f"{py_file.relative_to(canonical_dir).as_posix()}:{node.name}")
            except SyntaxError as exc:
                issues.append(
                    ValidationIssue(
                        severity="error",
                        code="python_syntax_error",
                        message=f"SyntaxError in {py_file.name}:{exc.lineno}: {exc.msg}",
                        path=str(py_file),
                    )
                )

        if not entrypoints:
            issues.append(
                ValidationIssue(
                    severity="error",
                    code="no_action_or_mcp_entrypoints",
                    message="No @action, @tool, @resource, or @prompt functions found in package.",
                    path=str(canonical_dir),
                )
            )

    else:
        manifest_path = canonical_dir / "robot.yaml"
        conda_path = canonical_dir / "conda.yaml"
        checks_run.extend(["robot_yaml_schema", "conda_yaml_schema", "python_ast_scan"])

        if not manifest_path.is_file() or manifest_path.stat().st_size == 0:
            issues.append(
                ValidationIssue(
                    severity="error",
                    code="missing_or_empty_robot_yaml",
                    message="robot.yaml is missing or empty.",
                    path=str(manifest_path),
                )
            )
        else:
            try:
                robot_doc: dict[str, Any] = yaml.safe_load(manifest_path.read_text(encoding="utf-8")) or {}
            except Exception as exc:
                robot_doc = {}
                issues.append(
                    ValidationIssue(
                        severity="error",
                        code="invalid_robot_yaml",
                        message=f"Failed to parse robot.yaml: {exc}",
                        path=str(manifest_path),
                    )
                )
            tasks_map = robot_doc.get("tasks") if isinstance(robot_doc, dict) else None
            if not isinstance(tasks_map, dict) or not tasks_map:
                issues.append(
                    ValidationIssue(
                        severity="error",
                        code="missing_robot_tasks",
                        message="robot.yaml must define a non-empty 'tasks:' mapping.",
                        path=str(manifest_path),
                    )
                )
            else:
                entrypoints.extend(sorted(str(k) for k in tasks_map.keys()))
                manifest_name = canonical_dir.name

        if not conda_path.is_file() or conda_path.stat().st_size == 0:
            issues.append(
                ValidationIssue(
                    severity="error",
                    code="missing_or_empty_conda_yaml",
                    message="conda.yaml is missing or empty.",
                    path=str(conda_path),
                )
            )
        else:
            try:
                conda_doc = yaml.safe_load(conda_path.read_text(encoding="utf-8")) or {}
                if not isinstance(conda_doc, dict) or not conda_doc.get("dependencies"):
                    issues.append(
                        ValidationIssue(
                            severity="error",
                            code="invalid_conda_dependencies",
                            message="conda.yaml must define a non-empty 'dependencies:' list.",
                            path=str(conda_path),
                        )
                    )
            except Exception as exc:
                issues.append(
                    ValidationIssue(
                        severity="error",
                        code="invalid_conda_yaml",
                        message=f"Failed to parse conda.yaml: {exc}",
                        path=str(conda_path),
                    )
                )

        tasks_py = canonical_dir / "tasks.py"
        if not tasks_py.is_file() or tasks_py.stat().st_size == 0:
            issues.append(
                ValidationIssue(
                    severity="error",
                    code="missing_or_empty_tasks_py",
                    message="tasks.py is missing or empty.",
                    path=str(tasks_py),
                )
            )
        else:
            try:
                ast.parse(tasks_py.read_text(encoding="utf-8"), filename=str(tasks_py))
            except SyntaxError as exc:
                issues.append(
                    ValidationIssue(
                        severity="error",
                        code="python_syntax_error",
                        message=f"SyntaxError in tasks.py:{exc.lineno}: {exc.msg}",
                        path=str(tasks_py),
                    )
                )

        if run_tool_diagnostics:
            resolved_rcc = resolve_binary("rcc", rcc_bin)
            if resolved_rcc and manifest_path.is_file():
                verified_rcc, _ = verify_binary_before_execution("rcc", resolved_rcc)
                checks_run.append("rcc_configuration_diagnostics")
                proc = subprocess.run(
                    [str(verified_rcc), "configuration", "diagnostics", "-r", str(manifest_path), "--json"],
                    capture_output=True,
                    text=True,
                    timeout=30,
                    check=False,
                )
                if proc.returncode != 0:
                    issues.append(
                        ValidationIssue(
                            severity="warning",
                            code="rcc_diagnostics_nonzero",
                            message=f"rcc configuration diagnostics exited with code {proc.returncode}.",
                            path=str(manifest_path),
                        )
                    )

    has_errors = any(issue.severity == "error" for issue in issues)
    return ValidateResult(
        project_kind=project_kind,
        project_dir=str(canonical_dir),
        valid=not has_errors,
        manifest_path=str(manifest_path),
        manifest_name=manifest_name,
        entrypoints=entrypoints,
        checks_run=checks_run,
        issues=issues,
    )
