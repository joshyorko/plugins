#!/usr/bin/env python3
"""Exercise only disposable, revision-fenced planning graph MCP operations."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any

from acceptance_canary import McpClient, WORKBENCH_URI, CLIENT, CAPABILITIES, RELEASE_UI_SHA256
import hashlib


REQUEST = {
    "repository": "canary",
    "objective": "Verify isolated Luna Factory planning persistence.",
    "acceptance": ["Persist a disposable planning graph across container recreation."],
    "non_goals": ["Do not launch Codex, workers, inference, or CAS."],
    "finish": "local_candidate",
    "profile": "oci-canary",
    "capacity": 1,
    "repair_attempts": 0,
    "wall_seconds": 300,
    "idempotency_key": "luna-factory-oci-planning-canary-v1",
}


def require(condition: bool, message: str) -> None:
    if not condition:
        raise RuntimeError(message)


def check_surface(client: McpClient, expected_ui_sha256: str = RELEASE_UI_SHA256) -> dict[str, Any]:
    initialized = client.request(
        "initialize",
        {"protocolVersion": "2025-11-25", "clientInfo": CLIENT, "capabilities": CAPABILITIES},
    )
    server = initialized.get("serverInfo", {})
    require(server.get("name") == "luna-factory" and server.get("version") == "0.2.1", "unexpected server identity")
    tools = client.request("tools/list").get("tools", [])
    names = {tool.get("name") for tool in tools}
    required = {
        "get_factory_capabilities", "list_factory_runs", "read_factory_settings",
        "create_factory_graph", "get_factory_graph", "propose_factory_change", "apply_factory_change",
    }
    require(len(tools) == 22 and required.issubset(names), "MCP planning tool catalog mismatch")
    resources = client.request("resources/list").get("resources", [])
    require(any(item.get("uri") == WORKBENCH_URI for item in resources), "bundled workbench resource missing")
    read = client.request("resources/read", {"uri": WORKBENCH_URI}).get("contents", [])
    require(len(read) == 1 and read[0].get("mimeType") == "text/html;profile=mcp-app", "workbench resource shape mismatch")
    ui_hash = hashlib.sha256(read[0].get("text", "").encode()).hexdigest()
    require(ui_hash == expected_ui_sha256, "workbench does not match the expected pinned UI")
    capabilities = client.call("get_factory_capabilities")
    require(capabilities.get("status_inference_calls") == 0, "capability read reported inference")
    settings = client.call("read_factory_settings").get("values", {})
    require(settings.get("profile") == "oci-canary", "canary profile setting did not persist")
    return {
        "server_name": server["name"],
        "server_version": server["version"],
        "tool_count": len(tools),
        "workbench_sha256": ui_hash,
        "persisted_profile": settings["profile"],
        "inference_calls": 0,
    }


def graph_from(result: dict[str, Any]) -> dict[str, Any]:
    graph = result.get("graph")
    require(isinstance(graph, dict), "graph result missing")
    return graph


def call_must_error(client: McpClient, name: str, arguments: dict[str, Any], fragment: str) -> None:
    result = client.request("tools/call", {"name": name, "arguments": arguments})
    require(result.get("isError") is True, f"{name} unexpectedly accepted stale revision")
    text = " ".join(item.get("text", "") for item in result.get("content", []) if isinstance(item, dict))
    require(fragment in text, f"{name} rejected request for an unexpected reason")


def prepare(client: McpClient, receipt_path: Path, surface: dict[str, Any]) -> dict[str, Any]:
    existing = client.call("list_factory_runs").get("runs", [])
    require(len(existing) <= 1 and all(item.get("planning_only") is True for item in existing), "planning canary requires an empty or canary-only planning ledger")
    created = graph_from(client.call("create_factory_graph", REQUEST))
    replayed = graph_from(client.call("create_factory_graph", REQUEST))
    run_id = created["run_id"]
    require(run_id == replayed["run_id"] and created.get("planning_only") is True, "planning graph replay mismatch")
    require(not existing or existing[0].get("id") == run_id, "planning canary found an unrelated ledger run")

    initial = graph_from(client.call("get_factory_graph", {"run_id": run_id}))
    revision = initial["revision"]
    criterion = initial["criteria"][0]["id"]
    repository_id = initial["repository"]["identity"]
    source = {
        "provider": "local-canary",
        "repository_id": repository_id,
        "item_id": "oci-canary-source",
        "revision": initial["repository"]["base_head"],
    }
    nodes = [
        {"id": "oci-canary-a", "title": "Disposable canary prerequisite", "criterion_ids": [criterion], "dependencies": [], "source": {**source, "item_id": "oci-canary-source-a"}},
        {"id": "oci-canary-b", "title": "Disposable canary dependent", "criterion_ids": [criterion], "dependencies": [], "source": {**source, "item_id": "oci-canary-source-b"}},
    ]
    import_request = {
        "run_id": run_id,
        "expected_revision": revision,
        "idempotency_key": "luna-factory-oci-import-v1",
        "change": {"kind": "import_candidates", "nodes": nodes},
    }
    proposed = client.call("propose_factory_change", import_request)
    replay = client.call("propose_factory_change", import_request)
    proposal_id = proposed["proposal"]["id"]
    require(replay["proposal"]["id"] == proposal_id, "proposal replay created a second change")
    proposal_revision = graph_from(proposed)["revision"]
    applied = client.call("apply_factory_change", {"run_id": run_id, "change_id": proposal_id, "expected_revision": proposal_revision})
    repeated_apply = client.call("apply_factory_change", {"run_id": run_id, "change_id": proposal_id, "expected_revision": proposal_revision})
    graph = graph_from(applied)
    require(graph["revision"] == revision + 2 and graph_from(repeated_apply)["revision"] == revision + 2, "apply replay changed revision")

    stale = dict(import_request)
    stale["idempotency_key"] = "luna-factory-oci-stale-v1"
    call_must_error(client, "propose_factory_change", stale, "stale_control_revision")

    context_request = {
        "run_id": run_id,
        "expected_revision": graph["revision"],
        "idempotency_key": "luna-factory-oci-context-v1",
        "change": {"kind": "set_dependencies", "node_id": "oci-canary-b", "dependencies": ["oci-canary-a"]},
    }
    context_result = client.call("propose_factory_change", context_request)
    context_proposal = context_result["proposal"]
    graph = graph_from(client.call("apply_factory_change", {"run_id": run_id, "change_id": context_proposal["id"], "expected_revision": graph_from(context_result)["revision"]}))

    selection_request = {
        "run_id": run_id,
        "expected_revision": graph["revision"],
        "idempotency_key": "luna-factory-oci-selection-v1",
        "change": {"kind": "set_target", "node_id": "oci-canary-b", "target_id": "native-local"},
    }
    selection_result = client.call("propose_factory_change", selection_request)
    selection_proposal = selection_result["proposal"]
    graph = graph_from(client.call("apply_factory_change", {"run_id": run_id, "change_id": selection_proposal["id"], "expected_revision": graph_from(selection_result)["revision"]}))

    listed = client.call("list_factory_runs").get("runs", [])
    require(len(listed) == 1 and listed[0].get("id") == run_id, "planning graph missing from run list")
    final_graph = graph_from(client.call("get_factory_graph", {"run_id": run_id}))
    node = next((item for item in final_graph["nodes"] if item.get("id") == "oci-canary-b"), None)
    require(final_graph["revision"] == revision + 6, "unexpected final graph revision")
    require(node and node.get("dependencies") == ["oci-canary-a"] and node.get("target_preference") == "native-local", "planning context/selection mismatch")
    require(final_graph.get("claim", {}).get("held") is False, "planning graph unexpectedly holds a claim")
    record = {
        **surface,
        "run_id": run_id,
        "planning_only": True,
        "revision": final_graph["revision"],
        "node_count": len(final_graph["nodes"]),
        "context_dependency": "oci-canary-a",
        "selected_target": "native-local",
        "create_replay_same_id": True,
        "proposal_replay_same_id": True,
        "apply_replay_same_revision": True,
        "stale_revision_rejected": True,
        "claim_held": False,
        "inference_calls": 0,
        "worker_launches": 0,
        "cas_calls": 0,
        "execution_tool_calls": 0,
    }
    receipt_path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    receipt_path.write_text(json.dumps(record, sort_keys=True) + "\n", encoding="utf-8")
    receipt_path.chmod(0o600)
    return record


def verify(client: McpClient, receipt_path: Path, surface: dict[str, Any]) -> dict[str, Any]:
    expected = json.loads(receipt_path.read_text(encoding="utf-8"))
    graph = graph_from(client.call("get_factory_graph", {"run_id": expected["run_id"]}))
    runs = client.call("list_factory_runs").get("runs", [])
    require(len(runs) == 1 and runs[0].get("id") == expected["run_id"], "persisted planning graph missing")
    require(graph.get("planning_only") is True and graph.get("revision") == expected["revision"], "persisted graph revision changed")
    node = next((item for item in graph["nodes"] if item.get("id") == "oci-canary-b"), None)
    require(node and node.get("dependencies") == ["oci-canary-a"] and node.get("target_preference") == "native-local", "persisted graph selection/context changed")
    require(graph.get("claim", {}).get("held") is False, "persisted planning graph holds a claim")
    return {**surface, "mode": "verify", "run_id": expected["run_id"], "revision": graph["revision"], "persisted": True, "claim_held": False, "inference_calls": 0, "worker_launches": 0, "cas_calls": 0, "execution_tool_calls": 0}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--url", default="http://127.0.0.1:18788/mcp")
    parser.add_argument("--mode", choices=("prepare", "verify"), required=True)
    parser.add_argument("--receipt", type=Path, required=True)
    parser.add_argument("--expected-ui-sha256", default=RELEASE_UI_SHA256)
    args = parser.parse_args()
    try:
        client = McpClient(args.url)
        surface = check_surface(client, args.expected_ui_sha256)
        result = prepare(client, args.receipt, surface) if args.mode == "prepare" else verify(client, args.receipt, surface)
        print(json.dumps({"mode": args.mode, **result}, sort_keys=True))
    except (OSError, UnicodeError, ValueError, RuntimeError, KeyError, IndexError, TypeError, json.JSONDecodeError) as error:
        print(f"planning canary failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
