#!/usr/bin/env python3
"""Round collector (ADR-017): same-origin POST target for finished rounds.

Hardened per external review: explicit conditions (no assert — it vanishes
under python -O), full record-shape validation including the ghost-v2
event stream, an Origin allowlist when the header is present, and size
caps. Appends one JSONL line per valid round."""
import datetime
import json
import os
import sys

MAX_BYTES = 262144
OUT_DIR = "/opt/worm/data/rounds"
ALLOWED_ORIGINS = {"https://worm.robertgpt.ai"}
ID_OK = set("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789:-.")


def respond(status: str) -> None:
    sys.stdout.write(f"Status: {status}\r\nContent-Type: text/plain\r\n\r\n")
    sys.stdout.flush()


def valid(rec) -> bool:
    if not isinstance(rec, dict) or rec.get("schemaVersion") != 1:
        return False
    device = rec.get("deviceId")
    rid = rec.get("id")
    if not (isinstance(device, str) and 1 <= len(device) <= 64 and set(device) <= ID_OK):
        return False
    if not (isinstance(rid, str) and 1 <= len(rid) <= 128 and set(rid) <= ID_OK):
        return False
    if not isinstance(rec.get("endedAt"), int) or not isinstance(rec.get("frames"), int):
        return False
    replay = rec.get("replay")
    if not (isinstance(replay, dict) and replay.get("v") == 2):
        return False
    seed = replay.get("seed")
    if not (isinstance(seed, str) and seed.isdigit() and len(seed) <= 20):
        return False
    ev = replay.get("ev")
    if not isinstance(ev, list) or len(ev) > 20000:
        return False
    for item in ev:
        if not (isinstance(item, list) and len(item) == 3
                and all(isinstance(x, int) and 0 <= x for x in item)
                and item[1] <= 3 and item[2] <= 3):
            return False
    return True


def main() -> None:
    origin = os.environ.get("HTTP_ORIGIN")
    if origin and origin not in ALLOWED_ORIGINS:
        return respond("403 Forbidden")
    try:
        length = int(os.environ.get("CONTENT_LENGTH") or 0)
    except ValueError:
        return respond("400 Bad Request")
    if os.environ.get("REQUEST_METHOD") != "POST" or not 0 < length <= MAX_BYTES:
        return respond("400 Bad Request")
    try:
        rec = json.loads(sys.stdin.buffer.read(length))
    except Exception:
        return respond("400 Bad Request")
    if not valid(rec):
        return respond("400 Bad Request")
    os.makedirs(OUT_DIR, exist_ok=True)
    day = datetime.datetime.utcnow().strftime("%Y%m%d")
    # O_APPEND single-write: records are far below PIPE_BUF*16; a whole json
    # line per write keeps concurrent CGI appends unsheared in practice.
    with open(os.path.join(OUT_DIR, f"{day}.jsonl"), "a") as f:
        f.write(json.dumps(rec, separators=(",", ":")) + "\n")
    respond("204 No Content")


if __name__ == "__main__":
    main()
