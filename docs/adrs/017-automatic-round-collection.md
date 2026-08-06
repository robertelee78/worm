# ADR-017: Automatic Round Collection — Every Visitor Feeds the Flywheel

## Status
Implemented (hardened)

## Date
2026-08-06

## Updated
2026-08-06 — external review hardening: the collector validates the full
record shape including the v2 event stream with explicit conditions (no
assert), enforces an Origin allowlist, and ingestion derives filenames
from a hash (a hostile deviceId was a path-traversal write primitive) and
dedups on (deviceId, id). The browser marks a round uploaded only on a
CONFIRMED 2xx from the sequential backfill — sendBeacon "true" means
queued, not delivered, and Safari's in-flight quota silently drops bulk
beacons — with the ledger merged in a single transaction. Legacy records
are normalized on read so old eras can never put NaN in the DOM.

## Context

Forty-eight distinct visitors had already played (the link is circulating;
the access log even shows a LinkedInBot fetch) — and their play was locked
in their own browsers, reachable only by a manual EXPORT MY ROUNDS. The
owner's directive: get the learning data automatically. Two compounding
problems were found in the same log: only 3 of ~78 app.js fetches were the
current build, because Apache served no cache headers and stale cached
pages — some from broken windows — never expired.

## Decisions

**The entry page always revalidates.** The vhost now sends
`Cache-Control: no-cache, must-revalidate` for `/` and `/index.html` only;
versioned assets stay cacheable. Every visit gets the newest build, which
also delivers today's fixed game to everyone who saw a broken one.

**Finished rounds upload themselves.** At game over the round record —
including its ADR-016 ghost log — is sent to `/collect` via
`navigator.sendBeacon` (survives tab close; `fetch keepalive` fallback;
offline silently skips). Returning visitors backfill previously saved
rounds once, tracked by round id in the same IndexedDB. DISCLOSED in the
page footer in plain words: "finished rounds (your moves + the board seed,
nothing else) are sent to the arcade owner to make the CPU smarter." A
game whose premise is honest telemetry does not collect silently.

**The collector is a CGI, deliberately.** `server/collect.cgi` behind a
`ScriptAlias` on the existing Apache vhost: no new daemon, no new port,
dies with the request. Size-capped (256 KB), parse- and shape-validated,
appends JSONL to `data/rounds/YYYYMMDD.jsonl` (gitignored — visitor data
is not repository content).

**Seeds travel as strings.** The first real collected round exposed a
silent corruption: a u64 round seed through JavaScript's `JSON.parse`
loses precision above 2^53, and a replay from the mangled seed diverges.
The ghost wire format now quotes the seed; the evaluator accepts both.

**Ingest to evaluation.** `scripts/collect_to_export.py` dedups (uploads
are at-least-once), groups by device, and writes per-player files in the
exact EXPORT MY ROUNDS shape — so `ghost_eval` consumes real visitors and
hand exports identically. Proven end-to-end the day it was built: a live
headless session's round auto-uploaded, ingested, replayed, and scored.

## What this makes possible

Every future visitor becomes a benchmark persona for free. The read panel
`ghost_eval` produces per player is the CPU measured against every play
style that actually visits — and the nightly Darwin can rank candidate
temperaments by how well they read real humans, not scripts.
