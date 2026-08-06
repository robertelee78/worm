# worm

A snake/Tron-lightcycle hybrid whose opponent is trying to learn *you*.

The game is the easy part. The claim is that the CPU builds a model of the
specific human it is playing, gets measurably better at predicting them across
matches, and can show you the evidence. That claim is easy to make and easy to
fool yourself about, so much of this repository is the machinery for testing
whether it is true — including the parts that currently say **no**.

---

## The claim, and how to falsify it

```bash
cargo test --test persona_learning -- --nocapture
```

Scripted personas play the game with a **known habit**. The CPU's forecasts are
scored *only* on the frames where the persona actually exercised that habit —
the frames where the answer was a fact about the player rather than a fact
about the board. Continuing down a corridor, or turning because there was only
one way out, is free accuracy and is excluded.

The suite is built around controls, because a test that cannot fail proves
nothing:

| | persona | what it establishes |
|---|---|---|
| **POSITIVE** | `compass` — prefers a fixed direction | An absolute habit the model provably *can* hold. If this stops being learned, the pipeline has regressed. |
| **NULL** | `coinflip` — no habit at all | There is nothing to learn. If the CPU ever reads this above chance, the harness is leaking the answer and every other number here is void. |
| **ACCEPTANCE** | `lefty` — "break left when cornered" | Heading-relative, and the actual product goal. |

Latest measured:

```
compass  (POSITIVE)   n=10052   read 95.3%  chance 34.3%  z= +128.9
coinflip (NULL)       n=15292   read 34.8%  chance 35.0%  z=   -0.5
lefty    (ACCEPTANCE) n=144     read 74.3%  chance 50.0%  z=   +5.8
```

The null control is what makes the other two rows evidence: the same CPU that
reads `lefty` at 74% remains unable to read an opponent with no habit, so the
suite is not leaking the answer into the forecast.

## Learning, and then winning

The claim is not that the CPU predicts you — it is that predicting you makes it
**beat** you. `cargo test --test domination` holds board, seeds and opponent
fixed and varies only whether the CPU may remember:

```
COLD (cannot learn)   cpu 26 player 4 draw 0   win  87%
WARM (remembers you)  cpu 30 player 0 draw 0   win 100%   lift 86%
```

An undefeated warm arm against a beaten cold one is the whole product in
two rows: the only difference between them is memory.
[ADR-012](docs/adrs/012-two-swarm-findings-implemented.md) and
[ADR-014](docs/adrs/014-the-codex-corrections.md) record the trades and
reverts behind these numbers, including the changes that measured worse
and were rolled back.

That test was failing until recently, with the *warm* CPU winning less — see
[ADR-009](docs/adrs/009-learning-must-convert-into-winning.md) for the two
defects that caused it and the two flaws in the experiment itself.


A read is only worth something if it reaches the wheel. `read_rate` is
**lift over the player's own base rate** (scaled by which temperament is
winning — see the Exp3 portfolio in the code), and it drives how much
survival margin the CPU will spend on an intercept. Confidence buys
*commitment*, never safety: the survival floor is untouched at every read
rate, and the dodge, ring-evacuation and wall-follow layers never consult
the hunt floor. A well-read player faces a CPU that takes intercepts it
would otherwise decline. They never face one that suicides. See
[ADR-007](docs/adrs/007-difficulty-earned-by-reading-you.md).

### Where the habit models plateau, and what took over

The habit family alone settles around 0.80 lift: a single global turn
prior learns *"this player breaks left"* but not *"…when the wall is three
cells away"*. The headroom turned out to be **why** the player moves, not
finer habit features: the ensemble now carries six errand models —
{food, hunt, weapon} × {holds-their-line, weaves} — each a BFS route step
toward an observed goal, elected by the same fixed-share weights as
everything else. Against a committed forager persona the voluntary-turn
read went from 6% (habit models only) to ~76-85%
([ADR-012](docs/adrs/012-two-swarm-findings-implemented.md),
[ADR-014](docs/adrs/014-the-codex-corrections.md)). Compensating with a
difficulty multiplier would just restore the arbitrary clock ramp ADR-007
deleted.

## The metric, and why the obvious version is wrong

The number a project like this wants to show is "prediction accuracy". Here
that number is worthless: ~95% of frames the player is continuing straight down
an open corridor, so accuracy sits at 84–99% forever and cannot move.

