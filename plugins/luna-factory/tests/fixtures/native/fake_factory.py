#!/usr/bin/env python3
"""Synthetic end-to-end factory protocol. No Codex or inference is used."""
import json
import pathlib
import sys

home = pathlib.Path(__file__).parent
history_path=home / "native_history.json"
history=json.loads(history_path.read_text()) if history_path.exists() else []
turn_number=len(history)
active={"owner":bool(history and history[-1]["status"]=="inProgress"),"child":bool(history and history[-1]["status"]=="inProgress")}

def emit(value):
    print(json.dumps(value), flush=True)

for line in sys.stdin:
    message = json.loads(line)
    with (home / "calls.jsonl").open("a") as file:
        file.write(json.dumps(message) + "\n")
    method = message.get("method")
    if method == "initialized":
        continue
    if "id" not in message:
        continue
    params = message.get("params", {})
    result = {}
    mode = (home / "mode").read_text() if (home / "mode").exists() else ""
    if method == "initialize":
        result = {"userAgent": "synthetic-factory"}
    elif method == "model/list":
        result = {"data": [{"model": "gpt-6-luna", "supportedReasoningEfforts": [{"reasoningEffort": "high"}]}]}
    elif method == "thread/start":
        result = {"thread": {"id": "owner"}, "model": "gpt-6-luna", "reasoningEffort": "high"}
    elif method == "thread/resume":
        result = {"thread": {"id": params["threadId"]}, "model": "gpt-6-luna", "reasoningEffort": "high"}
    elif method == "turn/start":
        if mode == "fail_resume" and turn_number > 0:
            emit({"id": message["id"], "error": {"code": -32000, "message": "synthetic failure"}})
            continue
        turn_number += 1
        active = {"owner": True, "child": True}
        history.append({"id":f"turn-{turn_number}","status":"inProgress","client_id":params.get("clientUserMessageId")})
        if mode=="lost_ack_completed":
            active={"owner":False,"child":False};history[-1]["status"]="completed"
            history_path.write_text(json.dumps(history))
            emit({"id":message["id"],"error":{"code":-32000,"message":"synthetic acknowledgement lost after execution"}})
            continue
        history_path.write_text(json.dumps(history))
        # Events intentionally precede the start response. A resubscription loses this proof.
        emit({"method": "item/completed", "params": {"threadId": "owner", "turnId": f"turn-{turn_number}", "item": {"type": "collabAgentToolCall", "tool": "spawnAgent", "receiverThreadIds": ["child"], "senderThreadId": "owner", "agentsStates":{"child":{"status":"running"}}}}})
        result = {"turn": {"id": f"turn-{turn_number}", "status": "inProgress"}}
    elif method == "thread/read":
        tid = params["threadId"]
        state = "notLoaded" if mode == "unknown_child" and tid == "child" else "active" if active.get(tid) else "idle"
        result = {"thread": {"id": tid, "model": "gpt-6-luna", "status": {"type": state}, "parentThreadId": "owner" if tid == "child" else None}}
    elif method == "thread/list":
        result = {"data": [{"id": "child", "parentThreadId": "owner"}] if turn_number else []}
    elif method == "thread/loaded/list":
        result = {"data": ["owner", "child"] if turn_number else ["owner"]}
    elif method == "turn/steer" and mode == "finish":
        active = {"owner": False,"child": False}
        history[-1]["status"]="completed";history_path.write_text(json.dumps(history))
        report = json.loads((home / "report.json").read_text())
        emit({"method":"item/completed","params":{"threadId":"owner","turnId":f"turn-{turn_number}","item":{"id":"final","type":"agentMessage","phase":"final_answer","text":json.dumps(report)}}})
        emit({"method":"turn/completed","params":{"threadId":"owner","turn":{"id":f"turn-{turn_number}","status":"completed","items":[]}}})
    elif method == "thread/items/list" and mode == "finish" and params["threadId"] == "owner":
        result = {"data":[{"id":"final","type":"agentMessage","phase":"final_answer","text":(home / "report.json").read_text()}]}
    elif method == "thread/items/list":
        result = {"data": [{"id": "background-item", "type": "commandExecution", "processId": "background-process", "status": "completed", "exitCode": 0 if mode == "process_exited" else None}] if mode in ("background", "process_exited") and params["threadId"] == "child" else []}
    elif method == "thread/turns/list":
        result = {"data":[{"id":turn["id"],"status":turn["status"]} for turn in history] if params["threadId"]=="owner" else ([{"id":f"child-turn-{turn_number}","status":"inProgress"}] if active.get(params["threadId"]) else [])}
    elif method == "turn/interrupt":
        active[params["threadId"]] = False
        if params["threadId"]=="owner" and history:
            history[-1]["status"]="interrupted";history_path.write_text(json.dumps(history))
    if method == "thread/items/list":
        result["data"]=[{"turnId":f"turn-{turn_number}","item":item} for item in result["data"]]
        if params["threadId"]=="owner":
            for turn in history:
                result["data"].append({"turnId":turn["id"],"item":{"id":"server-item-"+turn["id"],"type":"userMessage","clientId":turn.get("client_id"),"content":[]}})
                if mode=="lost_ack_completed" and turn["status"]=="completed":
                    result["data"].append({"turnId":turn["id"],"item":{"id":"final-"+turn["id"],"type":"agentMessage","phase":"final_answer","text":(home / "report.json").read_text()}})
            if params.get("turnId"):
                result["data"]=[entry for entry in result["data"] if entry["turnId"]==params["turnId"]]
    emit({"id": message["id"], "result": result})
