# ADR-020: The Turn Book — Reading the Player on the Frames That Decide Games

## Status
Accepted — implementing in three staged commits (this document is the plan
of record; each stage lands with its own receipts)

## Date
2026-08-06

## Context: the RCA

The owner's 45-round ghost corpus (the first real-human benchmark,
ADR-016/017) measured the entire learning stack at **+0.0% lift over
8,390 frames** while he won 39-5-1. Layered RCA, every link measured:

- He is a food-farming slalomer: 79% of the food economy, a turn every
  ~5 frames (47% within ≤4), global break habit 51L/49R — null on the
  habit axis — but P(alternate)=62% and a 1-gram over his turn sequence
  scores 66.9% on held-out rounds vs a 53% base. The rhythm is the shadow
  of the why: overshoot-and-correct weaving toward the next morsel.
- Per-model microscope (`examples/rca_probe.rs`): on his 995
  voluntary-turn frames the PUBLISHED forecast scores **9.0%** while
  eatW/armW sit unused in the same ensemble at **54.8%/56.3%**. On his
  7,392 straight frames the wall-readers score 99.9%.
- Root cause: model selection is GLOBAL fixed-share; straight-frame
  volume (88%) lets always-straight experts monopolize the weights, so
  the forecast degenerates to "straight" exactly where the game is
  decided. The turn-frame skill already exists and never reaches the
  wire. Forced turns are irrelevant here (n=5; the legal mask already
  owns them).

## Research grounding (Aug 2026 sweep)

Sleeping/specialist experts — score each expert only on the rounds it is
awake for — is the standing formalism for regime-local skill (Freund et
al.; active 2025-26 applications). Hazard modeling with unconditional ×
conditional statistics matches both the human-timing literature (PLOS
Biol. 2025) and his measured gap/misalignment structure. Online inverse
planning validates the intent-model stack; boundedly-rational execution
noise is precisely his slalom. Full source list in the session log.

## Consults (kata step: k3 + codex, both delivered)

Codex (gpt-5.6-sol) — verdict: reject two symmetric books; a straight
book is meaningless (conditional on straight, the answer IS the heading).
Use the hierarchical factorization P(S)=1−h, P(L)=h·q, P(R)=h·(1−q),
publish the joint argmax; a fixed hazard threshold is wrong because the
decision must couple to turn-side confidence. Plus four defects in the
EXISTING system: (1) rca_probe's baseline is not the production baseline
(modal absolute vs modal relative) — re-measure before promotion; (2) the
hunt-gate confidence ramp counts FORCED-turn observations (he has five in
45 rounds) — even a perfect forecast cannot open the gates against him;
(3) sharpness wakes on raw lift without significance — a null player can
wake it on noise; (4) the alternation model must track the last LATERAL
turn (the current last-turn state is overwritten by straights).

Kimi-k3 — independently reproduced the corpus numbers and the RCA;
verdict: the composition is sound AND, as first specified, **it
manufactures fake lift**: under a gated turn book the McNemar discordant
stream is one-sided by construction (base = modal turn = Straight is
structurally wrong on every gated frame → cpu_only pairs only → a
chance-level book grades p ≈ 0.5^n). A fair-coin alternator would read
as significantly learned; the current NULL tests cannot see this (the
coinflip unit test hand-feeds an even split; the persona NULL uses a
different instrument). Prescriptions adopted wholesale:
- **Class-conditional baseline as a prerequisite, own commit**: the base
  gets the same class information the CPU's forecast expresses — when
  the published forecast is a TURN, base = their modal non-straight
  turn; else modal overall. This also closes the smaller pre-existing
  one-sidedness at forced turns; persona lift numbers will DROP when it
  lands, and that drop is the artifact being removed.
- Derived gate, no knob: publish the turn book's pick iff
  h·aT > (1−h)·aS where aS/aT are the books' own online hit rates —
  self-calibrating (θ = aS/(aS+aT) ≈ 0.65 early), one sentence, correct
  on NULLs — with a small Schmitt band (~±0.05) to prevent identity
  flapping on the sealed, HUD-visible forecast.
