# Hypothesis: Opponent-Centric Intelligence for TRON CPU

## Context
The current `worm` CPU (`src/lib/cpu_ai.rs`) uses a self-centric episodic memory: it records its own moves and rewards based on survival. This results in a reactive agent that struggles to anticipate the player.

## Formal Hypothesis
By refactoring the `CpuBrain` to an **Opponent-Centric Learner**, the CPU will transition from a "survival maximizer" to a "predictive counter-agent."

Specifically, if the situation vector $\mathbf{v}$ is redefined as $\mathbf{v}_t = f(\text{PlayerState}_t)$ and the reward $R$ is maximized for the error $\epsilon = \| \text{PlayerDirection}_{t+1} - \text{Predict}(v_t) \|$, then the k-NN mechanism will naturally cluster player movement habits, allowing the CPU to play moves that counter the player's anticipated trajectory.

## Spike Plan

### Spike 1: The "Pattern Predictor" Test ✅
**Objective:** Prove the k-NN mechanism can learn a non-linear player pattern without a full engine integration.
**Method:** Inject synthetic sequence `[Up, Right, Down, Left, ...]` into `PlayerBrain`.
**Result:** 100% prediction accuracy after 20 cycles. k-NN mechanism is sound.

### Spike 2: State Access & Convergence Audit ✅
**Objective:** Ensure `WormGame` provides sufficient telemetry for a player-centric encoder.
**Method:** Implemented `encode_player_context` (13-dim player-centric vector) + `predict_player_move`.
**Result:** Opponent model reaches confidence=1.0 by frame 100 against wall-follower.

### Spike 3: Convergence Stability ✅
**Objective:** Validate the `CpuBrain` handles the player-direction vector space without the zero-vector trap.
**Method:** L2-normalised 16-dim vector (13 coded + 3 zero-padded). Cosine similarity is meaningful.

### Spike 4: Projectile/Bomb/Laser Avoidance ✅
**Objective:** Prove the CPU can detect and evade live threats (tri-shot bolts, planted bombs, laser beams).
**Method:** Add threat vectors to `cpu_decide` — scan projectiles/bombs, penalize directions into blast/beam paths.
**Result:** CPU survives power-up engagements that previously killed it.

### Spike 5: Power-Up Offensive Usage ✅
**Objective:** Prove `should_fire` can create kill opportunities from held power-ups.
**Method:** Fire laser/trishot when player is in line of fire; bomb when player is in blast radius.
**Result:** CPU converts power-up pickups into player kills.

### Spike 6: Chokepoint Intercept for Wall-Followers ✅
**Objective:** Kill wall-follower opponents who are always >10 cells away (intercept range never triggers).
**Method:** Predict which corner the player reaches next, cut across arena to lay a trail barrier.
**Result:** Adaptive CPU wins vs wall-follower, not just parity.

## Key Finding (Post-Implementation)
The original `score_direction`-driven approach (open-space maximising) was fundamentally incompatible with TRON survival. **Open space is a trap** — the wall-follow pattern is the actual survival strategy. The opponent model must MODIFY the survival strategy, not REPLACE it.

## Current Architecture (2026-08-04)
1. **Cold start:** `wall_follow_decide` (same as naive opponent)
2. **Adaptive mode:** wall-follow base + defensive avoidance (≤2 cells) + intercept (2-10 cells, confidence≥0.6) + adjacent food grab + projectile/bomb avoidance + power-up firing
3. **Opponent model:** `PlayerBrain` with k-NN recall, confidence-weighted prediction, multi-frame iterative prediction
4. **Cross-game persistence:** `shared_brain` accumulates episodes across games
5. **Kill positioning:** intercept the predicted player path + chokepoint corner-cutting for wall-followers
6. **Active food seeking:** BFS pathfinding to nearest food when safe
7. **Deterministic benchmark:** seeded RNG for reliable measurement
8. **Self-memory vote (gated):** the CPU's own survival episodes vote via k-NN
   (confidence ≥ 0.4 + open-space gate, deterministic favourite) — breaks the
   wall-follower stalemate (familiar wins 1% → 100%) without costing held-out
   (chaser 100% maintained)

## Success Metric
Baseline shift: from **-8.6 moves / -8.1 food** to **≥0 moves / ≥0 food** (beats naive vs wall-follower).
**Held-out chaser: adaptive 100% wins vs naive 8% wins** — the opponent model is decisively better against aggressive opponents.
