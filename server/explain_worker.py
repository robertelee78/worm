#!/usr/bin/env python3
"""The CPU's notebook (ADR-019): on-request deep round explanations.

Localhost-only worker (Apache ProxyPass fronts it at /explain; nothing new
listens publicly). Runs as the repo owner because `claude -p` needs their
subscription auth — the CGI user cannot have it, by design.

Contract with the player, enforced in the prompt: the model may narrate
ONLY the measurements we hand it. The numbers are computed HERE,
deterministically, from the round record and its ghost event stream; the
LLM adds voice, never facts. Responses cache by round id.
"""
import json
import os
import subprocess
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

PORT = 8791
MAX_BYTES = 262144
CACHE = "/opt/worm/data/explains"
DAY_CAP = 300
ID_OK = set("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789:-.")

DIRS = ["up", "down", "left", "right"]


def habit_stats(replay):
    """Deterministic depth: what the ghost stream actually shows."""
    ev = replay.get("ev", [])
    player_turns = [v for (_, k, v) in map(tuple, ev) if k == 0]
    fires = sum(1 for (_, k, _) in map(tuple, ev) if k == 1)
    # Relative turn tendency: compare consecutive player headings.
    left = right = 0
    prev = 3  # spawn heading: right
    for d in player_turns:
        # left-of mapping: up<-right<-down<-left<-up (indices 0,3,1,2)
        lefts = {3: 0, 0: 2, 2: 1, 1: 3}
        if d == lefts.get(prev):
            left += 1
        elif d != prev:
            right += 1
        prev = d
    return {
        "playerTurns": len(player_turns),
        "leftBreaks": left,
        "rightBreaks": right,
        "firesUsed": fires,
    }


def build_prompt(rec):
    replay = rec.get("replay") or {}
    habits = habit_stats(replay)
    models = sorted(
        (m for m in rec.get("models", []) if isinstance(m, dict) and m.get("samples", 0) > 0),
        key=lambda m: (m.get("hits", 0) / max(1, m.get("samples", 1))),
        reverse=True,
    )[:3]
    facts = {
        "outcome": {0: "the player won", 1: "the CPU won"}.get(rec.get("winner"), "a draw"),
        "cause": rec.get("cause") or "unknown",
        "frames": rec.get("frames"),
        "foodEaten": rec.get("foodEaten"),
        "predictionRate": rec.get("accuracy", {}).get("rate"),
        "predictionSamples": rec.get("accuracy", {}).get("samples"),
        "newSituationsMemorised": rec.get("memoryDelta"),
        "bestGuessers": [
            {
                "name": m.get("name"),
                "hitRate": round(m.get("hits", 0) / max(1, m.get("samples", 1)), 2),
                "samples": m.get("samples"),
            }
            for m in models
        ],
        "cpuFinalAction": rec.get("decisionReason"),
        "cpuFinalForecastSource": rec.get("decisionSourceName"),
        "playerHabits": habits,
        # ADR-020: the earned-evidence ledger — which family half funds
        # the difficulty, and the turn book's side read of the player.
        "earnedRead": (rec.get("book") or {}).get("earned"),
        "earnedReadSource": (rec.get("book") or {}).get("earnedSource"),
        "turnBookSideAccuracy": (rec.get("book") or {}).get("sideAccuracy"),
        "turnBookSideEvents": (rec.get("book") or {}).get("sideEvents"),
    }
    return (
        "You are the CPU opponent's notebook in a snake/Tron arcade game whose "
        "whole premise is HONEST, measurable learning of one specific human. "
        "Write the deeper post-round explanation the player asked for.\n\n"
        "HARD RULES: use ONLY the measurements below — never invent a number, "
        "event, or read that is not here. If a field is missing or small, say "
        "less rather than guessing. Speak to the player as 'you'. The CPU is "
        "'it'. 90-130 words, plain prose, no lists, no headers. Structure: what "
        "happened, what it learned about you this round (cite 1-3 of the "
        "numbers — if earnedReadSource is 'book', the thing reading you is "
        "its turn book calling WHICH WAY you swerve, and turnBookSideAccuracy "
        "is that read; say so plainly), and ONE practical tip for beating it "
        "next round based only on these measurements. Tone: a rival's notebook — sharp, warm, a "
        "little unnerving.\n\n"
        f"MEASUREMENTS (JSON): {json.dumps(facts, separators=(',', ':'))}"
    )


def day_capped():
    stamp = time.strftime("%Y%m%d")
    marker = os.path.join(CACHE, f".count-{stamp}")
    n = 0
    try:
        n = int(open(marker).read())
    except Exception:
        pass
    if n >= DAY_CAP:
        return True
    open(marker, "w").write(str(n + 1))
    return False


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_POST(self):
        if self.path != "/explain":
            return self.reply(404, "not found")
        try:
            length = int(self.headers.get("Content-Length") or 0)
        except ValueError:
            return self.reply(400, "bad request")
        if not 0 < length <= MAX_BYTES:
            return self.reply(400, "bad request")
        try:
            rec = json.loads(self.rfile.read(length))
            rid = rec.get("id")
            assert isinstance(rec, dict) and isinstance(rid, str)
            assert 1 <= len(rid) <= 128 and set(rid) <= ID_OK
        except Exception:
            return self.reply(400, "bad request")

        os.makedirs(CACHE, exist_ok=True)
        cache_path = os.path.join(
            CACHE, "".join(c if c.isalnum() else "_" for c in rid)[:96] + ".txt"
        )
        if os.path.exists(cache_path):
            return self.reply(200, open(cache_path).read())
        if day_capped():
            return self.reply(429, "the notebook is resting today — try tomorrow")
        try:
            out = subprocess.run(
                ["claude", "-p", "--model", "haiku"],
                input=build_prompt(rec),
                capture_output=True,
                text=True,
                timeout=45,
            )
            text = out.stdout.strip()
            if out.returncode != 0 or not text:
                return self.reply(503, "the notebook is unavailable right now")
        except Exception:
            return self.reply(503, "the notebook is unavailable right now")
        open(cache_path, "w").write(text)
        self.reply(200, text)

    def reply(self, code, body):
        data = body.encode()
        self.send_response(code)
        self.send_header("Content-Type", "text/plain; charset=utf-8")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)


if __name__ == "__main__":
    ThreadingHTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
