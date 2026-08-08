# The Architecture of an Honest Opponent

**A domain-driven description of worm's CPU learning system, with its
academic lineage.**

*Status: living document. Grounded in the code as of world v11; the
ADRs in `docs/adrs/` are the decision record, this is the map. Line
references drift; names do not.*

---

## 0. The premise, stated as a constraint system

The project builds a snake/Tron opponent whose difficulty is **earned
by reading one specific human**, under four constraints that shape
every domain below:

1. **Information parity.** The CPU perceives nothing the player cannot
   perceive: positions, not intentions; the board the player sees. One
   standing, deliberate carve-out must be stated: the *awake* CPU's
   threat-avoidance reads live bomb positions regardless of disguise —
   a legacy of the pre-disguise threat model. The dozy CPU is
   disguise-honest (enemy mines are invisible to it by design), and no
   *learning* surface consumes the privileged bit; full parity for the
   awake threat model is an open debt, not an achievement.
2. **Statistical honesty.** Every claim of "I have read you" must
   survive a hypothesis test against a class-aware baseline under an
   anytime-valid evidence budget — exact tails where computed (small-n
   McNemar), conservative concentration bounds (Hoeffding) at the
   scheduled looks. Difficulty on screen is
   significance, never vibes. The dual obligation: an abandoned habit
   must be *unlearned* — evidence release is as principled as evidence
   acquisition.
3. **Explainability.** Every read is a sentence a player can be told
   ("your left-break after two straights is 84% — I latch on that"),
   and the post-round notebook quotes the exact numbers. No component
   may earn behavior it cannot explain.
4. **Sample realism.** A round yields 100–400 decisions; a session,
   a few thousand. The methods are chosen for *tiny-n online* regimes:
   counting estimators, exact tests, conjugate-style priors — never
   gradient training loops. (A neural challenger exists behind a
   plateau gate and has never earned evaluation; see §7.)

The rest of this document is organized as bounded contexts in the
domain-driven-design sense [Evans 2004]: each context has its own
ubiquitous language, its own invariants, and explicit translation at
the boundaries.

---

## 1. Context: PREDICTION — "what will this human do next frame?"

The prediction context owns per-frame forecasting of the player's next
direction. Its output is a single forecast with a named source; its
consumers (Evidence, Behavior) treat it as opaque.

### 1.1 The ensemble of fourteen specialists