The obvious fix — compare against uniform chance, as
[rps.shaal.dev](https://rps.shaal.dev/) does with its stated 33% baseline —
fails for a subtler reason. An RPS player really *is* near-uniform; they are
trying to be random. A worm player is not. Against a straight-driving opponent,
"always predict Straight" scores 98%, which against a 33% baseline reads as
*"it has found a pattern"*. It has found nothing, and the error is biased
toward flattering the CPU.

So the baseline is **the player's own base rate** — the realized accuracy of an
online "predict their commonest move so far" rival — and significance is an
**exact McNemar test** over the frames where the CPU and that rival actually
disagreed. Against a very predictable player the two agree almost always, and
thousands of frames can carry a dozen frames of real evidence. That gets
reported honestly rather than as a large green number.

## Playing it

**Terminal**

```bash
cargo run --release
```

`WASD` or arrow keys to steer · `Space` fires a held power-up · `P` pause ·
`Q` quit. The brain persists to `$XDG_DATA_HOME/worm/worm_brain.bin` (override
with `WORM_BRAIN`).

**Browser**

```bash
wasm-pack build --target web --out-dir web/pkg --features wasm
# Deployed: https://worm.robertgpt.ai (Apache, DocumentRoot symlinked to
# web/ — the build above goes live immediately). After a rebuild, bump the
# cache-bust version in THREE places: index.html's app.js?v=, and app.js's
# BUILD const + pkg/worm.js?v= import.
# Local dev without Apache:
python3 scripts/serve.py 8080   # no-store headers, so rebuilds never serve stale
```

The browser build keeps a per-player brain in IndexedDB, keyed by an identity
stored beside it in the same database — deliberately, so the two share an
eviction fate. See [ADR-005](docs/adrs/005-durable-player-identity.md) for why
that mattered.

## Development

```bash
scripts/eval.sh                                       # THE GAUNTLET — run before every merge
cargo test                                            # unit + integration
cargo test --test persona_learning -- --nocapture     # the claim, ~45s
cargo run --release --example engagement_probe -- browser   # engagement + death census
cargo run --release --example intent_probe -- 24 1500 20260805  # errand-model reads
node scripts/page_probe.mjs                           # the SERVED page, headless (needs playwright)
cargo bench --bench cpu_ai_bench                      # CPU benchmarks
```

The gauntlet is the improvement flywheel
([ADR-013](docs/adrs/013-the-improvement-flywheel.md)): champion = `main`,
candidates prove themselves against the fixed seeds, and the measured
numbers go in the commit message — the receipts are the ledger.

Layout: `src/lib/game.rs` (rules, board, power-ups), `src/lib/cpu_ai.rs`
(opponent model, decision layers, brain persistence), `src/lib/web_state.rs`
(browser wire format), `src/main.rs` (terminal client), `web/` (canvas
client), `examples/` (the eval probes), `scripts/` (gauntlet, dev server,
page probe).

The brain is serialized in a sectioned format so that a schema change costs
only the section it invalidates. What the CPU has learned about *you* — habit
priors, head-to-head record — is encoding-independent and survives every future
change to the feature space. That is a hard requirement, not an optimisation:
a game whose premise is "it remembers you" cannot afford a release that resets
everyone.

### Decisions

Architecture decisions live in [`docs/adrs/`](docs/adrs/) and are treated as
living documents — if one disagrees with the code, the ADR is the bug.

- [ADR-001](docs/adrs/001-opponent-centric-cpu.md) — opponent-centric CPU
- [ADR-002](docs/adrs/002-responsive-browser-arena.md) — responsive browser arena
- [ADR-003](docs/adrs/003-transparent-cpu-telemetry.md) — truthful CPU telemetry
- [ADR-004](docs/adrs/004-cpu-fundamentals.md) — fixing the CPU's fundamentals
- [ADR-005](docs/adrs/005-durable-player-identity.md) — durable identity and brain integrity
- [ADR-006](docs/adrs/006-honest-read-rate.md) — measuring against the player's own base rate
- [ADR-007](docs/adrs/007-difficulty-earned-by-reading-you.md) — difficulty earned by reading you, not by the clock
- [ADR-008](docs/adrs/008-bomb-as-a-disguised-mine.md) — the bomb becomes a mine disguised as food
- [ADR-009](docs/adrs/009-learning-must-convert-into-winning.md) — learning must convert into winning
- [ADR-010](docs/adrs/010-what-the-cpu-learns-from.md) — what the CPU learns from, and what it may know
- [ADR-011](docs/adrs/011-intent-inference-and-engagement.md) — intent inference and an engaged CPU
- [ADR-012](docs/adrs/012-two-swarm-findings-implemented.md) — errand twins, kinematic traps, and honest silence
- [ADR-013](docs/adrs/013-the-improvement-flywheel.md) — the improvement flywheel
- [ADR-014](docs/adrs/014-the-codex-corrections.md) — the codex corrections: what survived measurement

### A note on the numbers in this repo

Win-rate and read-rate figures are quoted from seeded runs, and the seeds are
in the harnesses. Where a change turned out to be measurement-neutral it is
recorded as measurement-neutral rather than credited with a delta — several
fixes in ADR-004 are exactly that, kept because a test proves them rather than
because a number moved. Where a claim has been withdrawn, the withdrawal sits
in the ADR next to the original claim.
