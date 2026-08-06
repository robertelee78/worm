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
compass  (POSITIVE)   n=5230    read 99.7%  chance 34.1%  z= +100.1
coinflip (NULL)       n=9930    read 34.7%  chance 34.8%  z=   -0.3
lefty    (ACCEPTANCE) n=127     read 81.1%  chance 50.0%  z=   +7.0
```

The null control is what makes the other two rows evidence: the same CPU that
reads `lefty` at 81% remains unable to read an opponent with no habit, so the
suite is not leaking the answer into the forecast.

## Learning, and then winning

The claim is not that the CPU predicts you — it is that predicting you makes it
**beat** you. `cargo test --test domination` holds board, seeds and opponent
fixed and varies only whether the CPU may remember:

```
COLD (cannot learn)   cpu 28 player 2 draw 0   win  93%
WARM (remembers you)  cpu 29 player 1 draw 0   win  97%   lift 79%
```

(The pre-engagement-gates build measured the project's first zero-death
sweep — COLD 30-0, WARM 30-0, lift 83% — before three deaths were traded
deliberately for a measured ~5-point win-rate and engagement gain on the
browser board; [ADR-012](docs/adrs/012-two-swarm-findings-implemented.md)
records the trade.)

That test was failing until recently, with the *warm* CPU winning less — see
[ADR-009](docs/adrs/009-learning-must-convert-into-winning.md) for the two
defects that caused it and the two flaws in the experiment itself.


A read is only worth something if it reaches the wheel. Against `lefty`, over
40 games with one persistent brain:

```
read_rate:  0.569 → 0.676 → 0.760 → 0.779 → 0.788 → 0.804
tier:            3      4      4      4      4      4
record:     CPU 37 · player 3
```

`read_rate` is **lift over the player's own base rate**, and it drives how much
survival margin the CPU will spend on an intercept. Confidence buys
*commitment*, never safety: the survival floor is untouched at every read rate,
and the dodge, ring-evacuation and wall-follow layers never consult the hunt
floor. A well-read player faces a CPU that takes intercepts it would otherwise
decline. They never face one that suicides. See
[ADR-007](docs/adrs/007-difficulty-earned-by-reading-you.md).

### Where it plateaus, and why

`read_rate` settles near 0.80 rather than climbing indefinitely. That is the
model reaching the limit of what its features can express: it holds a single
global turn prior, so it learns *"this player breaks left"* but not *"this
player breaks left when the wall is three cells away and the CPU is behind
them"*. The remaining headroom is richer features, not more data — the corpus
is full by round three. Compensating with a difficulty multiplier would just
restore the arbitrary clock ramp ADR-007 deleted.

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
wasm-pack build --target web --features wasm
cp pkg/worm.js pkg/worm.d.ts pkg/worm_bg.wasm pkg/worm_bg.wasm.d.ts web/pkg/
python3 -m http.server -d web 8080
```

The browser build keeps a per-player brain in IndexedDB, keyed by an identity
stored beside it in the same database — deliberately, so the two share an
eviction fate. See [ADR-005](docs/adrs/005-durable-player-identity.md) for why
that mattered.

## Development

```bash
cargo test                                            # unit + integration
cargo test --test persona_learning -- --nocapture     # the claim, ~45s
cargo test -- --include-ignored                       # including the acceptance test
cargo bench --bench cpu_ai_bench                      # CPU benchmarks
```

Layout: `src/lib/game.rs` (rules, board, power-ups), `src/lib/cpu_ai.rs`
(opponent model, decision layers, brain persistence), `src/lib/web_state.rs`
(browser wire format), `src/main.rs` (terminal client), `web/` (canvas client).

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

### A note on the numbers in this repo

Win-rate and read-rate figures are quoted from seeded runs, and the seeds are
in the harnesses. Where a change turned out to be measurement-neutral it is
recorded as measurement-neutral rather than credited with a delta — several
fixes in ADR-004 are exactly that, kept because a test proves them rather than
because a number moved. Where a claim has been withdrawn, the withdrawal sits
in the ADR next to the original claim.
