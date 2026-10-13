#!/usr/bin/python3
"""Synthetic typed image app-server fixture; no network or real generation."""
import base64
import json
import sys
import time
from pathlib import Path

root = Path(__file__).parent
options = json.loads((root / "image-options.json").read_text())
item = {"type": "imageGeneration", "id": "image-item", "status": "completed", "failure": None, "savedPath": None, "result": options["image"]}

if options.get("failed"):
    item.update(status="failed", result="", failure=options.get("failure"))

def send(value):
    print(json.dumps(value), flush=True)

for line in sys.stdin:
    request = json.loads(line)
    method = request.get("method")
    with (root / "image-methods.jsonl").open("a") as log:
        log.write(json.dumps({"method": method, "params": request.get("params", {})}) + "\n")
    if "id" not in request:
        continue
    if method == "initialize":
        result = {}
    elif method == "account/read":
        result = {"account": {"type": options.get("account_type", "chatgpt")}, "requiresOpenaiAuth": True}
    elif method == "modelProvider/capabilities/read":
        result = {"imageGeneration": options.get("capable", True)}
    elif method == "thread/start":
        result = {"thread": {"id": "image-thread"}}
    elif method == "thread/read":
        result = {"thread": {"id": "image-thread", "turns": [{"id": "image-turn", "items": [item]}]}}
    elif method == "turn/start":
        if options.get("lost"):
            sys.exit(0)
        send({"id": request["id"], "result": {"turn": {"id": "image-turn"}}})
        started = dict(item, status="in_progress", result="")
        send({"method": "item/started", "params": {"threadId": "image-thread", "turnId": "image-turn", "item": started}})
        if options.get("cancel"):
            continue
        if options.get("gate"):
            until = time.monotonic() + 8
            while not (root / "release-image").exists() and time.monotonic() < until:
                time.sleep(0.01)
        send({"method": "item/completed", "params": {"threadId": "image-thread", "turnId": "image-turn", "item": item}})
        if options.get("disconnect_after_image"):
            sys.exit(0)
        send({"method": "turn/completed", "params": {"threadId": "image-thread", "turn": {"id": "image-turn", "status": "completed"}}})
        continue
    elif method == "turn/interrupt":
        send({"id": request["id"], "result": {}})
        send({"method": "turn/completed", "params": {"threadId": "image-thread", "turn": {"id": "image-turn", "status": "interrupted"}}})
        continue
    else:
        send({"id": request["id"], "error": {"code": -32601, "message": "Unsupported synthetic method"}})
        continue
    send({"id": request["id"], "result": result})
