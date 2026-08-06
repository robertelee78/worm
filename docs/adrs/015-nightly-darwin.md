# ADR-015: Nightly Darwin — Continual Improvement With Human Promotion

## Status
Implemented

## Date
2026-08-06

## Context

The owner's directive is continual improvement. The first worm-native
Darwin sweep proved the mechanism the same day it was built: 22
single-knob candidates against the fixed-seed gauntlet, twenty judged
no-better (validating the week's hand-tuning as locally optimal), one
promoted (`ESCAPE_MARGIN_CELLS` 8 → 10, habitual 92% → 98% with warm
domination and the browser board untouched), and one runner-up that WON
alone but REGRESSED stacked on the new champion — epistasis caught only
because promotion is one knob at a time.

A static candidate grid cannot deliver "continual": after one promotion it
re-tests a stale neighbourhood forever. Continual improvement needs the
sweep to move with the champion.

## Decisions

**Champion-relative candidates.** `scripts/darwin.py` no longer carries a
value table. Each run asks the live binary for the current tuning
(`examples/print_tuning.rs`) and generates one candidate below and one
above each knob's CURRENT value. Promote a knob today and tomorrow's sweep
explores around the new value — a hill-climb, not a checklist.

**Date-seeded exploration.** Step sizes are drawn from an RNG seeded with
the calendar date (×0.70–0.92 down, ×1.08–1.40 up): reproducible within a
day — receipts stay auditable — while successive nights probe different
offsets, so the climb doesn't rut in one step size.

**Nightly schedule, trustworthy receipts.** A crontab entry (03:41, plus
the Monday metaharness audit from ADR-013's loop) runs
`scripts/darwin-cron.sh`, which refuses a dirty working tree — an
unattended sweep must measure a committed champion or its receipt cannot
name what it measured — and stamps every run with the champion's commit.

**Winners cannot die in a log.** When a sweep finds a champion-beater, the
wrapper appends `.darwin/WINNERS.md` AND stores a pattern in project
memory, so the next working session's recall surfaces it without anyone
reading cron output.

**Promotion stays human — this is the load-bearing rule.** The nightly job
never edits a default. A winner becomes the champion only when a person
(or an attended session) re-verifies it through the full gauntlet —
including the browser-board probe and a stacking check against other
recent winners, because the first sweep already demonstrated that two
individually-winning knobs can regress in combination. Fitness on the
domination suite is necessary, never sufficient.

## The compounding loop this completes

Nightly: mutate around the current champion → sandboxed fitness on fixed
seeds → ADR-009 hard gate → winners surfaced to memory. By day: a session
verifies, promotes, commits with receipts — and that promotion becomes the
centre of the next night's search. The wheel turns unattended; the
judgment stays human; every step leaves a receipt.
