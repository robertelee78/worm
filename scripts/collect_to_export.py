#!/usr/bin/env python3
"""Turn collected rounds (data/rounds/*.jsonl) into per-player export files.

    python3 scripts/collect_to_export.py
    cargo run --release --example ghost_eval -- data/players/<id>.json

Dedups by round id (uploads are at-least-once), groups by deviceId, sorts
each player's rounds oldest-first, and writes data/players/<id8>.json in
the exact shape the browser's EXPORT MY ROUNDS produces — so ghost_eval
consumes real visitors and hand-exports identically. ghost_eval reverses
its input (browser exports are newest-first), so rounds are written
newest-first here too.
"""
import glob
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
            rid = rec.get("id") or f"anon:{hash(line)}"
            if rid in seen or not isinstance(rec.get("replay"), dict):
                continue
            seen.add(rid)
            players.setdefault(rec.get("deviceId", "unknown"), []).append(rec)

    os.makedirs(OUT_DIR, exist_ok=True)
    for device, rounds in players.items():
        rounds.sort(key=lambda r: r.get("endedAt", 0), reverse=True)  # newest-first
        out = os.path.join(OUT_DIR, f"{device[:8]}.json")
        with open(out, "w") as f:
            json.dump({"v": 1, "deviceId": device, "rounds": rounds}, f, separators=(",", ":"))
        frames = sum(r.get("frames", 0) for r in rounds)
        print(f"{out}: {len(rounds)} round(s), {frames} frames")
    if not players:
        print("no collected rounds yet — data/rounds/ is empty")


if __name__ == "__main__":
    main()
