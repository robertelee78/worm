# ADR-005: Durable Player Identity and Brain Integrity

## Status
Implemented

## Date
2026-08-05

## Context

The product premise is that the CPU learns a specific human across many
matches. That makes the brain the player's accumulated investment, and losing
it is the worst failure the product has. An audit found three ways to lose it
and one way to silently neutralise it.

### 1. Identity and brain had different eviction fates (web)

`deviceId` lived in `localStorage`; the brain it keys lives in IndexedDB.
Safari's ITP clears `localStorage` after seven days without interaction, while
IndexedDB survives. A player returning after a break was therefore minted a
fresh id, and the brain they had spent twenty matches teaching sat unreachable
under the old key — still consuming quota, permanently orphaned. No corruption
required, just time.

### 2. The native brain path was relative

`brain_path()` defaulted to `worm_brain.bin` in the *working directory*, so
launching the terminal build from anywhere else silently began a new brain.

### 3. A structurally valid brain could be semantically poisonous

The WRM2 decoder is bounds-checked and degrades gracefully on truncation or
corrupt sections. But a *valid* blob can still carry `NaN`: it round-trips
intact, `prior_distribution()` then returns `[NaN; 4]`, every comparison goes
false, and argmax degrades to a constant. The CPU becomes **silently** useless
— no panic, nothing reported. That is worse than a crash, because nothing
surfaces it. `tail_len` deserialised as `usize::MAX` similarly makes the tail
trim loop never fire; `ensemble.active` deserialised out of range; and
`opp_pred_hits > opp_pred_total` reports accuracy above 100%.

### 4. Firing worked while paused (native)

Only `update()` is gated on `paused`; the input loop was not. A paused player
could fire a laser in frozen time — and a laser kill ends the game outright.

## Decision

**Identity moves into IndexedDB**, in a `meta` store beside the data it keys
(DB version 2 → 3), so the two share a fate. A one-time adoption reads any
existing `localStorage` id first — without it, this release would orphan every
existing player in the very change that fixes orphaning. `localStorage` is
still mirrored, for diagnostics and for that adoption path, but is no longer
the source of truth. `navigator.storage.persist()` is requested on boot.

**The native brain moves to `$XDG_DATA_HOME/worm/worm_brain.bin`** (or
`~/.local/share/worm`), with `brain_load_path()` falling back to the legacy
relative file so an upgrading player keeps what they earned; the next save
rewrites it to the stable location. `WORM_BRAIN` still overrides.

**`CpuBrain::sanitize`** runs on every load, after decoding and before the
brain is returned. Same philosophy as the section decoder: drop the unusable
part, keep everything else, report what was dropped. Non-finite episodes are
dropped and counted into `BrainRestore`; a poisoned tally resets to zeros
(yielding a uniform, inert prior rather than a wrong one); `tail_len` clamps to
`1..=16`; `opp_pred_hits` clamps to `opp_pred_total`; `ensemble.active` clamps
into range.

**Firing is gated on `!paused`.** Direction input stays live, because it only
latches a pending turn the next frame would apply anyway.

## Consequences

Existing players are carried across by two adoption paths that must not be
removed until well after this ships. Both are cheap and both are load-bearing:
deleting either turns a fix into the data loss it was meant to prevent.

A claim from the persistence audit did **not** survive checking: it reported
that the browser saves only at game over, so a round's learning is lost if the
tab closes. The code already flushed on a 10-second interval and on
`pagehide`. A `visibilitychange` handler was added anyway, because mobile
Safari does not reliably deliver `pagehide` when a tab is backgrounded or
discarded.

## Verification

`cargo test` — 87 tests pass. `a_poisoned_brain_is_sanitized_on_load` runs a
brain carrying NaN and Inf episode vectors, a NaN tally, `tail_len =
usize::MAX`, `opp_pred_hits` far exceeding total, and an out-of-range ensemble
index through a full save/load cycle, and asserts the result is finite,
bounded, and still usable. `node --check` on `web/app.js`; wasm rebuilt.