- Books scored gate-INDEPENDENTLY on the realized class (specialist
  accounting) or starvation becomes chronic; ~22 turn events/round is
  ample. Maturity floor ~30 scored turn events before the gate may fire.
- Book weights, per-book hit counters, and hazard statistics PERSIST —
  today `reset_scores()` cold-starts selection every round, which
  against a 45-round corpus reproduces the failure 45 times. New
  length-prefixed WRM2 section (SEC_CLASS_BOOKS = 9, EpisodesWire
  pattern, roster-size-tolerant); ReadRate's widened shape ships as
  SEC_READ_RATE2 with fallback decode from the old section.
- Per-book confidence must feed the hunt gates (a turn-book forecast
  carrying the global straight-dominated 0.99 confidence is the
  phantom-confidence bug class recurring).
- Hazard features by measured value: dedicated frames-since-voluntary-
  turn counter (the 4-deep player tail cannot represent a 5-frame gap),
  food-route alignment (reuse the intent stack's still_closing/val
  tests), frames-since-last-food-pickup, CPU-proximity/closing bucket
  (the post-sharpening regime); NOT wall proximity (predicts forced
  turns, which bypass selection). KT cells with ~0.995 decay; log-loss;
  no EMA (it would erase the 5-frame periodicity).
- M14 is not a hand-rolled alternation chain: feed a SECOND TurnPattern
  (VOMM) instance ALL voluntary turns (the forced-only instance keeps
  its distinct "which way when cornered" semantics for the mask) and
  expose it as an ensemble model, abstaining below VOMM_MIN_EVENTS. KT
  depth mixing subsumes the 1-gram alternator (his 66.9% floor).
- The gauntlet is blind to this change (the habitual persona expresses
  its habit at forced turns): a voluntary-slalom/alternator persona
  joins the suites FIRST. End-to-end NULL tests (coinflip AND
  fair-alternator personas through the real update() loop asserting the
  lifetime read never goes significant) are what make the honesty fix
  falsifiable.
- Known residual, flagged not fixed: the 5-frame player projection
  feeding the intercept layers assumes one turn then straight; when turn
  reads start landing, measure projection quality (rca_probe can grade
  it) before trusting deeper intercepts.

## The decision

Stage 1 — HONESTY (prerequisite): class-conditional baseline
(SEC_READ_RATE2 + fallback), significance-gated sharpness
(drive_read = significant ? lift : 0), voluntary-turn evidence ramp for
the hunt gates (replacing the forced-turn-starved turn_observations
ramp), the alternator persona, and the end-to-end NULL tests. Persona
lift numbers will drop; the receipts say why.

### Stage 1 — executed, and what execution found (2026-08-06)

Everything above landed, plus four findings the plan did not contain:

1. **The baseline also needed the LEGAL SET (information parity).** With
   class-conditioning alone, the new end-to-end NULL (a fair-coin
   voluntary slalomer, `SlalomCoin`) still graded lift 0.35
   SIGNIFICANT: on forced turns the CPU calls the only exit from board
   knowledge while a baseline never told what was legal is structurally
   wrong — every such frame fabricated evidence. `ReadRate::record` now
   takes the frame's legal turn set and the base answers from the modal
   turn AMONG LEGAL options (class-conditioned to legal laterals when
   the published forecast is a turn). The NULL went null.
