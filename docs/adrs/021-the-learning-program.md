# ADR-021: The Learning Program — Nine Surfaces Where the CPU Learns More

## Status
Proposed — design in consult (codex + kimi-k3, in flight); this document
carries the spike data and constraints now, and gains its Decision
sections as each surface's kata completes

## Date
2026-08-06

## Context

ADR-020 gave the CPU an honestly earned read of the player's turns and
the machinery to spend it. The owner's question "where is additional
learning possible?" produced a nine-surface survey, and a data-supply
spike over his real 63-round corpus (13,120 frames;
`examples/learning_probe.rs`) turned the survey into measurements:

| # | Surface | Measured supply (owner corpus) |
|---|---------|-------------------------------|
| 1 | Tactic bandit — which of the CPU's hunt tactics kill THIS player | 17 player deaths as outcomes (5 enemy-trail, 5 own-trail, 5 wall, 2 mine-bait); nonlethal attempts uninstrumented |
| 2 | Bait book — per-weapon effectiveness on this player | THIN: 9 CPU fires in 63 rounds (6 trishot/2 lethal, 3 mine/1 lethal, 0 laser) while the player killed the CPU 4× with weapons — an exploration-policy gap before a learning gap |
| 3 | Epistemic self-map — where the read does NOT exist | 85/96 hazard cells populated, 22 thin (<5 mass), 11 empty, 4,586 total mass — the unknown is countable and nameable |
| 4 | Rhythm distribution + motifs | Lateral-gap histogram strongly non-geometric: burst mode (232 immediate re-turns), 2–5 slalom plateau, long-straight tail (175 at 15+) |
| 5 | Loss-attribution defense | 45 CPU deaths; DOMINANT mode 27× "hit enemy trail" — the player's box-ins |
| 6 | Drift alarm | MEASURED REAL: era 1–45 vs 46–63, mean gap 7.17→5.82, P(alternate) 0.61→0.52 — play changed after the honest build started reading alternation |
| 7 | Spawn book | Top opening signature 15/63 (24%); top-4 cover ~57% |
| 8 | Fleet warm-start | 48+ distinct collected visitors; precondition: a written merge policy for decayed single-human statistics |
| 9 | SONA trajectory consolidation | Entry gated on the counting stack measurably plateauing |

**Drift does not (yet) degrade the read**: book side accuracy held
0.71→0.70 and spendable 0.33→0.34 across the era boundary — the
eatW-family reads the WHY (food geometry), which did not change; only
the sequence habit (alternation) moved, and the decayed statistics
absorbed it. The drift is visible in behavior, not in read health. The
drift alarm's job is to SAY so, not to save a failing read.

## Standing constraints (inherited, non-negotiable)

- Anything feeding AGGRESSION passes the family-wise anytime evidence
  discipline (ADR-020: geometric looks, exact Hoeffding bounds,
  SE-shrunk spends, round-boundary snapshots).
- Every read explainable to the player in a sentence; the notebook and
  HUD name their evidence sources.
- Bit-exact ghost replay survives every change; recorded rounds pin
  their world (arena versioning pattern).
- Sectioned WRM2 wire; a schema change never wipes knowledge about the
  human; sections are individually tolerant.
- The beatable opening gates sharp behavior; novice experience is a
  measured invariant.
- ADR-014: promoted by receipt, reverted by receipt.

## ruvector commitments (grounded via the RuvNet Brain)

- Surface 3 uses `ruvector-mincut` if its live API matches ADR-001's
  design (dynamic min-cut + witness partitions) — verified before
  building on it; else a minimal in-repo cut with the crate as
  reference. The concept (min-cut over the knowledge graph = measuring
  the absence of data) is rUv's, either way.
- Surface 8 is ruvector-native: RVF single-file store + HNSW + local
  ONNX embeddings, server-side beside the collector.
- Surface 4/kernel-hazard successors: ruvector HNSW is the retrieval
  candidate when similarity recall replaces discrete cells.
- Surface 9 IS a ruvector-family crate (`crates/sona`).
- The per-frame counting core (1, 2, 5, 6, 7) deliberately stays
  dependency-free: counting statistics in a 35 ms replay-deterministic
  wasm loop, where a vector engine adds a dependency but no capability.

## Decisions

(Filled per surface as each kata completes; consult synthesis first.)
