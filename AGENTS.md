# worm — agent briefing

TRON light-cycle duel in Rust (crossterm terminal UI). The CPU opponent
(`src/lib/cpu_ai.rs`) is a **live episodic-memory learner** ported from
`/opt/rps-ai` — there is no training phase; the loop is the game:

```
every decision → encode situation → recall similar pasts → vote → act
      ↑                                                            |
      └──────── store what ACTUALLY happened (both sides) ─────────┘
```

## Commands

```
cargo build                          # debug build
cargo test --lib --tests             # 10 unit/integration tests
cargo bench --bench cpu_ai_bench     # THE behavioural gate (harness=false, runs main)
cargo run --release                  # play it
```

## The benchmark rule (load-bearing, from /opt/rps-ai/CLAUDE.md)

The bench pits the adaptive CPU against scripted opponents and scores
**survival (moves) + food**. Discipline:

1. **FAMILIAR** opponents (wall-follower) are for iterating, not evidence.
2. **HELD-OUT** opponents (not wall-follower restatements) decide what ships.
3. Do not tune constants to make a held-out row look good — that converts it
   to familiar and spends it.
4. A change ships only if the bench improves: adaptive must beat naive.

Current baseline (2026-08-04): adaptive **-8.6 moves / -8.1 food** vs naive
wall-follower. The learning half is currently a net drag.

## The mission: dual live memory

rps-ai's episodes are `situation → what the HUMAN played next` — it models
the opponent. worm's `CpuEpisode` only stores the CPU's *own* move
(self-experience). The missing half:

1. **Opponent-model episodes** — `(player-centric situation → player's next
   direction)`, recorded live on player direction changes + every K frames
   of straight travel.
2. **Player-centric encoder** — player open-neighbours, wall/trail distances,
   player→food, player→CPU-threat, travel direction, recent-move TRANSITIONS
   (order matters — rps-ai's `bg`/`oc`/`ar` blocks), phase depth.
3. **Use the prediction** — avoid the player's predicted cell (survival) +
   position to intercept their predicted path (kill), confidence-weighted.
4. **Cross-game persistence** — the brain must survive game-over within a
   session (rps-ai: "what someone opens with is a habit, it is stored").
   Cold start should be global, not per-game.

## Reference implementation (read it, don't approximate it)

- `/opt/rps-ai/lib/engine.ts` — the live loop (commit → resolve → remember)
- `/opt/rps-ai/lib/predict.ts` — vote: proximity × recency × trailing-match;
  confidence = margin × support × maturity; prior blend; temperature+explore
- `/opt/rps-ai/lib/feature-embed.ts` — 57-dim deterministic encoder (why the
  coded slots, phase-depth zero-vector trap, recency weights)
- `/opt/rps-ai/lib/prior.ts` — EMA base rate + when it beats memory
- `/opt/rps-ai/README.md` — the round lifecycle and commit-reveal design

## Working rules

- Read a file before editing it. Minimal diffs. Follow existing style.
- After any cpu_ai.rs change: `cargo test --lib --tests` AND the bench.
- Constants changes must be justified by the bench, not vibes.
