#!/usr/bin/env python3
"""Client-error beacon (ADR-017 addendum): pages report their own deaths."""
import datetime
import json
import os
import sys

MAX_BYTES = 4096
OUT_DIR = "/opt/worm/data/errors"


def main() -> None:
    try:
        length = int(os.environ.get("CONTENT_LENGTH") or 0)
    except ValueError:
        length = 0
    sys.stdout.write("Status: 204 No Content\r\nContent-Type: text/plain\r\n\r\n")
    if os.environ.get("REQUEST_METHOD") != "POST" or not 0 < length <= MAX_BYTES:
        return
    try:
        rec = json.loads(sys.stdin.buffer.read(length))
    except Exception:
        return
    if not (isinstance(rec, dict) and rec.get("v") == 1):
        return
    os.makedirs(OUT_DIR, exist_ok=True)
    day = datetime.datetime.utcnow().strftime("%Y%m%d")
    with open(os.path.join(OUT_DIR, f"{day}.jsonl"), "a") as f:
        f.write(json.dumps(rec, separators=(",", ":")) + "\n")


if __name__ == "__main__":
    main()
