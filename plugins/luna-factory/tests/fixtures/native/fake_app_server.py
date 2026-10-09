#!/usr/bin/env python3
"""Synthetic native protocol fixture. Never performs inference or external I/O."""
import json
import os
import sys
import time
import pathlib

counts = {"responses": 0}
initialized = False
history = []
reorder = []
terminal_reads = {}
preflight_barrier = None

def emit(value):
    print(json.dumps(value), flush=True)

for line in sys.stdin:
    message = json.loads(line)
    method = message.get("method")
    if method is None:
        counts["responses"] += 1
        continue
    counts[method] = counts.get(method, 0) + 1
    history.append(message)
    if method == "initialized":
        initialized = True
        continue
    request_id = message["id"]
    if method == "initialize":
        emit({"id": request_id, "result": {"userAgent": "synthetic-native-fixture"}})
        continue
    assert initialized, "Missing initialized notification"
    if method == "turn/start" and not message.get("params"):
        continue
    if method == "error":
        emit({"id": request_id, "error": {"code": -32001, "message": "fake-secret-must-not-leak"}})
        continue
    if method == "oversized":
        print("x" * 9000, flush=True)
        continue
    if method == "exit":
        break
    if method == "notify":
        emit({"method": "thread/started", "params": {"thread": {"id": "synthetic-thread"}}})
    if method == "approval":
        emit({"id": "server-approval", "method": "item/commandExecution/requestApproval", "params": {"threadId": "synthetic-thread"}})
    if method == "stop-reading":
        emit({"id": request_id, "result": {}})
        time.sleep(3600)
    if method == "reorder":
        reorder.append(message)
        if len(reorder) == 2:
            for pending in reversed(reorder):
                emit({"id": pending["id"], "result": pending["params"]})
        continue
    if method == "test/preflightBarrier":
        preflight_barrier = message["params"]
    elif preflight_barrier and method == preflight_barrier["method"]:
        pathlib.Path(preflight_barrier["entered"]).touch()
        for _ in range(400):
            if pathlib.Path(preflight_barrier["release"]).exists():
                break
            time.sleep(0.005)
        else:
            raise AssertionError("fixture preflight barrier timed out")
        preflight_barrier = None
    result = counts if method == "counts" else message.get("params")
    if method == "history":
        result = history[:-1]
    if method == "pid":
        result = os.getpid()
    if method == "model/list":
        result = {"data": [{"model": "gpt-6-luna", "supportedReasoningEfforts": [{"reasoningEffort": "high"}]}]}
    if method == "thread/start":
        result = {"thread": {"id": "owner"}, "model": "gpt-6-luna", "reasoningEffort": "high"}
    if method == "turn/start":
        result = {"turn": {"id": "turn-one", "status": "inProgress"}}
    if method == "thread/read":
        thread_id = message["params"]["threadId"]
        result = {"thread": {"id": thread_id, "model": "gpt-6-luna", "status": {"type": "idle"}, "parentThreadId": "owner" if thread_id == "ephemeral-child" else None}}
    if method == "thread/list":
        result = {"data": [{"id": "persisted-child", "parentThreadId": "owner"}]}
    if method == "thread/loaded/list":
        result = {"data": ["owner", "ephemeral-child"]}
    if method == "thread/items/list":
        item={"id":"cmd-1","type":"commandExecution","processId":"owned-pty","status":"completed","exitCode":None}
        result={"data":[item] if message["params"]["threadId"]=="malformed-history" else [{"turnId":"turn-one","item":item}]}
    if method == "thread/items/list" and message["params"]["threadId"] in ("correlation-owner","ambiguous-correlation"):
        result={"data":[{"turnId":"turn-one","item":{"id":"server-item-7","type":"userMessage","clientId":"dispatch-one","content":[]}}]}
        if message["params"]["threadId"]=="ambiguous-correlation":
            result["data"].append({"turnId":"turn-two","item":{"id":"server-item-8","type":"userMessage","clientId":"dispatch-one","content":[]}})
    if method == "thread/backgroundTerminals/list":
        params=message["params"];tid=params["threadId"]
        terminal_reads[tid]=terminal_reads.get(tid,0)+1
        row={"processId":"42","itemId":"terminal-item","command":"private command","cwd":"/private"}
        if tid=="terminal-malformed": row["processId"]="-1"
        if tid=="terminal-replaced" and terminal_reads[tid]>1: row["itemId"]="new-item"
        result={"data":[row]}
        if tid=="terminal-duplicate": result["data"].append(row)
        if tid=="terminals":
            if params.get("cursor") is None: result["nextCursor"]="next"
            else: result["data"]=[dict(row,processId="43",itemId="second-item")]
    if method == "thread/backgroundTerminals/terminate":
        result={"terminated":True}
    if method == "thread/turns/list":
        result = {"data": [{"id": "turn-one", "status": "inProgress"}, {"id": "old-turn", "status": "completed"}]}
    emit({"id": request_id, "result": result})
