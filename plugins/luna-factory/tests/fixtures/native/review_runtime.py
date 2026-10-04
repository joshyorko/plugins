#!/usr/bin/env python3
"""Review regression fixture. No Codex, inference, network, or worker execution."""
import json
import pathlib
import sys

home = pathlib.Path(__file__).parent
state_path = home / "native-state.json"
state = json.loads(state_path.read_text()) if state_path.exists() else {
    "active": {"owner": False, "child": False}, "turn": 0, "closed_once": False
}
flooded = False


def emit(value):
    print(json.dumps(value), flush=True)


for line in sys.stdin:
    message = json.loads(line)
    with (home / "calls.jsonl").open("a") as log:
        log.write(json.dumps(message) + "\n")
    method = message.get("method")
    if "id" not in message:
        continue
    mode = (home / "mode").read_text() if (home / "mode").exists() else ""
    params = message.get("params", {})
    result = {}
    if method == "initialize":
        result = {"userAgent": "synthetic-review-fixture"}
    elif method == "model/list":
        result = {"data": [{"model": "gpt-6-luna", "supportedReasoningEfforts": [{"reasoningEffort": "high"}]}]}
    elif method == "thread/start":
        result = {"thread": {"id": "owner"}, "model": "gpt-6-luna", "reasoningEffort": "high"}
    elif method == "thread/resume":
        result = {"thread": {"id": "owner"}, "model": "gpt-6-luna", "reasoningEffort": "high"}
    elif method == "turn/start":
        state["turn"] += 1
        state["active"] = {"owner": True, "child": True}
        emit({"method": "item/completed", "params": {"threadId": "owner", "turnId": f'turn-{state["turn"]}', "item": {"type": "collabAgentToolCall", "tool": "spawnAgent", "receiverThreadIds": ["child"]}}})
        result = {"turn": {"id": f'turn-{state["turn"]}', "status": "inProgress"}}
    elif method == "thread/read":
        thread = params["threadId"]
        result = {"thread": {"id": thread, "model": "gpt-6-luna", "status": {"type": "active" if state["active"].get(thread) else "idle"}, "parentThreadId": "owner" if thread == "child" else None}}
    elif method == "thread/items/list":
        result = {"data": []}
    elif method == "thread/list":
        result = {"data": [{"id": "child", "parentThreadId": "owner"}] if state["turn"] else []}
    elif method == "thread/loaded/list":
        result = {"data": ["owner", "child"] if state["turn"] else ["owner"]}
    elif method == "thread/turns/list":
        result = {"data": [{"id": f'turn-{state["turn"]}', "status": "inProgress"}] if state["active"].get(params["threadId"]) else []}
    elif method == "turn/interrupt":
        state["active"][params["threadId"]] = False
    elif method == "turn/steer" and mode == "child_failure":
        state["active"]["child"] = False
        emit({"method": "turn/completed", "params": {"threadId": "child", "turn": {"id": "child-failure", "status": "failed", "items": []}}})
    state_path.write_text(json.dumps(state))
    emit({"id": message["id"], "result": result})
    if mode == "close_once" and method == "turn/start" and not state["closed_once"]:
        state["closed_once"] = True
        state_path.write_text(json.dumps(state))
        break
    if mode == "lag" and method == "thread/read" and not flooded:
        flooded = True
        for _ in range(5000):
            emit({"method": "item/agentMessage/delta", "params": {"threadId": "owner", "delta": "synthetic"}})
