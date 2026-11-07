"""Type `uuid` to get new UUIDs. Press Enter to copy one. Needs Python 3 on PATH."""
import json
import sys
import uuid

COUNT = 3
offered = {}


def query(text):
    offered.clear()
    items = []
    for i in range(COUNT):
        value = str(uuid.uuid4())
        offered[str(i)] = value
        items.append({"id": str(i), "title": value, "subtitle": "Press Enter to copy"})
    return {"items": items}


def run(item_id):
    return {"effects": [{"type": "copy", "text": offered[item_id]}]}


# Exits when Glint closes stdin, which is how extensions must stop.
for line in sys.stdin:
    request = json.loads(line)
    params = request["params"]
    reply = {"jsonrpc": "2.0", "id": request["id"]}
    try:
        if request["method"] == "query":
            reply["result"] = query(params["text"])
        else:
            reply["result"] = run(params["id"])
    except Exception as error:
        reply["error"] = {"code": -1, "message": str(error)}
    print(json.dumps(reply), flush=True)
