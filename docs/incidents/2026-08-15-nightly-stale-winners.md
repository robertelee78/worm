# Incident: the nightly reported winners for 8 nights while measuring nothing

## Summary
From 2026-08-08 through 2026-08-15, the nightly Darwin sweep crashed
every night, and its wrapper appended a "WINNERS FOUND" entry to
.darwin/WINNERS.md anyway — the same stale knob and digits
(DIRECT_GATE=0.3179, fitness [138,70] vs [136,70]) replayed under
eight different champion hashes. The weekly learning audit was
independently degraded by the same environment bug with its own
silencing. No nightly measurement of the CPU has occurred since the
manual sweep of 2026-08-07 18:26 — a window that spans the ADR-018
wake amendments, unread trigger discipline, the tri-shot value model
(ADR-027), and the Coil (ADR-028).

## Timeline
- 08-05 07:40 — Cargo.lock committed in v4 format (written by the
  interactive rustup cargo 1.97; the distro cargo is 1.75).
- 08-07 03:41 — nightly crashes: EMPTY print_tuning stdout ->
  unguarded IndexError. ROOT A: a 17-hour read-only verification
  agent squatted the shared target/ build lock. Fix landed same day:
  sandboxed CARGO_TARGET_DIR + a fail-loud guard in darwin.py.
- 08-07 18:26 — MANUAL sweep succeeds (interactive shell, cargo
  1.97). Its genuine winner is the last real measurement.
- 08-08 .. 08-15, 03:41 nightly — ROOT B: cron's PATH cargo (1.75)
  cannot parse lockfile v4; the (now-guarded) sweep exits 1. The
  WRAPPER then reads the untouched 08-07 last-run.json and appends
  "WINNERS FOUND" — nightly, for eight nights.
- 08-10 08:53 (Monday) — weekly-learning-audit cron: its three cargo
  probes fail on the same split, but `2>/dev/null` swallows the
  errors; the log records a probe-less audit that looks merely thin.
- 08-15 — owner asks "any overnight learnings?"; the identical-digits
  pattern and the exit-101 log line surface the incident.

## Five whys (root B, the sustained lie)
1. Why false WINNERS entries? The wrapper's winners block ran
   unconditionally after the sweep and read .darwin/last-run.json —
   an artifact with no run identity — so any past success replays.
2. Why did the sweep fail? Cron's minimal PATH resolves the distro
   cargo 1.75, which cannot read the v4 lockfile the interactive
   toolchain writes.
3. Why did the toolchains diverge? The repo is built interactively
   with rustup's cargo; nothing pinned or exported that toolchain for
   unattended contexts, and no preflight asserted parity.
4. Why no failure gate? The wrapper was designed to "surface winners
   OUT of the log so nobody reads cron logs" — it coupled the success
   path tightly to visibility and the failure path to an unread file.
   STATUS was captured and logged but never gated on.
5. Why undetected for 8 days? The failure MANUFACTURED a success
   signal. The one place the crash was visible is the place the
   design promised nobody needs to read. The identical-digits anomaly
   sat in WINNERS.md unflagged (no repeat-detection), and the weekly
   audit that might have caught a dead toolchain silenced its own
   stderr.

## Blast radius
- .darwin/WINNERS.md: 8 false entries (now under a CORRECTION note).
- Project memory: the wrapper attempts a ruflo memory store per
  "win"; semantic search finds no darwin-cron entries, so pollution
  is believed none, unverified.
- Weekly learning audit 08-10: probe sections silently empty.
- Decision risk: anyone promoting DIRECT_GATE=0.3179 off a "fresh"
  nightly would have acted on an Aug-7 measurement of a CPU seven
  ADR-amendments younger than HEAD. (Nobody promoted: the promotion
  step is human, which is what bounded the damage.)

## What already held
- ADR-015 discipline (the machine never edits defaults) bounded the
  blast radius to reporting, not behavior.
- The dirty-tree refusal kept receipts honest about WHICH champion —
  making the identical-fitness-across-champions anomaly visible in
  retrospect.

## Fixes
LANDED (cfcae65 + this change):
1. darwin-cron.sh exports ~/.cargo/bin ahead of the distro toolchain
   and stamps `cargo --version` into every log.
2. A failed sweep records "SWEEP FAILED … no winners recorded" and
   exits nonzero — the winners block is unreachable.
3. Freshness fence: winners only count from a last-run.json newer
   than a per-run start marker (artifact gains run identity).
4. Repeat tripwire: a winners block byte-identical to the previous
   entry is annotated as suspicious in WINNERS.md.
5. weekly-learning-audit.sh: same PATH export; `2>/dev/null` removed
   from cargo probes (stderr now lands in the audit log); an empty
   probe section is announced as A FAILURE line, not silence.
6. WINNERS.md correction entry over the stale rows.
REMAINING (accepted risks / follow-ups):
- Verify tonight's 03:41 run end-to-end (first honest sweep in 9
  days) — the real proof of fix.
- Dream Machine adoption for worm would make "witness every
  quantitative claim" structural rather than per-script discipline;
  /opt/dream-machine is on the machine, unconfigured for worm.

## The lesson (third occurrence this week)
A green light must be produced by observing the actual thing: the
import-only music check missed a TDZ crash, the served-HTML grep
missed an invisible button, and the nightly's success signal was
decoupled from the run that earned it. Success signals must carry
run identity, and failure paths must be at least as loud as success
paths — a question asked to an empty room is a silent crash, and so
is a victory declared to one.
