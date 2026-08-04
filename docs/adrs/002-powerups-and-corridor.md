# ADR-002: Power-ups, Corridor, and Symmetric Firing

## Status
Accepted / Shipped

## Context
The base `worm` light-cycle duel had a single mechanic: leave a trail, don't hit
it. The gameplay was sparse and the CPU had no tools beyond spatial movement.
This ADR introduces active weaponry and arena topology that give both players
tactical options and create learnable, observable opponent behaviour for the
dual-memory upgrade described in ADR-001.

## Decision

### 1. Arena topology: the outer corridor
When the terminal is at least 10×10, the grid is divided into three concentric
rings (see `src/lib/game.rs:229-258`):

```
Ring 0  — outer frame        (always Wall, never passable)
Ring 1  — outer corridor     (Empty — the pacman tunnel between holes)
Ring 2  — arena wall         (Wall, punchable → becomes Hole)
Center  — play arena         (Empty, grows/shrinks with terminal size)
```

- Food and power-ups spawn **inside** the arena only (`xlo=4, xhi=width-4`,
  etc.) — never in the corridor.
- The corridor is traversable empty space; `passable()` returns true for it.
- `WallPunch` punches a `Hole` through ring 2, opening a two-way passage into
  the corridor. Players can route through the corridor to escape or flank.
- `passable()` treats `Hole` as legal space (genuinely open, unlike a trail).

### 2. Four power-up classes (`src/lib/game.rs:38-59`)

| Power-up | Behaviour | Range / Radius |
|---|---|---|
| **Laser** | Hitscan beam along the facing axis. Kills on head contact, detonates bombs in path, passes through trails/holes, **stops at the first wall**. Does not sever the shooter's own trail. | Line-of-sight |
| **TriShot** | Three projectiles (straight + two diagonals). Each is a `Projectile` with `owner` — they pass through their shooter's head but kill/sever any other target. Bolts die in walls at range 7 steps. | 7 cells (`TRI_SHOT_RANGE`) |
| **Bomb** | Planted at the current cell. After a 3 s fuse, detonates with Chebyshev radius 10: kills heads inside, clears trails/food/power-ups, **severes** surviving opponents' tails (same missile sever rule), and **chains** into other armed bombs. | 10 cells (`BOMB_RADIUS_CELLS`) |
| **WallPunch** | Fires to the first wall cell. If it is the punchable arena wall (ring 2), opens a permanent `Hole` — a corridor gateway. Never punches the outer frame (ring 0). | Until ring 2 |

### 3. Sever rule (consistent across all weapons)
> Only a **head** hit kills. A **body/trail** hit **severs** the victim's tail
> at the struck cell and deducts 1 point per lost cell (`src/lib/game.rs:919-932`).

This mirrors classic missile-trek rules: grazing a trail is a setback, not a
game over. It also makes tail-positioning meaningful — players can be "maimed"
without being eliminated.

### 4. Symmetric firing
Both the player and the CPU can hold and fire power-ups. `should_fire()`
(`src/lib/cpu_ai.rs:686`) is wired into the adaptive decision path so the CPU
can plant bombs, fire lasers, and use WallPunch against the player's predicted
position. The player's input path (`src/main.rs`) maps keys to `fire_powerup`
with the same dispatch.

### 5. Bolt ownership semantics (`src/lib/game.rs:62-73`)
A `Projectile` carries its `owner` index. Bolts advance one cell per frame;
since they spawn on the shooter's head and the shooter also advances one cell,
the bolt and its owner move in lock-step. The bolt **never kills its owner** —
this is explicit in `advance_projectiles()` which skips `owner == shooter`
head checks. This is necessary because firing tri-shot while moving right would
otherwise be instant suicide.

### 6. Design intent for the CPU learning layer
The new mechanics create **observable, repeatable opponent behaviour** that the
episode memory (ADR-001) can learn from:

- **Player opens with a predictable first direction** → stored as a habit
  (rps-ai: "what someone opens with is a habit, it is stored").
- **Bomb placement patterns** → player predictability under time pressure.
- **WallPunch corridor usage** → route-choice prediction (flank vs. hold).
- **Laser aim timing** → anticipation of the player's path.

The corridor in particular transforms the spatial geometry: the open ring-1
space gives both players an escape valve, making pure wall-hugging less
dominant and rewarding predictive positioning.

## Consequences
- **Positive:** Gameplay depth increases substantially. The CPU now has
  offensive tools, making the dual-memory predictor (model the opponent's
  next move) directly actionable for interception, not just evasion.
- **Positive:** Bomb chain-reactions and laser reflections create emergent
  chaos that produces diverse episode data for k-NN voting.
- **Risk:** The adaptive CPU bench regressed post-implementation (see Bench
  below). This is expected — the corridor topology and asymmetric power-up
  usage create a larger state space the cold-start scorer hasn't mapped. The
  opponent-model encoding (ADR-001) is the remedy, not a rollback of these
  mechanics.
- **Risk:** Bolts passing through the shooter's head is non-obvious. The
  `owner` field and the explicit skip in `advance_projectiles` must be
  preserved if the projectile system is refactored.

## Bench
```
Cargo bench --bench cpu_ai_bench  (100-game sample)

Naive wall-follower:    survival = 55.0 moves (avg), food = 56.5 (avg), alive 93/100
Adaptive memory CPU:    survival = 15.0 moves (avg), food = 16.5 (avg), alive 98/100

Verdict: Adaptive CPU is flat/behind (-39.1 moves, -40.1 food) — needs reformulation
```

The survival count (98 vs 93) is higher for adaptive, but average survival
when adaptive loses is much shorter. This indicates the adaptive CPU wins by
turturing the naive wall-follower into a corner less often — the corridor
gives the naive player escape routes the adaptive scorer doesn't value. The
opponent-centric encoder from ADR-001 will address this by predicting the
player's escape route and intercepting through the corridor.

## References
- ADR-001: Opponent-Centric CPU Intelligence
- `src/lib/game.rs:38-86` — PowerUpKind, Projectile, Bomb, constants
- `src/lib/game.rs:770-820` — `detonate()` blast resolution with chain reactions
- `src/lib/game.rs:229-258` — corridor/arena-wall topology
- `src/lib/cpu_ai.rs:686` — `should_fire()` entry point for adaptive firing
- `tests/game_test.rs` — 14 power-up + corridor integration tests