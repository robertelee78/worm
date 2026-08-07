# ADR-021: The Learning Program — Nine Surfaces Where the CPU Learns More

## Status
Implemented (v1) — surfaces 1–7 shipped in their consult-shaped first
forms with receipts below; 8 is design-complete and build-deferred
(docs/fleet-warm-start-policy.md, N≥2 humans precondition); 9 is an
unscheduled offline challenger behind three hard gates

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

## Consult synthesis (codex + kimi-k3, both delivered 2026-08-06)

Convergent on every major call. The program's governing rules, adopted:

**The Estimate / Evidence / Authority triad (codex), with k3's
one-sentence discipline.** Estimates (KT, decay, Exp3, VOMM, priors)
update freely on every eligible event. Evidence is a non-decayed,
precommitted comparison against a NAMED incumbent/null, tested under an
anytime-valid budget. Authority — the right to alter aggression — is
only ever a round-boundary snapshot of gated evidence. The forbidden
move across all nine surfaces: a self-knowledge channel that raises
hunt pressure without passing through `earned_snapshot`.

**The class table (k3).** Player-read evidence (forecasts, hazard,
timing, drift stats) rides the family discipline. Self-knowledge
(tactic outcomes, weapon outcomes, own death causes, coverage of own
map) is exempt PROVIDED it can never raise aggression — it may re-rank
already-gated options, raise defensive floors, or steer curiosity.
Board knowledge is exempt by construction.

**Evidence-budget registry (codex).** The four-channel α split is
currently a hardcoded assumption in `look_threshold`; before any fifth
channel, the budget becomes an explicit named registry (families,
channels, α allocations, sum stated). The drift alarm gets its OWN
family (k3): round-count geometric looks, α = 0.005, separate from the
per-frame family.

**Wire (both).** Independent top-level sections by failure domain:
SEC_ACTION_OUTCOMES (tactics + weapons, semantic action IDs),
SEC_LOSS_DEFENSE, SEC_TURN_TIMING (rhythm + opening + motifs),
SEC_DRIFT_EPOCHS, later SEC_FLEET_PRIOR_RECEIPT. Count-keyed bodies,
own schema versions, no roster-sized fixed arrays. No umbrella (it
recreates section framing inside one failure domain).

**Rejections, agreed by both:** no min-cut dependency for surface 3 v1
(96 static cells need direct mass/uncertainty, not connectivity — a
disconnected context graph makes the cut zero/arbitrary; the live
ruvector-mincut API is real and fits semantically, verdict recorded,
revisit only for genuine transition-graph questions); no motif VOMM
parallel to M13 (alphabet-extension probe instead); no dodge-skill
evidence channel (outcome-conditioned sampling — descriptive counts
only); no "lethal within 40 frames" reward without precommitted
attempt IDs and competing-risk attribution; no fleet-stat merging into
this-player counters, ever; no runtime SONA; no per-surface confidence
numbers on the HUD (one earned number, named sources, a sentence per
surface).

## The build order (adopted)

0. **Kata 0 — honesty + instrumentation**: reconcile the ADR-020
   α/look prose drift (done in this commit); evidence-budget registry;
   attempt/outcome/attribution/round-summary ledgers with ZERO behavior
   change (receipt: gameplay-identical replays); era-2 baseline
   recorded (book aT 0.70, spendable 0.34 at era-2 end — the read
   survived the measured drift); k3's downward-crossing drifting
   persona joins the gauntlet.

   EXECUTED. Receipts: full suite green (53+79+7+1+2); the downward
   crossing passes (peak earned >0.2 during alternation, honestly
   released to 0 after twelve coin games); ghost_eval bisected to
   verify metric-neutrality — Kata 0 changes nothing (+2.5% before and
   after). The bisect also attributed an unreported −0.2pp from stage
   2.2 (the 96-cell hazard feeds the publish gate, so gate-fire frames
   changed): **the program's honest baseline is +2.5% over 13,120
   frames on the 63-round corpus**, superseding the +2.7 quoted in the
   arena-era receipts.
1. **#5 loss-attribution defense** (Darwin ESCAPE_* static sweep
   first; chase-flag attribution; floors only rise; coin NULL).
