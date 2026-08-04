# Hypothesis: Opponent-Centric Intelligence for TRON CPU

## Context
The current `worm` CPU (`src/lib/cpu_ai.rs`) uses a self-centric episodic memory: it records its own moves and rewards based on survival. This results in a reactive agent that struggles to anticipate the player.

## Formal Hypothesis
By refactoring the `CpuBrain` to an **Opponent-Centric Learner**, the CPU will transition from a "survival maximizer" to a "predictive counter-agent." 

Specifically, if the situation vector $\mathbf{v}$ is redefined as $\mathbf{v}_t = f(\text{PlayerState}_t)$ and the reward $R$ is maximized for the error $\epsilon = \| \text{PlayerDirection}_{t+1} - \text{Predict}(v_t) \|$, then the k-NN mechanism will naturally cluster player movement habits, allowing the CPU to play moves that counter the player's anticipated trajectory.

## Spike Plan

### Spike 1: The "Pattern Predictor" Test
**Objective:** Prove the k-NN mechanism can learn a non-linear player pattern without a full engine integration.
**Method:** 
1.  Create a minimal `PlayerBrain` in a test module.
2.  Inject a synthetic sequence of player moves: `[Up, Right, Down, Left, Up, Right, Down, Left, ...]`.
3.  Verify that the `aggregate` function converges on `Right` when the current state is `Up`.
**Success Criterion:** $\text{PredictionAccuracy} > 80\%$ for the repeating sequence.

### Spike 2: State Access & Convergence Audit
**Objective:** Ensure `WormGame` provides sufficient telemetry for a player-centric encoder and that the transition from `survival` to `prediction` doesn't cause "Death by Vacuum" (ignoring walls).
**Method:** 
1.  Measure `WormGame` state retrieval latency for player telemetry.
2.  Simulate a hybrid reward: $R_{total} = \alpha R_{\text{survival}} + (1-\alpha) R_{\text{prediction}}$.
3.  Identify the $\alpha$ threshold where the CPU stops hitting walls while still predicting.

### Spike 3: Convergence Stability
**Objective:** Validate that the `CpuBrain` can handle the `PlayerDirection` vector space without the zero-vector trap identified in `rps-ai`.
**Method:** 
1.  Run a convergence test on the cosine similarity of the player-move embeddings.

## Success Metric
A shift from the current baseline (**-8.6 moves**) to a positive or zero-drag baseline in the `cpu_ai_bench`.
