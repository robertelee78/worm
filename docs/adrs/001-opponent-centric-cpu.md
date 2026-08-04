# ADR-001: Opponent-Centric CPU Intelligence

## Status
Accepted (Post-Spike Reformulation)

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

1.  **Cold Start (Mode 0):** Continue using `score_based_decide` (spatial survival+food+
    hunt scoring) until `COLD_START_EPISODES` is reached. This is unchanged.
2.  **Adaptive Mode (Mode 1):**
    *   Store **opponent-centric episodes**: `PlayerContext -> PlayerNextDirection`.
    *   Encode `PlayerContext` using a high-recency, transition-sensitive vector (bigrams),
      analogous to `rps-ai`'s "human moves" and "bigram" features, NOT just spatial distances.
    *   Vote on the player's likely next move.
    *   **Act to intercept:** Choose the CPU move that minimizes the player's predicted
      available open-space and maximizes the CPU's own open-space relative to the player.

## Consequences
*   Positive: Moves the CPU from a "self-survival" agent to a "strategic opponent" agent,
  directly addressing the "doesn't learn" and "runs into walls" issues.
*   Risk: A naive switch could destabilize the cold-start safety floor. The dual-mode design
  mitigates this by keeping Mode 0 intact.
*   Risk: A purely predictive model might overfit to short-term patterns. The existing
  `recency` weights and `EXPLORE_RATE` in the `aggregate` function will act as safeguards.