2. **#4+#7 TurnTimingBook** (merged): discrete survival over gap with
   16 buckets + tail + right-censoring, opening phase as context,
   motif features; prequential 8-vs-16-bucket log-loss gate before the
   resolution is kept; feeds the existing published evidence channel.
3. **#6 drift alarm**: two-window comparison on round summaries, own
   anytime family; resets touch fast horizons ONLY (ReadRate, book,
   latches, maturity are sacred); notebook + HUD sentence.
4. **#1 tactic bandit**: on the grown ledger; per-tactic maturity
   floors; perturbation among ALREADY-GATED applicable tactics
   (structurally cannot raise aggression); the incumbent rule order is
   the null.
5. **#2 bait book**: bounded per-weapon exploration floors first
   (mine, then laser; trishot floor zero), novice-invariant measured;
   KT consults only marginally-failing gates.
6. **#3 epistemic self-map**: count-based (mass + never-seen), decayed
   so thinness reflects currency; curiosity consumer merges into the
   existing Curiosity layer as target-selection (inherits the
   driving-lane ban verbatim); notebook names the unknown regime; HUD
   keeps ONE earned number and gains the coverage sentence.
7. **#8 fleet warm-start**: DESIGN ONLY until N≥2 real humans:
   hierarchical pseudo-count prior with hard cap (~3 rounds of mass),
   imported/local mass separated, SEC_FLEET_PRIOR_RECEIPT honesty flag
   ("briefed on players like you" ≠ "remembers you"), disclosure in
   the footer, replay never calls the server.
8. **#9 SONA**: unscheduled offline challenger; bars = beats the
   counting stack prequentially, compiles to one HUD sentence,
   deterministic wasm inference.

## Kata record (all executed 2026-08-06/07)

- **Kata 0** — registry + ledgers + downward-crossing persona; ghost
  bisect proved metric-neutrality and re-baselined the program at
  +2.5%/13,120 (stage 2.2's gate coupling, previously unreported).
- **Kata 1 (#5)** — boxer aversion: chased enemy-trail deaths raise the
  escape floor +6%/kill, cap +50%, floors only rise, chase-gated;
  Darwin ESCAPE_* static sweep ran alongside (no static winner —
  validating the learner as the right tool).
- **Kata 2 (#4+#7)** — the consult gate REJECTED 16 gap buckets
  (prequential log-loss 0.3642 vs 0.3619 — sparsity beats shape on
  this corpus; 8 stays). The durable half shipped: the voluntary VOMM
  persists (SEC_TURN_TIMING), so the rhythm/opening read survives
  sessions — the honest core of the spawn book, with codex's
  hindsight-scored signature explicitly not built.
- **Kata 3 (#6)** — drift alarm: two-window alternation z at
  round-count looks, family B, Schmitt-latched, narration-only
  (nothing resets, nothing spends). End-to-end: alternator→coin trips
  it; 34 stationary rounds never do.
- **Kata 4 (#1)** — tactic bandit v1: precommitted-window ledger
  matured at ≥10 attempts/tactic; its one consumer is a YIELD (corner
  intercept steps aside when the ledger says direct kills this player
  better) — strictly non-aggression-raising, snapshot at round
  boundary, incumbent order as the null, gauntlet unchanged.
- **Kata 5 (#2)** — bait supply-generator: one exploratory mine
  placement per round after 40 held frames, never in a player's first
  three rounds, room-to-leave preserved; novice probe BYTE-IDENTICAL
  (5/30/5). Learned per-weapon exploitation stays off until the
  opportunity ledger matures (~10 fires/weapon).
- **Kata 6 (#3)** — epistemic self-map, count-based: (populated, thin,
  unseen) over decayed hazard mass; notebook + web state carry it as
  coverage QUANTITY, kept apart from earned significance. The
  ruvector-mincut dependency was evaluated against its live API and
  declined for v1 (both consults: 96 static cells need mass, not
  connectivity); active-curiosity steering deferred — the only
  steerable context dimension (proximity) is already what curiosity
  maximizes.
- **Kata 7 (#8)** — docs/fleet-warm-start-policy.md: the merge rule
  (pseudo-counts, ~3-round cap, separate fields), honesty invariants
  (SEC_FLEET_PRIOR_RECEIPT, disclosure, deletion, poisoning caps),
  ruvector-native server-side infra. Build waits for a second
  returning human.
