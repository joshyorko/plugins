#!/usr/bin/env python3
"""Native app-server fixture for the isolated qualification canary only."""
import json
import pathlib
import sqlite3
import sys

home = pathlib.Path(__file__).parent
cwd = None
(home / "invocation.json").write_text(
    json.dumps({"argv": sys.argv[1:], "pid": __import__("os").getpid()})
)


def emit(value):
    print(json.dumps(value), flush=True)


def revoke():
    with sqlite3.connect(home / "state/runs.sqlite", timeout=5) as database:
        rows = database.execute(
            "SELECT fingerprint,payload FROM execution_qualifications"
        ).fetchall()
        for fingerprint, payload in rows:
            record = json.loads(payload)
            record["revoked_at"] = 1
            database.execute(
                "UPDATE execution_qualifications SET payload=?1 WHERE fingerprint=?2",
                (json.dumps(record), fingerprint),
            )


for line in sys.stdin:
    message = json.loads(line)
    with (home / "calls.jsonl").open("a") as history:
        history.write(json.dumps(message) + "\n")
    method = message.get("method")
    if method == "initialized":
        continue
    if "id" not in message:
        continue

    params = message.get("params", {})
    mode = (home / "mode").read_text() if (home / "mode").exists() else ""
    result = {}
    if method == "initialize":
        result = {"userAgent": "synthetic-qualification-canary"}
    elif method == "model/list":
        if mode == "revoke_on_model/list":
            revoke()
        result = {
            "data": [
                {
                    "model": "gpt-6-luna",
                    "supportedReasoningEfforts": [{"reasoningEffort": "high"}],
                }
            ]
        }
    elif method == "thread/start":
        if mode == "revoke_on_thread/start":
            revoke()
        cwd = pathlib.Path(params["cwd"])
        result = {
            "thread": {"id": "owner"},
            "model": "gpt-6-luna",
            "reasoningEffort": "high",
        }
    elif method == "thread/read":
        thread_id = params["threadId"]
        result = {
            "thread": {
                "id": thread_id,
                "model": "gpt-6-luna",
                "status": {"type": "idle"},
                "parentThreadId": None,
            }
        }
    elif method == "turn/start":
        objective = "\n".join(
            item.get("text", "")
            for item in params.get("input", [])
            if item.get("type") == "text"
        )
        assert "luna-factory-qualification-canary.txt" in objective
        assert cwd is not None
        (cwd / "luna-factory-qualification-canary.txt").write_text(
            "Luna Factory qualification canary v1\n"
        )
        if mode == "write_extra_file":
            (cwd / "unexpected.txt").write_text("unexpected\n")
        emit(
            {
                "method": "turn/completed",
                "params": {
                    "threadId": "owner",
                    "turn": {"id": "turn-one", "status": "completed", "items": []},
                },
            }
        )
        result = {"turn": {"id": "turn-one", "status": "completed"}}
    elif method in (
        "thread/list",
        "thread/turns/list",
        "thread/backgroundTerminals/list",
        "thread/items/list",
    ):
        result = {"data": []}
        if method == "thread/items/list" and mode == "active_command":
            result = {
                "data": [
                    {
                        "turnId": "turn-one",
                        "item": {
                            "id": "canary-command",
                            "type": "commandExecution",
                            "status": "inProgress",
                            "exitCode": None,
                        },
                    }
                ]
            }
    elif method == "thread/loaded/list":
        result = {"data": ["owner"]}

    emit({"id": message["id"], "result": result})
