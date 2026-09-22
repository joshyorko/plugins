#!/usr/bin/env python3
"""Emit a narrow Codex routing capability audit as JSON."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import tomllib
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


def _is_luna_model(slug: str, model: dict[str, Any]) -> bool:
    for key in ("role", "family", "model_family"):
        value = model.get(key)
        if isinstance(value, str):
            return value.lower() == "luna"
    return slug.rsplit("-", 1)[-1].lower() == "luna"


def summarize_catalog(catalog: dict[str, Any]) -> dict[str, Any]:
    summary: dict[str, Any] = {}
    for model in catalog.get("models", []):
        if not isinstance(model, dict):
            continue
        slug = model.get("slug")
        if not isinstance(slug, str) or not slug or slug in summary:
            continue
        levels = model.get("supported_reasoning_levels", [])
        if not isinstance(levels, list):
            levels = []
        summary[slug] = {
            "default_reasoning_effort": model.get("default_reasoning_level"),
            "supported_reasoning_efforts": [
                item.get("effort")
                for item in levels
                if isinstance(item, dict) and item.get("effort")
            ],
            "multi_agent_version": model.get("multi_agent_version"),
        }
    return summary


def assess_luna_route(
    catalog: dict[str, Any],
    *,
    requested_model: str | None,
    root_model: str | None,
) -> dict[str, Any]:
    """Describe safe Luna selection without choosing a generation implicitly."""
    entries = {
        model.get("slug"): model
        for model in catalog.get("models", [])
        if isinstance(model, dict)
        and isinstance(model.get("slug"), str)
        and model.get("slug")
    }
    available_luna = [
        slug for slug, model in entries.items() if _is_luna_model(slug, model)
    ]

    if requested_model is not None:
        model = entries.get(requested_model)
        if model is None:
            return {
                "status": "requested_model_unavailable",
                "requested_model": requested_model,
                "available_luna_models": available_luna,
                "fallback_proposed": False,
                "reason": (
                    "requested Luna model is absent from the live catalog; "
                    "report the capability or rollout limitation without a fallback"
                ),
            }
        if not _is_luna_model(requested_model, model):
            return {
                "status": "requested_model_not_luna",
                "requested_model": requested_model,
                "available_luna_models": available_luna,
                "fallback_proposed": False,
            }
        return {
            "status": "explicit_request",
            "requested_model": requested_model,
            "available_luna_models": available_luna,
            "fallback_proposed": False,
        }

    if root_model is not None and root_model in entries and _is_luna_model(
        root_model, entries[root_model]
    ):
        return {
            "status": "inherit_selected_root",
            "root_model": root_model,
            "available_luna_models": available_luna,
            "fallback_proposed": False,
            "reason": "inheritance was requested; effective child routing remains unverified",
        }

    return {
        "status": "explicit_selection_required",
        "available_luna_models": available_luna,
        "fallback_proposed": False,
        "reason": (
            "no explicit Luna request or selected Luna root resolves the route; "
            "do not silently choose a generation"
        ),
    }


def _comparison(requested: str | None, observed: str | None) -> str:
    if requested is None:
        return "not_requested"
    if observed is None:
        return "unverified"
    return "verified_match" if requested == observed else "verified_mismatch"


def compare_requested_to_observed(
    *,
    requested_model: str | None,
    requested_effort: str | None,
    observed_model: str | None,
    observed_effort: str | None,
) -> dict[str, str]:
    model = _comparison(requested_model, observed_model)
    effort = _comparison(requested_effort, observed_effort)
    relevant = [
        status
        for requested, status in (
            (requested_model, model),
            (requested_effort, effort),
        )
        if requested is not None
    ]
    if not relevant:
        overall = "no_request_to_verify"
    elif "verified_mismatch" in relevant:
        overall = "verified_mismatch"
    elif all(status == "verified_match" for status in relevant):
        overall = "verified_match"
    elif "verified_match" in relevant:
        overall = "partially_verified"
    else:
        overall = "unverified"
    return {"model": model, "reasoning_effort": effort, "overall": overall}


def run_command(*args: str) -> str:
    result = subprocess.run(
        args,
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    return result.stdout.strip()


def parse_features(output: str) -> dict[str, dict[str, str]]:
    features: dict[str, dict[str, str]] = {}
    for line in output.splitlines():
        parts = line.split()
        if len(parts) >= 3:
            features[parts[0]] = {
                "stage": " ".join(parts[1:-1]),
                "enabled": parts[-1],
            }
    return features


def load_config_summary(path: Path) -> dict[str, Any]:
    if not path.is_file():
        return {"available": False}
    with path.open("rb") as handle:
        config = tomllib.load(handle)
    agents = config.get("agents", {})
    return {
        "available": True,
        "root_model_configured": config.get("model"),
        "root_reasoning_effort_configured": config.get("model_reasoning_effort"),
        "max_spawned_threads_configured": agents.get(
            "max_concurrent_threads_per_session", agents.get("max_threads")
        ),
        "default_subagent_model_configured": agents.get("default_subagent_model"),
        "default_subagent_effort_configured": agents.get(
            "default_subagent_reasoning_effort"
        ),
    }


def observe_session(session_id: str | None, sessions_root: Path) -> dict[str, Any]:
    if not session_id:
        return {"available": False, "reason": "no_session_id"}
    matches = sorted(sessions_root.glob(f"**/rollout-*{session_id}.jsonl"))
    if not matches:
        return {"available": False, "reason": "rollout_not_found", "session_id": session_id}

    observed_model = None
    observed_effort = None
    depth = None
    path = matches[-1]
    with path.open(encoding="utf-8") as handle:
        for line in handle:
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            payload = event.get("payload", {})
            if event.get("type") == "session_meta":
                source = payload.get("source")
                if isinstance(source, dict):
                    depth = (
                        source.get("subagent", {})
                        .get("thread_spawn", {})
                        .get("depth")
                    )
            elif event.get("type") == "turn_context":
                observed_model = payload.get("model", observed_model)
                observed_effort = payload.get(
                    "effort",
                    payload.get(
                        "model_reasoning_effort",
                        payload.get("reasoning_effort", observed_effort),
                    ),
                )
    return {
        "available": True,
        "session_id": session_id,
        "rollout": str(path),
        "depth": depth,
        "observed_model": observed_model,
        "observed_reasoning_effort": observed_effort,
        "evidence_source": "turn_context",
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--requested-model")
    parser.add_argument("--requested-effort")
    parser.add_argument("--session-id", default=os.environ.get("CODEX_SESSION_ID"))
    codex_root = Path(os.environ.get("CODEX_HOME", Path.home() / ".codex"))
    parser.add_argument("--config", type=Path, default=codex_root / "config.toml")
    parser.add_argument("--sessions-root", type=Path, default=codex_root / "sessions")
    args = parser.parse_args()

    report: dict[str, Any] = {
        "audit_schema": 2,
        "audited_at_utc": datetime.now(timezone.utc).isoformat(),
        "errors": [],
    }
    catalog: dict[str, Any] = {"models": []}
    try:
        report["codex_version"] = run_command("codex", "--version")
    except (OSError, subprocess.CalledProcessError) as exc:
        report["errors"].append(f"codex_version: {exc}")
    try:
        catalog = json.loads(run_command("codex", "debug", "models"))
        report["models"] = summarize_catalog(catalog)
    except (OSError, subprocess.CalledProcessError, json.JSONDecodeError) as exc:
        report["models"] = {}
        report["errors"].append(f"model_catalog: {exc}")
    try:
        report["features"] = parse_features(run_command("codex", "features", "list"))
    except (OSError, subprocess.CalledProcessError) as exc:
        report["errors"].append(f"features: {exc}")

    try:
        report["config"] = load_config_summary(args.config)
    except (OSError, tomllib.TOMLDecodeError) as exc:
        report["config"] = {"available": False, "error": str(exc)}

    report["routing_capability"] = assess_luna_route(
        catalog,
        requested_model=args.requested_model,
        root_model=report.get("config", {}).get("root_model_configured"),
    )

    session = observe_session(args.session_id, args.sessions_root)
    report["session"] = session
    report["requested_routing"] = {
        "model": args.requested_model,
        "reasoning_effort": args.requested_effort,
    }
    report["routing_verification"] = compare_requested_to_observed(
        requested_model=args.requested_model,
        requested_effort=args.requested_effort,
        observed_model=session.get("observed_model"),
        observed_effort=session.get("observed_reasoning_effort"),
    )
    report["manual_checks_required"] = [
        "inspect live collaboration tool schemas",
        "test harmless spawn if delegation is required",
        "test nested spawn only if the execution graph requires it",
        "verify Sol override before relying on escalation",
    ]
    report["evidence_policy"] = {
        "catalog_support_proves_effective_routing": False,
        "accepted_request_proves_effective_routing": False,
        "missing_effective_metadata": "unverified",
    }
    json.dump(report, sys.stdout, indent=2, sort_keys=True)
    sys.stdout.write("\n")
    return 2 if report["errors"] else 0


if __name__ == "__main__":
    raise SystemExit(main())