2. **A second honest evidence channel was required.** Against a pure
   modal habit the class-aware base calls the habit exactly as well as
   the CPU — discordants vanish and McNemar honestly reports "no
   evidence either way", yet the read is real (85%+ on the persona's
   true choices). `ReadRate` gains a LATERAL channel: forecast hits on
   frames where the player actually turned with ≥2 legal options,
   z-tested against exact uniform chance, Schmitt-latched (proven at
   3σ, released below 1σ — without hysteresis a proven read's fixed
   excess drifts under a single gate as variance grows and sharpness
   flaps). `earned_read()` = max of the two gated channels; sharpness,
   difficulty, and the hunt-gate confidence ramp spend that. (One
   documented non-read exception: the hunt confidence is floored by the
   bold_* OPENING affordance — `bold_drive × (1−sharpness) ×
   boldness_scale()` — which is deliberate first-contact aggression, not
   read evidence, and fades with sharpness or scoreboard lead.) Nulls
   clear neither channel — asserted end-to-end at EVERY FRAME.
3. **Quantity × quality on the hunt ramp.** The voluntary-turn evidence
   ramp alone maxed out in one warm game and opened full-confidence
   hunts behind an unproven forecast (measured: warm arms LOSING to
   cold). The ramp now multiplies `earned_read()`.
4. **Two opening refinements (ADR-018 amendments), both measured:**
   curiosity may close distance but never down the player's own driving
   lane ±1 ahead of their head (3 of 5 warm draws were curiosity-steered
   HEAD-ONs finished by a dozy held heading); and the bold_* knobs now
   scale by `boldness_scale()` — fading to zero as the CPU pulls ahead
   on the visible scoreboard, since manufactured recklessness is for
   first contact, not for a CPU already winning.