Fourteen hand-crafted predictors run concurrently. In the code's
vocabulary (which is also the notebook's): habit tracker, rotation
guesser, pattern hunter, frequency reader, two wall-followers
(left/right hand), deep memory (k-NN), a rhythm reader, and six intent
models — {food, hunt, weapon-arming} × {holds-their-line, weaves} —
each an intent hypothesis simulated forward as a BFS route step.

Design lineage:

- **Specialists that abstain.** Intent models sleep when their
  precondition is absent (no power-up on the board → the arming
  models abstain). Abstention-aware expert aggregation follows the
  *specialists* framework of Freund, Schapire, Singer & Warmuth
  [1997]: sleeping experts are not charged for rounds they sit out.
  The engine's measured refinement: charging abstainers the awake
  population's mean loss (the textbook variant) performed *worse* on
  this domain and was reverted — sleepers are not charged at all.
- **Two-horizon weighting.** Each specialist carries a fast and a slow
  multiplicative weight (`w_fast`, `w_slow`), exponentiated on 0/1
  loss, blended at election time. This is fixed-share tracking in the
  sense of Herbster & Warmuth [1998]: the slow horizon believes what
  the player always does; the fast horizon chases what they started
  doing five choices ago, and the share step keeps every specialist
  recoverable after a style change.
- **k-NN episodic memories — plural.** Two distinct corpora, queried
  by cosine distance in the classical nearest-neighbor sense of Cover
  & Hart [1967]. The *opponent* corpus (≈32-dim context → the
  player's next move) feeds the deep-memory specialist; the separate
  *self-survival* corpus (25-dim context → CPU move, survival,
  reward) feeds the CPU's own episode voting. In the opponent vector
  only the recent-turn transition block is mass-normalized — the
  measured fix for a block that once carried ~47% of the vector's L2
  energy and made retrieval compare tail lengths. Both corpora record
  *terminal mistakes* — the survivor-bias fix: deaths are the most
  informative frames about how this human loses.
- **The rhythm reader (VOMM).** A variable-order Markov model over the
  player's voluntary lateral breaks: per-context Krichevsky–Trofimov
  estimators [Krichevsky & Trofimov 1981] up to a small depth, mixed
  across depths with fixed-share weights — a flattened cousin of
  context-tree weighting [Willems, Shtarkov & Tjalkens 1995] and
  context-tree switching. The alphabet is binary (left/right at
  voluntary breaks) because that is where a turning habit lives; the
  KT estimator's (n+½)/(N+1) form is the asymptotically minimax
  choice at the tiny counts this game supplies.

### 1.2 The turn book: when, and which way

Above the ensemble sits a class-conditional model of the player's
*swerve grammar*:

- **The hazard book**: 96 cells — swerve-gap bucket (8) × food side
  (3) × just-ate (2) × CPU-close (2) — each holding a KT estimate of
  P(voluntary lateral | cell), exponentially decayed (0.995) so the
  book tracks the player of the last few hundred choices. It answers
  *when* they turn.
- **The side book**: specialist-scored side predictions answering
  *which way*, scored only on genuine two-sided choices (both
  laterals legal — with one legal lateral the side is
  board-determined, and counting it would be evidence theft; see
  §2.3).
- **A correction prior**: a KT estimate of P(break toward food side)
  that bends the CPU's 5-frame projection of the player toward the
  side the food sits on — an overshoot-correction learned, not
  assumed.

The projection consumer (Behavior) may only be bent by the book when
the book holds *projection authority*: its evidence family latched
AND a maturity floor met. A chance-level side book must never reshape
defensive paths (a verification-round finding, now an invariant).

### 1.2b Sealing, and the class-aware baseline

Two mechanisms deserve explicit mention. **Forecast sealing**: each
frame's forecast is hashed into a running seal chain *before* the
player's input lands, and the chain is exposed — a player can verify
post-hoc that predictions preceded moves (the commitment discipline
that makes "it predicted you" checkable rather than assertable).
**The class-aware modal baseline**: the base-rate rival is not a
global mode — it predicts the player's modal move *conditioned on the
legality class of the frame* (which turns were even available), so
the CPU earns nothing for "predicting" forced moves; and the hazard
book's publish gate is *derived* (the book's hazard estimate must
beat the straight-rate baseline with its own Schmitt hysteresis)
before its timing skill may publish at all.

### 1.3 The portfolio: which temperament beats this human

Rather than estimating the opponent's full policy, the CPU keeps four
counter-styles (drive multipliers 0.5×–2.4× on how hard it spends its
read) and learns online *which style beats this human* — the implicit
opponent modeling of Bard, Johanson, Burch & Bowling [2013], which
collapses the learned dimensionality to a handful of numbers, the
right size for a round-level reward signal. Credit assignment is
**on-policy Exp3** [Auer, Cesa-Bianchi, Freund & Schapire 2002]:
win/draw/loss for the style actually played, importance-weighted, with
Exp3's exploration floor so no temperament's probability reaches zero.
Counterfactually replaying a round under a different style is invalid
past the first divergence — the human would have reacted — so the
slower unbiased signal is the honest one.

Between the two hunt intercepts, arm choice is **Thompson-style sampling**
[Thompson 1933]: Gaussian (Box–Muller) approximations around the KT
kill-rate estimates stand in for Beta posteriors — adequate at the
n≥10 maturity floor and disclosed as an approximation — drawn
deterministically from a hash of (seal seed, round count) so seeded
runs and replays stay bit-exact — principled exploration that keeps
measuring the losing tactic without a schedule.

---

## 2. Context: EVIDENCE — "is the read real?"

This context owns the only quantity allowed to drive difficulty: the
**earned read**. Its language — looks, latches, families, spends — is
the project's most load-bearing vocabulary.

### 2.1 The baseline problem

Raw accuracy cannot drive difficulty: most snake moves are "keep
going," so a model scoring 90% may have learned nothing, while 45%
against a 33% baseline is a strong read. Worse, a *static* baseline
can be gamed by boringness. The engine's null is therefore the
player's own realized base rate: an online rival that predicts their
commonest move so far. Significance of the CPU's advantage over that
rival is an **exact McNemar test** [McNemar 1947] computed only over
the frames where the two disagreed — against a very predictable
player they almost always agree, and thousands of frames honestly
carry a dozen frames of evidence.

The lateral channel has its own conditional null (a verification
finding that made the first implementation unsound): side evidence is
scored *given that a turn occurred*, against uniform-over-legal-
laterals — frames with fewer than two legal laterals carry zero side
information and are excluded outright. Scored against 1/options
instead, an always-left forecaster beats a fair coin by construction.

### 2.2 Anytime-valid looks

Evidence accumulates online and is *peeked at* online, which is the
textbook setting for p-hacking by optional stopping. The defense is a
group-sequential design: significance is checked only at **geometric
looks** (sample sizes growing by ratio 1.4 from a base), with a
per-look budget α_k = (α_family / channels) · 6/(π²k²) — a convergent
series spend in the spirit of alpha-spending functions [Lan & DeMets
1983; Pocock 1977]. Each look's threshold is a **Hoeffding
inequality** on fair-coin sums [Hoeffding 1963] — a conservative
concentration bound, not an exact tail; the McNemar tail *is*
computed exactly for reporting at small discordant counts, switching
to the normal approximation above ~1,000 discordants, while the
behavioral latches ride the Hoeffding-bounded looks. The family budget
(α = 0.005, power analysis documented at the constant) is split
across channels so racing channels cannot multiply false-positive
odds — the null-persona invariant is *family-wise*: a coin-flip
player must never light the read, however many channels watch it.
This is the practical cousin of modern anytime-valid inference
[Howard, Ramdas, McAuliffe & Sekhon 2021], implemented in counting
arithmetic.

Two families exist: **Family A** (the read: published × book ×
McNemar × lateral) funds aggression; **Family B** (drift: alternation
and mean-gap sign tests against a frozen 15-round reference — the
classical sign test of Dixon & Mood [1946]) funds only a narration
flag and re-learning posture, never a hunt.

### 2.3 Latches, spends, and the two-directional contract

A look that clears its bound **latches** the channel (a Schmitt
trigger, in the electronics sense of Schmitt [1938]: open at the look
bound ≈4–5σ, release at z<1 — hysteresis, not a second test). The
spend of a latched channel is its lift **shrunk by one standard
error** — SE-shrunk spends mean a barely-significant read buys barely
any aggression.

Honesty is two-directional, and two mechanisms enforce the release
side:

- **The dwell release** (v9 verification round). The Schmitt pair has
  an unbounded dead zone: under heavy dilution a diluted z can
  asymptote just above 1.0 forever, holding a latch that spends
  nothing. A latched family whose spend sits below a behavioral floor
  (0.05) for five consecutive round boundaries releases outright —
  release keyed to *harm*, with the product assertion kept at a hard
  `earned == 0.0`, never an epsilon.
- **Round-boundary snapshots.** In-round consumers may spend only the
  snapshot taken at the last boundary (`earned_snapshot`,
  `book_spend_snapshot`, authority flags): a latch that opens
  mid-round must not open hunts before a boundary has seen it.

The evidence context's most instructive failures are recorded as
receipts in ADR-022: the discrete look schedule is **chaos-coupled**
to trajectories — any physics change re-rolls where the looks land,
so benchmarks assert channel *strength* (z, monotone in n) alongside
latch outcomes, and fixture re-baselines require paired supply
receipts (`FunnelStats`: the exact eligibility funnel from moves →
two-sided choices → declarations → records).

### 2.4 Exploitation posture (inspiration, not theorem)

The stance is *inspired by* the safe-exploitation literature
[Johanson, Zinkevich & Bowling 2007; Ganzfried & Sandholm 2015]: hunt
aggression scales with proven regularities and unwinds as evidence
dilutes. The honest caveats: this is statistical authorization, not a
game-theoretic exploitability bound; spends unwind at round
boundaries (snapshot-gated), not instantaneously; the session
discipline latch deliberately persists survival sharpness past a
marginal evidence release; and while no *style* multiplier touches
the floors, the ADR-018 opening thins them for an unread CPU by
design. What is invariant: escape margins never fall below their
learned floors, and boxer-aversion floors only rise.

---

## 3. Context: MEMORY — "what persists, and what it costs"

### 3.1 The wire

Everything durable lives in a sectioned binary format (`WRM2`):
independently decodable sections for the ensemble weights, the
books, the episodic corpora, ledgers, drift epochs, turn timing. The
discipline, enforced by a golden-brain tripwire test (a checked-in
serialized brain that must keep decoding): **a schema change must
never wipe what the CPU learned about a human.** Precisely: sections
decode independently, so an incompatible section degrades to partial
survival rather than total loss; sections that have changed shape
carry version bytes and dual-decode from their previous form; loads
sanitize (NaN/range sweeps), because a corrupted float in a weight is
a silent lobotomy. This is graceful degradation with tripwires, not a
formal forward-compatibility proof.

Deliberately *not* persisted: the session doze-exit latch (§4.2), the
dwell counter, all diagnostics. The beatable opening belongs to
unread sessions; wits earned against this human re-latch at load via
the same refresh path everything else uses.

### 3.2 Self-knowledge ledgers

Beyond the opponent model, the CPU keeps books about *itself in this
matchup* (ADR-021's nine surfaces): which of its hunt tactics have
killed this player (episodic attempts with a horizon, resolved
exactly once per round); which weapons bait them, including
deliberate exploratory mine placements (an exploration floor — the
bait book was measured data-starved before the policy existed); how
they kill *it*, with a chased flag — deaths while being hunted raise
its escape margins against this player specifically (boxer aversion:
floors only rise); and a ring-buffer of round summaries feeding the
style bandit.

### 3.3 The epistemic self-map

The 96-cell hazard book doubles as a map of ignorance: populated /
thin / unseen cells are counted and named, surfacing "situations it
has never seen you in" to the notebook. Active thin-cell steering was
considered and explicitly deferred (ADR-021) — the map is narration
and audit today, not a drive; the in-game "Curiosity" behavior is an
unrelated approach heuristic. The
unknown is a first-class, countable quantity — the difference
between *your read is 0.6* and *I have literally never watched you
near food while chased*.

### 3.4 The between-round ratchet

An **exact Stoer–Wagner minimum cut** [Stoer & Wagner 1997] (~50
lines, ~1ms at this scale) partitions the hazard cells' co-movement
graph to name *which region of situation-space moved* when the drift
alarm fires — "your close-quarters game changed; your open-field game
didn't." Integration honesty: today this runs as an offline
diagnostic (an example binary invoked by the weekly learning audit),
capable of round-boundary execution but not wired into the live loop.
The
vendored dynamic-mincut crate was measured unusable at integration
scale (its own benches predicted it); the boring exact algorithm won.
The methodological lesson is banked as project memory: run a
dependency's own benches before integrating it.

---

## 4. Context: BEHAVIOR — "what the read is allowed to buy"

### 4.1 Spending topology

The earned read buys, in order of cost: tick-perfect wits (the doze
ends), projection-bent interception, hunt margin, and finally style
amplification (the Exp3 drive multiplier scales the *spend*, not the
floors). The novice envelope is an invariant: competent basics, a
visibly beatable opening, no false sharpness from time or geometry.

### 4.2 The beatable opening (ADR-018)

An unread CPU is deliberately slow-witted: it re-decides every Nth
frame, holding heading between decisions, with wall and ring reflexes
always on and **trails deliberately invisible** — a fixated casual
player rams trails, and dying to the trail *you* laid is the classic
earned Tron kill. The doze's reflex set is exactly the static-
geometry class: walls, sealed-ring doom, its own mines
(self-knowledge, not sharpness), and — since v9 — one-step dead-end
pockets. Survival discipline is binary on any proven read (the
half-woken middle was measured to lose games both pure modes win),
and the **session doze-exit latch** guarantees discipline never
regresses mid-session on a marginal evidence flicker: once wits are
earned, sloppiness does not return until the read has genuinely
lapsed (dwell release) and a session boundary passes.

### 4.3 Weapons and the uniform immunity law

Three weapons, one law refined across ADR-023 and the v8/v9/v11
rounds: **the firer is immune to their own discharged weapon** —
laser, blast, and fire alike — because the counterplay to your own
ordnance is memory, and punishing the past (severing a trail laid
before the plant) was measured to be pure noise-damage. The laser's
dual-test (ADR-023) makes the beam exist across the movement
transition it fires into — the painted frame can never show an un-hit
intersection (the renderer is a *contract*: solid means hot, faded
means spent). Napalm's catch is *literal touch* (v11): contact sets
the burn state directly rather than hoping the victim still overlaps
a ground flame at the next hazard tick.

---

## 5. Context: WORLD — versioned physics as an evidence guarantee

Every physics rule is stamped into `ARENA_VERSION` (v1…v11), recorded
per round, and **replays pin their recorded version** — a ghost
replays bit-exact under the physics it was played on, for as long as
that version's code paths are kept (they are kept deliberately; the
suite pins them). This
is not nostalgia; it is what makes the evidence context auditable:
the owner's recorded rounds are executable receipts (the v7 laser fix
shipped with the owner's own ghost as its regression test), and the
learning benchmarks can run version-pinned A/B arms
(`play_v(games, seed, warm, version)`) to separate physics effects
from learner effects.

The discipline (ADR-022): one physics change per version, landed
through a fixed ritual — physics contract tests, prior-version
replay identity, live-to-ghost identity, statistical invariant
suites, and paired benchmark receipts for anything that moved.
Re-baseline *measurements of the arena*; never re-baseline
*measurements of the learner*.

---

## 6. Context: VERIFICATION — personas, instruments, adversaries

### 6.1 Personas as null and positive controls

The statistical suites drive scripted personas against the full
engine: null controls (coin-flip slalomers, habit-free wanderers)
that must never latch anything, anywhere, at any point — checked per
channel, not at the endpoint — and positive controls (absolute
habits, strict alternators, heading-relative break biases) that must
be learned whenever adequately supplied. The distinction between "not
learned" and "not supplied" is load-bearing: the eligibility funnel
(§2.3) is instrumented precisely so a supply collapse is never
mistaken for a pipeline break, and vice versa.

### 6.2 The memory instrument

"Remembering you must never cost wins" is a product invariant tested
as **five paired 90-game warm/cold arms** scored by expected points
(draws count half — a draw is not a loss), asserting mean and median
paired gap ≤ 5 points with every per-seed gap published. The pooled
predecessor was retired when receipts showed a single pathological
spawn-lap interaction could veto the doctrine alone; the instrument
keeps the outlier *visible* instead of re-baselined away. This is
group thinking borrowed from paired-design experimental practice:
identical seeds, arms differing only in retained memory.

### 6.3 Adversarial consultation as process

Every design and landing passes external adversarial review (two
independent frontier-model consultants) with a SOUND/UNSOUND verdict
and blocking findings; disagreements resolve by *experiment* (the
five-seed instrument was itself the arbitration of one such split).
The verification context's culture is receipts-first: a claim
("the wake will narrow the margin") is something you measure, and
being empirically refuted in the record is normal and kept.

---

## 7. The plateau gate: why there is no neural network in the loop

The gate exists; the challenger does not. A plateau detector
(drift-vetoed) guards the door: heavier machinery may be *evaluated*
only if the counting stack stops improving against a **stationary**
opponent — and against a human who keeps changing, a plateau is
indistinguishable from the opponent moving. No SONA/kernel challenger
has been built, because the gate has never opened. So far the humans
keep moving. This is the
architecture's thesis in miniature: at 100–400 decisions per round,
hand-shaped counting estimators with exact tests dominate
gradient-trained function approximation on sample efficiency,
auditability, and wasm frame-budget cost — and the burden of proof
sits, permanently, on the heavier machinery.

---

## 8. Ubiquitous language (glossary)

| Term | Meaning |
|---|---|
| **earned read** | SE-shrunk, family-tested lift over the player's own base rate; the only currency difficulty accepts |
| **look** | a geometric-schedule significance checkpoint with its own α spend |
| **latch** | Schmitt-style hysteresis state of a proven channel |
| **spend** | how much behavior a latched read may buy this round (snapshot-gated) |
| **dwell release** | forced family release after K consecutive below-floor spends |
| **doze** | the beatable opening's held-heading cadence; ends when any read latches |
| **ghost** | a recorded round, bit-exact replayable under its recorded physics |
| **funnel** | the instrumented eligibility pipeline from frames to evidence records |
| **persona** | a scripted null/positive-control opponent |
| **arm** | one seeded benchmark run (warm = memory kept, cold = wiped) |
| **receipt** | a published measurement that licenses a change (paired, seeded) |
| **book** | a KT-estimated, decayed, class-conditional model (hazard/side/bait) |
| **ratchet** | the between-round mincut naming which situation region drifted |

---

## References

- Auer, P., Cesa-Bianchi, N., Freund, Y., & Schapire, R. E. (2002).
  The nonstochastic multiarmed bandit problem. *SIAM Journal on
  Computing*, 32(1), 48–77. (Exp3; the style portfolio.)
- Bard, N., Johanson, M., Burch, N., & Bowling, M. (2013). Online
  implicit agent modelling. *Proc. AAMAS 2013*. (Portfolio of
  counter-strategies instead of full opponent-policy estimation.)
- Cover, T., & Hart, P. (1967). Nearest neighbor pattern
  classification. *IEEE Trans. Information Theory*, 13(1), 21–27.
  (The episodic k-NN memory.)
- Dixon, W. J., & Mood, A. M. (1946). The statistical sign test.
  *JASA*, 41(236), 557–566. (The drift family's tests.)
- Evans, E. (2004). *Domain-Driven Design: Tackling Complexity in the
  Heart of Software.* Addison-Wesley. (Bounded contexts; this
  document's organizing frame.)
- Freund, Y., & Schapire, R. E. (1997). A decision-theoretic
  generalization of on-line learning… *JCSS*, 55(1), 119–139.
  (Multiplicative weights underlying the ensemble.)
- Freund, Y., Schapire, R. E., Singer, Y., & Warmuth, M. K. (1997).
  Using and combining predictors that specialize. *Proc. STOC 1997*.
  (Sleeping experts / abstaining specialists.)
- Ganzfried, S., & Sandholm, T. (2015). Safe opponent exploitation.
  *ACM Trans. Economics and Computation*, 3(2). (The exploitation
  posture: deviate only within a provable budget.)
- Herbster, M., & Warmuth, M. K. (1998). Tracking the best expert.
  *Machine Learning*, 32(2), 151–178. (Fixed-share; the two-horizon
  weights and per-depth VOMM mixing.)
- Hoeffding, W. (1963). Probability inequalities for sums of bounded
  random variables. *JASA*, 58(301), 13–30. (Exact look bounds on
  fair-coin sums.)
- Howard, S. R., Ramdas, A., McAuliffe, J., & Sekhon, J. (2021).
  Time-uniform, nonparametric, nonasymptotic confidence sequences.
  *Annals of Statistics*, 49(2). (The modern frame for anytime-valid
  peeking; implemented here as geometric looks + spending.)
- Johanson, M., Zinkevich, M., & Bowling, M. (2007). Computing robust
  counter-strategies. *NIPS 2007*. (Restricted Nash response; the
  safety/exploitability trade the floors encode.)
- Krichevsky, R., & Trofimov, V. (1981). The performance of universal
  encoding. *IEEE Trans. Information Theory*, 27(2), 199–207. (KT
  estimators throughout the books and VOMM.)
- Lan, K. K. G., & DeMets, D. L. (1983). Discrete sequential
  boundaries for clinical trials. *Biometrika*, 70(3), 659–663.
  (Alpha-spending across scheduled looks.)
- McNemar, Q. (1947). Note on the sampling error of the difference
  between correlated proportions… *Psychometrika*, 12(2), 153–157.
  (The exact paired test against the base-rate rival.)
- Pocock, S. J. (1977). Group sequential methods in the design and
  analysis of clinical trials. *Biometrika*, 64(2), 191–199.
  (Group-sequential looks.)
- Schmitt, O. H. (1938). A thermionic trigger. *Journal of Scientific
  Instruments*, 15(1), 24–26. (Hysteresis latching, by loving
  analogy.)
- Stoer, M., & Wagner, F. (1997). A simple min-cut algorithm. *JACM*,
  44(4), 585–591. (The between-round ratchet.)
- Thompson, W. R. (1933). On the likelihood that one unknown
  probability exceeds another… *Biometrika*, 25(3/4), 285–294.
  (Tactic-arm posterior sampling.)
- Willems, F. M. J., Shtarkov, Y. M., & Tjalkens, T. J. (1995). The
  context-tree weighting method: basic properties. *IEEE Trans.
  Information Theory*, 41(3), 653–664. (The VOMM's depth-mixing
  ancestry.)
