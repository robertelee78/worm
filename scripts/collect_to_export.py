#!/usr/bin/env python3
"""Turn collected rounds (data/rounds/*.jsonl) into per-player export files.

    python3 scripts/collect_to_export.py
    cargo run --release --example ghost_eval -- data/players/<id>.json

Dedups by round id (uploads are at-least-once), groups by deviceId, sorts
each player's rounds oldest-first, and writes data/players/<id8>.json in
the exact shape the browser's EXPORT MY ROUNDS produces — so ghost_eval
consumes real visitors and hand-exports identically (it sorts rounds
(endedAt, id) ascending itself, so on-disk order is cosmetic).
"""
import glob
import hashlib
import json
import os

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
IN_DIR = os.path.join(ROOT, "data", "rounds")
OUT_DIR = os.path.join(ROOT, "data", "players")


def main():
    seen, players = set(), {}
    for path in sorted(glob.glob(os.path.join(IN_DIR, "*.jsonl"))):
        for line in open(path):
            line = line.strip()
            if not line:
                continue
            try:
                rec = json.loads(line)
            except json.JSONDecodeError:
                continue
            try:
                device = str(rec.get("deviceId", "unknown"))
                rid = (device, str(rec.get("id") or f"anon:{hash(line)}"))
                replay = rec.get("replay")
                if rid in seen or not isinstance(replay, dict) or replay.get("v") != 2:
                    continue
                seen.add(rid)
                players.setdefault(device, []).append(rec)
            except Exception:
                continue

    os.makedirs(OUT_DIR, exist_ok=True)
    for device, rounds in players.items():
        rounds.sort(key=lambda r: (r.get("endedAt", 0) or 0, str(r.get("id", ""))), reverse=True)  # newest-first
        # Filename from a hash, never from client input: a hostile deviceId
        # was a path-traversal write primitive (external review), and 8-char
        # prefixes collided.
        out = os.path.join(OUT_DIR, hashlib.sha256(device.encode()).hexdigest()[:16] + ".json")
        with open(out, "w") as f:
            json.dump({"v": 1, "deviceId": device, "rounds": rounds}, f, separators=(",", ":"))
        frames = sum(r.get("frames") or 0 for r in rounds)
        print(f"{out}: {len(rounds)} round(s), {frames} frames")
    if not players:
        print("no collected rounds yet — data/rounds/ is empty")


if __name__ == "__main__":
    main()