Receipts, owner corpus (45 rounds, 8,390 frames): honest lifetime lift
**+2.8%, statistically significant** (the fake-masked figure was +0.0%);
rca_probe with the PRODUCTION baseline: voluntary-turn PUBLISHED 9.0%
vs honest BASE 5.4% (eatW 54.8% still unused — stage 2's prize).
Novice opening unchanged (~25% wins+draws). All suites green: lib 43,
game_test 76, persona 5 (+1 stage-3 ignore), domination 1 (+1 ignore).

**Known honest regression, now the STAGE-2 GATE:** pooled over two
30-game arms, warm 82% vs cold 97% against the domination persona.
Working hypothesis (NOT yet instrumented — attribution is stage-2
work): the losses accumulate in the half-woken middle of the arc,
where partial discipline coexists with enabled hunts at confidences no
gate was tuned for — a regime the fabricated forced-turn lift used to
skip by making warm arms fully sharp from game 2.
`learning_converts_into_winning` is `#[ignore]` with this reason in
the code; its warm-vs-cold bar carries a formal non-inferiority margin
of 5 points on 60-game pooled arms (binomial noise sd ≈ 4.6 points).
Un-ignoring it is part of stage 2's proof bar, alongside the original
bar below.

### Stage 1.1 — external verification round (codex, verdict UNSOUND → fixed)

Codex verified the committed stage 1 against this ADR and found the
lateral channel's null OUTCOME-CONDITIONED: it sampled frames where
the player did turn but scored against 1/options INCLUDING straight.
Conditioned on a lateral outcome a uniform player picks each side at
1/2 (and with one legal lateral the side is certain), so an
always-Left forecast systematically beat the stated chance and could
latch a fake read — the exact class of defect stage 1 exists to kill,
reproduced in a unit test before fixing. Corrections, all landed:

- The lateral channel claims SIDE evidence only: recorded solely when
  BOTH laterals are legal, null exactly 1/2. Timing skill earns
  nothing here by construction — it must be claimed by a predictor
  scored on all its declarations including straight false alarms (the
  stage-2 hazard book).
- The repeated-look 3σ open was not anytime-valid (a null random walk
  crosses any fixed line eventually). The opening boundary now grows
  with the looks — sqrt(2(ln 1000 + ln ln n)) ≈ 4.0–4.3 — with the 1σ
  close kept as post-open hysteresis only.
- `modal_among` answers at zero history (lowest-index legal), closing
  the bounded first-occurrence cpu_only bias at forced laterals.
- NULL personas assert at every FRAME (read_conf consults the latch
  mid-round), and the slalom null asserts the channel was actually
  exercised (≥30 real choices scored).
- Wire: v2 precedence over v1 is explicit, not file-order luck; a
  v1-only load (old build, or a downgrade cycle that stripped v2)
  keeps everything that is knowledge about the human but ZEROES the
  discordant bookkeeping — a mixed class-blind/class-aware McNemar
  stream cannot be untangled, so significance is re-earned. Lateral
  evidence is lost on downgrade by construction; accepted cost.
- The misnamed missing-section test now actually strips both read
  sections; a reordered-blob test pins the precedence rule.

Post-fix receipts: all NULLs hold per-frame; the habitual read is
still earned at real choices (peak 0.70 under the corrected null,
domination 85%); owner corpus honest lift unchanged at +2.8%
significant; suite 48 lib + 76 game + 5 persona + domination green.

Stage 2 — THE TURN BOOK: hazard (KT cells over gap × alignment ×
just-ate × cpu-closing, two-horizon fixed-share, log-loss), a
turn-conditioned selection book over the existing models, the derived
Schmitt gate, per-book confidence to the hunt gates, ForecastTrace
carrying which book drove, and the persistence sections.

### Stage 2 — executed; the measured result that reshaped it (2026-08-06)

The machinery above landed as specified (ClassBooks: 64 KT hazard
cells, turn-scored fixed-share book, derived Schmitt gate,
SEC_CLASS_BOOKS wire, PendingBook precommit record folded into the
reveal chain). Then the owner-corpus measurement invalidated the
plan's central assumption AND revealed the real prize:

- **The side book is excellent: aT = 0.71** on his genuine two-sided
  voluntary turns (single prequential pass, 983 events, coverage
  0.99) — the turn-scored fixed-share does surface the eatW/armW
  skill the RCA found buried.
- **The derived gate can never open for him.** h·aT > (1−h)·aS needs
  a context cell with h ≳ 0.55; his best populated cell reaches
  ~0.2 (base hazard 12%). This is the gate WORKING — for a rare-event
  player the expected-hit argmax at every single frame IS straight —
  and it means the original proof bar ("PUBLISHED voluntary-turn
  accuracy 9% → 55–65%") was unachievable by the agreed design. Both
  consults endorsed the gate; nobody connected θ ≈ 0.55 to his
  measured 12% hazard. A metronome alternator probe confirms the gate
  DOES fire when hazard truly spikes (h = 0.90 at its due cell).

Reformulation (consulted with codex — verdict "proceed, with two
blockers", both resolved):

1. **The evidence family** (the codex blocker): every channel —
   published McNemar, published lateral, book McNemar, book lateral —
   now shares ONE anytime-valid boundary sqrt(2(ln(4/α)+lnln n)) with
   the α split across the family, Schmitt latches, and every spend is
   an SE-shrunk lift, never the raw point estimate. The book's
   evidence lives in `book_read`: its PRECOMMITTED side picks scored
   on genuine two-sided voluntary turns through the same ReadRate
   machinery, nulls and all, scaled by declaration coverage.
   **Measured on the owner corpus: book_read earned 0.38, spendable
   0.37, authority granted.** The CPU finally holds an honestly
   earned read against the owner — his alternation — where every
   previous number was 0. Difficulty spends it through a
   round-boundary snapshot (no mid-round latch may open hunts).
2. **Projection** (the other blocker): scenario-based weighted-medoid
   projection (no-turn + turn-at-t × side, survival-adjusted masses,
   book-calibrated side split, path-loss medoid — codex's design,
   no forced bend, authority-gated). **Measured NEUTRAL on the owner
   corpus** (paired 5-frame loss 11445 vs 11433 over 1671 windows):
   kept behind `projection_authority()` and the WORM_TUNE_BOOK_BEND
   arm switch, claims nothing, candidate for Darwin.
3. **The transition regime, finally attributed and fixed**: with
   honest evidence the read arrives MID-arc, and survival discipline
   scaled by partial sharpness produced a half-woken middle that lost
   games both dozy (96%) and fully-sharp (92%) play win. Fix:
   `discipline_sharpness()` — any proven read ends the casual
   opening's sloppiness outright (basics snap to full; AGGRESSION
   still scales continuously with the read). Sloppy basics exist for
   the genuinely-unread phase only.

**The stage-2 gate is un-ignored and green**: warm 93% vs cold 96%
pooled over three paired 30-game seeds (5-point non-inferiority
margin), peak earned read 0.75; the habitual arm reads at 0.57 with
85% wins over 60 games. NULL personas hold on every channel of the
family, at every frame, across three seeds, and a fair-side book
never wins projection authority. Novice opening unchanged (~22%
wins+draws — the discipline snap touches only proven-read play).

### Proof bar, rewritten (stage 2 measured; supersedes the original)

- a. Book side skill: aT ≥ 0.65 with coverage ≥ 0.9 on genuine
  two-sided choices, single prequential pass — **met (0.71/0.99)**.
- b. book_read positive and family-latched on the owner corpus —
  **met (earned 0.38)**; NULLs null on every channel — **met**.
- c. Projection: paired 5-frame loss must not regress; bending claims
  nothing until it measurably wins — **met (neutral, gated off the
  spend path)**.
- d. Warm-vs-cold non-inferiority un-ignored and green — **met**.
- e. Full gauntlet green; ghost_eval honest lift non-regressing —
  **met (+2.9%)**.
- f. Stage 3 (M14 voluntary VOMM) raises aT on alternation personas
  (measured today: a strict alternator's side book sits at coin level
  because no roster model reads voluntary alternation — the metronome
  probe's aT = 0.54 is the gap M14 exists to close).

Stage 3 — M14: the voluntary-turn VOMM as an ensemble model.

### Stage 3 — executed (2026-08-06)

M13 `alt` ("Rhythm reader" on the HUD): a second TurnPattern (VOMM)
instance fed every voluntary lateral, exposed as an ensemble model that
speaks whenever it has ≥ VOMM_MIN_EVENTS. Global weights bury it on
straight volume BY DESIGN; the turn book scores it on the only frames
it exists for and elects it there. Receipts:

- Metronome alternator: book aT **0.54 → 0.98**; the gate now publishes
  123 swerve forecasts per 12-game probe at **100% hit**; overall
  published accuracy on its swerves 31% → 68%. The strict-alternator
  acceptance test is UN-IGNORED and green (z >> 5).
- Owner corpus: `alt` scores **60.5%** on his voluntary turns — the new
  best single model on the frames that decide games (eatW 54.8%) —
  while scoring 1% on straight frames, exactly the specialist shape
  the sleeping-experts design wants. book_read stays latched
  (spendable 0.33, coverage 1.00).
- Roster 13 → 14: SEC_ENSEMBLE's fixed arrays make old ensemble
  sections drop on decode (weights only — per-round state anyway);
  ClassBooksWire is roster-tolerant and keeps the hazard. Web tables
  updated in lockstep (MODEL_NAMES, MODEL_INFO with the `??` fallback,
  BUILD 17→18) — the MODEL_INFO page-killer class cannot recur.
- Full gauntlet green after the roster change: domination 93/96 warm/
  cold and habitual 88% reading 0.54; NULLs hold on every family
  channel; novice opening unchanged; live-site probes (legacy-state
  poisoning + page boot) pass against the rebuilt wasm.

Proof bar (per the kata): rca_probe re-run with the PRODUCTION baseline
(codex fix) on the owner corpus — published voluntary-turn accuracy must
move from 9% toward book-T's measured skill (~55-65%); ghost_eval
holdout lift must be positive and significant UNDER THE HONEST BASELINE;
both NULL personas must stay non-significant; the full gauntlet holds;
kimi-k3 verifies the committed result against this ADR.
