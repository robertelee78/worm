# ADR-001: Opponent-Centric CPU Intelligence

## Status
Accepted (Post-Implementation Reformulation)

## Context
The `worm` CPU (`src/lib/cpu_ai.rs`) was originally designed as a port of `rps-ai`'s
k-NN mechanism, but the "situation" and "reward" were recontextualized in a way that
made the CPU **self-centric**: every episode it records is `situation -> my best move`.
The baseline performance was **-8.6 moves / -8.1 food** against a naive wall-follower,
meaning **learning is a net-drag**.

Analysis of `rps-ai` (reference: `/opt/rps-ai/lib/engine.ts`, `/predict.ts`,
`/feature-embed.ts`) revealed that its power lies in its **opponent model**: it stores
`situation -> what the HUMAN did next`. This prediction error is the direct,
learnable signal.

## Spike Findings (Spike 1: Pattern Prediction Test)
A minimal `PlayerBrain` (storing `LastMove -> NextMove` with k-NN voting) was
implemented as a unit test (`spike_1_knn_predicts_repeating_pattern`).

Result: The k-NN mechanism correctly predicted a 4-step repeating pattern
(`Up, Right, Down, Left`) with 100% accuracy after 20 cycles. This confirms
that **the core k-NN memory + voting logic is sound for opponent modeling**.

## Decision
Refactor the CPU intelligence to a **dual-mode opponent-centric architecture**:

1.  **Cold Start (Mode 0):** Use `wall_follow_decide` (same as naive benchmark
    opponent) until `COLD_START_EPISODES` is reached. This guarantees the adaptive
    CPU is never worse than the baseline during warm-up.
2.  **Adaptive Mode (Mode 1):**
    *   **Base strategy:** `wall_follow_decide` (proven survival). The wall-follow
        pattern is NEVER abandoned for a low-confidence prediction.
    *   **Opponent-model episodes:** `PlayerContext -> PlayerNextDirection`,
        recorded live on every player move.
    *   **Player-centric encoder:** player open-neighbours, wall/trail distances,
        player→food, player→CPU-threat, travel direction.
    *   **Defensive avoidance:** when the predicted player position is within 2
        cells, pick the direction that maximises distance (with wall-follow bonus).
    *   **Kill positioning (intercept):** when confidence >= 0.6 and the predicted
        player is 2-10 cells away, move toward the predicted future position
        (2-5 frames ahead) to create a trail barrier. Best-intercept selection
        scores targets by distance + frames-ahead; wall-follow bonus (1.0) and
        open-space check prevent marginal/trap deviations.
    *   **Adjacent food grab:** only deviate from wall-follow for directly
        adjacent food (doesn't break the survival pattern).
    *   **Cross-game persistence:** `CpuBrain` is shared across games via
        `shared_brain` in the benchmark, simulating rps-ai's persistent DB.

## Implementation Notes (2026-08-04)
*   The original `score_direction`-driven approach (open-space maximising) was
    found to be fundamentally incompatible with TRON survival. Open space is a
    trap — the wall-follow pattern is the actual survival strategy.
*   The opponent model reaches confidence=1.0 by frame 100 against a
    wall-follower player. Predictions are used defensively, not offensively.
*   Benchmark vs wall-follower: adaptive roughly matches naive (high variance
    from unseeded RNG). Both survive to the arena-fill limit (~3920 frames).
    Two opposite-direction wall-followers never approach each other closely
    (>38 cells), so the intercept cannot trigger — the game is a stalemate.
*   Benchmark vs chaser (held-out): adaptive wins 100% consistently across
    multiple runs. The chaser always approaches the CPU, so the intercept
    triggers constantly. Adaptive kills the chaser in ~30 frames (fast kill
    via prediction); naive gets killed in ~72 frames (slow death).
*   Key lesson: in TRON, the opponent model should MODIFY the survival strategy,
    not REPLACE it. rps-ai's prior-blend design prevents the memory from walking
    into walls; the same principle applies here.
*   Self-memory vote (rps-ai's own-episode loop, added 2026-08-04): once
    `memory_size >= COLD_START_EPISODES`, the CPU's own survival episodes cast a
    k-NN vote — encode situation → recall → aggregate → legal favourite (argmax,
    deterministic, no temperature/explore noise). It fires only when aggregate
    confidence (margin × support × maturity) >= `SELF_VOTE_MIN_CONFIDENCE` (0.4)
    AND the vote destination is at least as open as wall-follow's — "memory
    modifies survival, never replaces it". Bench (seeded, reproduced): held-out
    chaser adaptive wins 100/100 (pre-vote 100/100, un-gated noisy vote 96/100);
    familiar wall-follower adaptive wins 100/100 (pre-vote 1/100 — the vote
    breaks the perimeter stalemate and kills in ~10 frames).

## Update (2026-08-05, audit round 2)
*   Self-memory vote refinements (all verified against the seeded bench):
    crash episodes (reward 0) no longer vote FOR the move that died; the
    5% close-evasion explore draws from the threat-filtered candidate set;
    tier-3 food pathing obeys the shared `open_floor` survival floor instead
    of a hard-coded 10%; `m_due`'s tie-break now matches its longest-unseen
    doc; `SurvivalMemory` is only reported when the vote actually changed the
    move; dead `sample_with_temperature` removed (decisions are gated argmax —
    cpu_decide's doc now says so).
*   Fairness/telegraph: the CPU's laser charges visibly for
    `LASER_TELEGRAPH_FRAMES` (10) before firing; tri-shot only fires into the
    forward arc.
*   Bench after these changes: held-out chaser adaptive wins 99/100 (the
    two earlier round-1 non-wins were removed unearned tail-cell kills;
    round 2 recovered one). Verdict: WINS.

## Consequences
*   Positive: Moves the CPU from a "self-survival" agent to a "strategic opponent"
    agent. The opponent model infrastructure is in place and learning.
*   Positive: The wall-follow base guarantees the adaptive CPU is never worse
    than the naive baseline.
*   Risk: The wall-follow base is so strong that the opponent model's contribution
    is marginal against a wall-follower opponent. The real test requires a
    held-out opponent (per the AGENTS.md bench rule).
*   Risk: Cross-game persistence means the shared brain accumulates episodes
    across games. Early bad experiences could poison the memory. The `MAX_EPISODES`
    retention cap (800) mitigates this.
