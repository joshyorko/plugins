#!/usr/bin/env python3
"""Synthetic end-to-end factory protocol. No Codex or inference is used."""
import json
import pathlib
import sys

home = pathlib.Path(__file__).parent
history_path=home / "native_history.json"
history=json.loads(history_path.read_text()) if history_path.exists() else []
terminal_stopped = False
terminal_reads = {}
turn_number=len(history)
active={"owner":bool(history and history[-1]["status"]=="inProgress"),"child":bool(history and history[-1]["status"]=="inProgress")}

def emit(value):
    print(json.dumps(value), flush=True)

def report_for(turn):
    report=json.loads((home / "report.json").read_text())
    if report.pop("_fixture_bind_dispatch",False):
        for check in report.get("checks",[]):
            check["binding"]={"task_id":"objective","attempt_id":turn["client_id"],
                "intent_generation":1,"dispatch_generation":int(turn["id"].split("-")[-1]),
                "subject":report["subject"],"assumptions":{}}
    return report

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
        result = {"thread": {"id": "owner"}, "model": "gpt-6-luna", "reasoningEffort": "high", "modelProvider": "inherited-fixture"}
    elif method == "thread/resume":
        result = {"thread": {"id": params["threadId"]}, "model": "gpt-6-luna", "reasoningEffort": "high", "modelProvider": "different-fixture" if mode == "provider_drift" else "inherited-fixture"}
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
        report = report_for(history[-1])
        emit({"method":"item/completed","params":{"threadId":"owner","turnId":f"turn-{turn_number}","item":{"id":"final","type":"agentMessage","phase":"final_answer","text":json.dumps(report)}}})
        emit({"method":"turn/completed","params":{"threadId":"owner","turn":{"id":f"turn-{turn_number}","status":"completed","items":[]}}})
    elif method == "turn/steer" and mode.startswith("reroute_"):
        route={"threadId":"owner", "turnId":f"turn-{turn_number}", "fromModel":"gpt-6-luna", "toModel":"gpt-6-sol", "reason":"highRiskCyberActivity"}
        if mode in ("reroute_child", "reroute_child_completed", "reroute_child_unknown"):
            route.update(threadId="child", turnId=f"child-turn-{turn_number}")
            if mode == "reroute_child_unknown":
                route["turnId"] = "missing-child-turn"
            if mode == "reroute_child_completed":
                # The queued event is genuine, but history has advanced by the
                # time the runtime handles it and asks for correlation.
                active["child"] = False
        elif mode == "reroute_unrelated":
            route["threadId"]="unrelated"
        elif mode == "reroute_stale":
            route["turnId"]="old-turn"
        elif mode == "reroute_invalid":
            route["toModel"]="https://synthetic.invalid/sk-synthetic-secret"
        emit({"method":"model/rerouted","params":route})
        emit({"method":"model/rerouted","params":route})
    elif method == "thread/items/list" and mode == "finish" and params["threadId"] == "owner":
        result = {"data":[{"id":"final","type":"agentMessage","phase":"final_answer","text":json.dumps(report_for(history[-1]))}]}
    elif method == "thread/backgroundTerminals/list":
        tid=params["threadId"];terminal_reads[tid]=terminal_reads.get(tid,0)+1
        if mode=="terminal_preflight" and tid=="child" and terminal_reads[tid]==3:
            emit({"id":message["id"],"error":{"code":-32000,"message":"read-only preflight unavailable"}})
            continue
        if mode == "terminal_unsupported":
            emit({"id":message["id"],"error":{"code":-32601,"message":"unsupported"}})
            continue
        result = {"data": [{"processId":"42","itemId":"terminal-item","command":"synthetic sleep","cwd":"/synthetic"}] if mode.startswith("terminal_") and params["threadId"]=="child" and not terminal_stopped else []}
    elif method == "thread/backgroundTerminals/terminate":
        assert params == {"threadId":"child","processId":"42"}, "unowned termination"
        terminal_stopped = mode in ("terminal_exit", "terminal_disappeared", "terminal_reused_pid", "terminal_conflicting_exit", "terminal_preflight", "terminal_changed_pid")
        if mode == "terminal_lost_ack":
            emit({"id":message["id"],"error":{"code":-32000,"message":"uncertain outcome"}})
            continue
        result = {"terminated": True}
    elif method == "thread/items/list" and mode.startswith("terminal_") and mode != "terminal_unsupported":
        result = {"data": [{"id":"terminal-item","type":"commandExecution","processId":"42","status":"completed","exitCode":-9 if terminal_stopped and mode in ("terminal_exit", "terminal_preflight") else None}] if params["threadId"]=="child" else []}
    elif method == "thread/items/list":
        result = {"data": [{"id": "background-item", "type": "commandExecution", "processId": "background-process", "status": "completed", "exitCode": 0 if mode == "process_exited" else None}] if mode in ("background", "process_exited") and params["threadId"] == "child" else []}
    elif method == "thread/turns/list":
        result = {"data":[{"id":turn["id"],"status":turn["status"]} for turn in history] if params["threadId"]=="owner" else ([{"id":f"child-turn-{turn_number}","status":"inProgress"}] if active.get(params["threadId"]) else [])}
        if mode == "reroute_child_completed" and params["threadId"] == "child":
            result = {"data": [{"id": f"child-turn-{turn_number}", "status": "completed"}]}
    elif method == "turn/interrupt":
        active[params["threadId"]] = False
        if params["threadId"]=="owner" and history:
            history[-1]["status"]="interrupted";history_path.write_text(json.dumps(history))
    if method == "thread/items/list" and params["threadId"]=="child":
        if mode=="terminal_reused_pid":
            result["data"].insert(0,{"id":"old-item","type":"commandExecution","processId":"42","status":"completed","exitCode":0})
        if mode=="terminal_changed_pid" and terminal_stopped:
            result["data"][0]["processId"]="43"
            result["data"][0]["exitCode"]=-9
        if mode=="terminal_conflicting_exit":
            result["data"][0]["exitCode"]=0
    if method == "thread/items/list":
        result["data"]=[{"turnId":f"turn-{turn_number}","item":item} for item in result["data"]]
        if params["threadId"]=="owner":
            for turn in history:
                result["data"].append({"turnId":turn["id"],"item":{"id":"server-item-"+turn["id"],"type":"userMessage","clientId":turn.get("client_id"),"content":[]}})
                if mode=="lost_ack_completed" and turn["status"]=="completed":
                    result["data"].append({"turnId":turn["id"],"item":{"id":"final-"+turn["id"],"type":"agentMessage","phase":"final_answer","text":json.dumps(report_for(turn))}})
            if params.get("turnId"):
                result["data"]=[entry for entry in result["data"] if entry["turnId"]==params["turnId"]]
    emit({"id": message["id"], "result": result})
