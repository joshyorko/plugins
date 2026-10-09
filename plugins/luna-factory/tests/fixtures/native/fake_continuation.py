#!/usr/bin/env python3
"""Deterministic continuation transport. This never invokes Codex or inference."""
import json
import pathlib
import re
import sys
import shutil

home = pathlib.Path(__file__).parent
history_file = home / "history.json"
history = json.loads(history_file.read_text()) if history_file.exists() else []
late_preflight = False


def emit(value):
    print(json.dumps(value), flush=True)


def mode():
    path = home / "mode"
    return path.read_text() if path.exists() else ""


def save():
    history_file.write_text(json.dumps(history))


def report(turn):
    value = json.loads((home / "report.json").read_text())
    if value.pop("_bind_current", False):
        for check in value.get("checks", []):
            if check.pop("_current", False):
                check["binding"] = {
                    "task_id": turn["task"], "attempt_id": turn["client_id"],
                    "intent_generation": 1, "dispatch_generation": turn["generation"],
                    "subject": value["subject"], "assumptions": {},
                }
    return value


for line in sys.stdin:
    message = json.loads(line)
    with (home / "calls.jsonl").open("a") as output:
        output.write(json.dumps(message) + "\n")
    if "id" not in message:
        continue
    method, params = message["method"], message.get("params", {})
    result = {}
    if method == "initialize":
        result = {"userAgent": "synthetic-continuation"}
    elif method == "model/list":
        if history and history[-1]["status"] == "completed" and mode().startswith("late_"):
            late_preflight = True
        result = {"data": [{"model": "gpt-6-luna", "supportedReasoningEfforts": [{"reasoningEffort": "high"}]}]}
    elif method in ("thread/start", "thread/resume"):
        result = {"thread": {"id": "owner"}, "model": "gpt-6-luna", "reasoningEffort": "high", "modelProvider": "inherited-fixture"}
    elif method == "turn/start":
        packet = "\n".join(item.get("text", "") for item in params.get("input", []) if item.get("type") == "text")
        task = re.search(r"CONTROL: task_id=([^,]+),", packet)
        generation = len(history) + 1
        history.append({"id": f"turn-{generation}", "generation": generation,
                        "client_id": params.get("clientUserMessageId"),
                        "task": task.group(1) if task else "objective", "status": "inProgress"})
        save()
        if mode() == "lost_ack" and generation > 1:
            emit({"id": message["id"], "error": {"code": -32000, "message": "accepted but acknowledgement lost"}})
            continue
        result = {"turn": {"id": history[-1]["id"], "status": "inProgress"}}
    elif method == "turn/steer":
        turn = history[-1]
        assert params["expectedTurnId"] == turn["id"], "steer must target exact active turn"
        if mode() == "unknown_child":
            emit({"method": "item/completed", "params": {"threadId": "owner", "turnId": turn["id"], "item": {
                "type": "collabAgentToolCall", "tool": "spawnAgent", "receiverThreadIds": ["child"],
                "senderThreadId": "owner", "agentsStates": {"child": {"status": "running"}}}}})
        if mode() == "unknown_effect":
            emit({"method": "item/completed", "params": {"threadId": "owner", "turnId": turn["id"], "item": {
                "id": "external-push", "type": "commandExecution", "command": "git push origin fixture", "status": "completed", "exitCode": 0}}})
        turn["status"] = "completed"
        turn["report"] = report(turn)
        save()
        if mode() == "identity_drift":
            (home / "repo/.git").rename(home / "original-git")
            shutil.copytree(home / "original-git", home / "repo/.git")
        if mode() == "authority_drift":
            (home / "skills/luna-factory/SKILL.md").write_text("changed after native completion")
        if mode() == "completion_without_notification":
            emit({"id": message["id"], "result": {"turnId": turn["id"]}})
            continue
        emit({"method": "item/completed", "params": {"threadId": "owner", "turnId": turn["id"], "item": {
            "id": "final-" + turn["id"], "type": "agentMessage", "phase": "final_answer", "text": json.dumps(turn["report"])}}})
        emit({"method": "turn/completed", "params": {"threadId": "owner", "turn": {"id": turn["id"], "status": "completed", "items": []}}})
        result = {"turnId": turn["id"]}
    elif method == "thread/read":
        if late_preflight:
            late_preflight = False
            if mode() in ("late_source_drift", "late_ignored_predicate_drift"):
                (home / "repo/proof.txt").write_text("changed during final native preflight")
            elif mode() == "late_skill_drift":
                (home / "skills/luna-factory/SKILL.md").write_text("changed during final native preflight")
        tid = params["threadId"]
        state = "notLoaded" if tid == "child" else "active" if history and history[-1]["status"] == "inProgress" else "idle"
        result = {"thread": {"id": tid, "model": "gpt-6-luna", "status": {"type": state}, "parentThreadId": "owner" if tid == "child" else None}}
    elif method == "thread/list":
        result = {"data": [{"id": "child", "parentThreadId": "owner"}] if mode() == "unknown_child" else []}
    elif method == "thread/loaded/list":
        result = {"data": ["owner", "child"] if mode() == "unknown_child" else ["owner"]}
    elif method == "thread/backgroundTerminals/list":
        result = {"data": []}
    elif method == "thread/turns/list":
        result = {"data": [{"id": turn["id"], "status": turn["status"]} for turn in history] if params["threadId"] == "owner" else []}
    elif method == "thread/items/list":
        entries = []
        if params["threadId"] == "owner":
            for turn in history:
                entries.append({"turnId": turn["id"], "item": {"id": "user-" + turn["id"], "type": "userMessage", "clientId": turn["client_id"], "content": []}})
                if "report" in turn:
                    entries.append({"turnId": turn["id"], "item": {"id": "final-" + turn["id"], "type": "agentMessage", "phase": "final_answer", "text": json.dumps(turn["report"])}})
        result = {"data": [entry for entry in entries if not params.get("turnId") or entry["turnId"] == params["turnId"]]}
    elif method == "turn/interrupt":
        for turn in history:
            if turn["id"] == params["turnId"]:
                turn["status"] = "interrupted"
        save()
    emit({"id": message["id"], "result": result})
