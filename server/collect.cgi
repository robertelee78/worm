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
    if not (isinstance(seed, str) and seed.isascii() and seed.isdigit()
            and 0 < len(seed) <= 20 and int(seed) <= 0xFFFFFFFFFFFFFFFF):
        return False
    # Mirror the evaluator's domain checks server-side so garbage never
    # lands on disk (kimi-k3 #6): board bounds, frame cap, event
    # monotonicity within the frame budget.
    w, h = replay.get("w"), replay.get("h")
    frames = replay.get("frames")
    if not (isinstance(w, int) and 10 <= w <= 400
            and isinstance(h, int) and 10 <= h <= 400
            and isinstance(frames, int) and 0 <= frames <= 100000):
        return False
    ev = replay.get("ev")
    if not isinstance(ev, list) or len(ev) > 20000:
        return False
    last = 0
    for item in ev:
        if not (isinstance(item, list) and len(item) == 3
                and all(isinstance(x, int) and 0 <= x for x in item)
                and item[1] <= 3 and item[2] <= 3
                and item[0] <= frames + 1 and item[0] >= last):
            return False
        last = item[0]
    return True


MAX_DAY_BYTES = 64 * 1024 * 1024  # disk-fill guard on a public endpoint


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
    path_today = os.path.join(OUT_DIR, f"{day}.jsonl")
    try:
        if os.path.getsize(path_today) > MAX_DAY_BYTES:
            return respond("429 Too Many Requests")
    except OSError:
        pass
    # O_APPEND single-write: records are far below PIPE_BUF*16; a whole json
    # line per write keeps concurrent CGI appends unsheared in practice.
    with open(os.path.join(OUT_DIR, f"{day}.jsonl"), "a") as f:
        f.write(json.dumps(rec, separators=(",", ":")) + "\n")
    respond("204 No Content")


if __name__ == "__main__":
    main()
