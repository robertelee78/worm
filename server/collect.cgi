#!/usr/bin/env python3
"""Round collector (ADR-017): same-origin POST target for finished rounds.

Accepts one round record (with its ghost log), validates shape and size,
appends one JSONL line to /opt/worm/data/rounds/YYYYMMDD.jsonl. No auth by
design — it is same-origin telemetry for a toy game; the payload is a
gameplay input log and a random per-browser id, nothing more. Size-capped
and parse-validated so it cannot be used as a dumping ground.
"""
import datetime
import json
import os
import sys

MAX_BYTES = 262144
OUT_DIR = "/opt/worm/data/rounds"


def respond(status: str) -> None:
    sys.stdout.write(f"Status: {status}\r\nContent-Type: text/plain\r\n\r\n")
    sys.stdout.flush()


def main() -> None:
    try:
        length = int(os.environ.get("CONTENT_LENGTH") or 0)
    except ValueError:
        return respond("400 Bad Request")
    if os.environ.get("REQUEST_METHOD") != "POST" or not 0 < length <= MAX_BYTES:
        return respond("400 Bad Request")
    body = sys.stdin.buffer.read(length)
    try:
        rec = json.loads(body)
        assert isinstance(rec, dict)
        assert rec.get("schemaVersion") == 1
        assert isinstance(rec.get("replay"), dict)
        assert isinstance(rec.get("deviceId"), str) and len(rec["deviceId"]) <= 64
    except Exception:
        return respond("400 Bad Request")
    os.makedirs(OUT_DIR, exist_ok=True)
    day = datetime.datetime.utcnow().strftime("%Y%m%d")
    with open(os.path.join(OUT_DIR, f"{day}.jsonl"), "a") as f:
        f.write(json.dumps(rec, separators=(",", ":")) + "\n")
    respond("204 No Content")


if __name__ == "__main__":
    main()
