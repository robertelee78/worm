# Fleet Warm-Start Policy (ADR-021 surface #8 — design, build deferred)

Status: policy complete, implementation DEFERRED until the fleet holds
N ≥ 2 real returning humans (today: one owner + drive-by single-round
visitors). Both consults: building archetypes over one player is a
copy, not a generalization.

## The merge rule (the precondition k3 named)

Another human's statistics NEVER merge into this human's counters.
The fleet prior enters exactly once, as **pseudo-counts with a hard
cap of ~3 rounds of equivalent mass**, into a SEPARATE prior field per
surface (hazard cells, turn priors, VOMM counts). Local evidence and
imported mass are stored apart and reported apart; one evening of this
player's own play must outweigh the fleet.

## Honesty invariants

- Imported mass counts toward NO evidence channel: the family scores
  only this player's frames, so a prior-shaped CPU still earns its
  read honestly, from zero.
- `SEC_FLEET_PRIOR_RECEIPT` (wire, reserved): archetype id, pseudo-mass
  per surface, artifact hash, import timestamp. `BrainRestore` then
  distinguishes "your opponent still remembers you" from "it was
  briefed on players like you" — the product's first sentence stays
  true.
- Disclosure: the page footer's collection notice gains one line when
  this ships ("new opponents start with habits learned from previous
  players").
- Replay determinism: the prior is snapshotted INTO the brain file at
  import; ghost replay never calls the server.
- Deletion: a player's rounds deleted from the collector must trigger
  archetype rebuild; archetype artifacts carry the source-round hash
  list for exactly this purpose.
- Poisoning: archetype construction excludes rounds failing the
  evaluator's completeness checks, and caps any single device's
  contribution to an archetype at 20%.

## Infrastructure (committed choice, ruvector-native)

Server-side only, beside the collector: RVF single-file store, HNSW
index over player-signature embeddings (habit stats: turn-gap
histogram, alternation rate, food economy, hazard-cell profile),
local ONNX embeddings if free-text ever enters (it does not today).
Nothing of this enters the wasm client except the receipt section.
