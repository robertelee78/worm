# worm — agent briefing

TRON light-cycle duel in Rust (crossterm terminal UI). The CPU opponent
(`src/lib/cpu_ai.rs`) is a **live learner** ported from the REAL `/opt/rps-ai`
mechanism — there is no training phase; the loop is the game:

```
every frame: every model predicts the player's next direction
      ↓
next frame: score every prediction (hit/miss × frame² — quadratic recency)
      ↓
argmax-score model drives the hunt layers; wall-follow floor vetoes traps
```

Seven specialist models (`rep/pat/frq/due/wlR/wlL/knn`), each a falsifiable
assumption about the player. The cheap ones work from frame 2; `knn` (k-NN
over player-centric contexts) is the sophisticated member and abstains cold.
Per-game scores reset on restart; the k-NN memory persists as the corpus.

## Commands

```
cargo build                          # debug build
cargo test --lib --tests             # 45 unit/integration tests
cargo bench --bench cpu_ai_bench     # THE behavioural gate (harness=false, runs main)
cargo run --release                  # play it (terminal)
./web/build.sh                       # build the browser bundle (web/pkg)
cd web && python3 -m http.server 8080  # serve the web version
```

## One engine, two frontends

The same `WormGame` (src/lib/) drives both: terminal UI (crossterm,
`src/main.rs`) and browser (WASM via `src/lib/wasm_api.rs` + `web/`,
wasm-bindgen `--target web`). All gameplay changes go in the lib — never in a
frontend. Terminal-specific code is `#[cfg(not(target_arch = "wasm32"))]`;
wasm-only code is `#[cfg(target_arch = "wasm32")]` or the `wasm` feature.
rand is target-split (full features native; `std,std_rng` only on wasm —
games on wasm are always seeded).

- **Terminal**: board = (term_w/2, term_h−2) cells, rendered 2 chars/cell
  (terminal cells are ~2:1 tall — 1-char cells made horizontal travel look
  2× vertical). Sounds = terminal bell (threaded jingles).
- **Browser**: `WormGame::with_size_seed` from JS; canvas render + CRT
  overlay in `web/`; real WebAudio chiptune SFX (`web/audio.js`, typed
  event protocol kind∈0..7 via `sfx_json`) + procedural 8-bit BGM (drop-in
  slot: `web/music.mp3` wins if present); hero art `web/assets/hero.png`.
- **Per-player brain persistence**: `CpuBrain::to_bytes/from_bytes` (bincode
  + magic). Native: `worm_brain.bin` (env `WORM_BRAIN`). Browser: IndexedDB
  keyed by a long-lived `worm_device_id` localStorage nonce — the CPU learns
  THAT player across sessions; no server needed. Aggregate layer (if ever):
  server-side SQLite of per-game summaries is sufficient.
- **Speed model**: speed is earned by eating — `frame_delay = 115ms −
  min(80, food_eaten_total/2)` (floor 35ms), `speed_pct()` for HUD/music.
  Not time-based. Food renders as value-sized orbs/glyphs (no digits).
- **Scoreboard**: `session_wins` banks at `restart()`; DISPLAY code uses
  `displayed_wins()` (banked + current winner) or the champion check fires
  a round late. Match target: first to 3 (web).
- **Death cause**: `death_cause: Option<DeathCause>` records the first
  lethal event (wall / own / enemy trail / head-on / bomb / laser / bolt)
  and both game-over screens show it ("CPU WINS — bomb blast"). Bombs are
  the only weapon that erases trails (detonate clears cells in radius).
- **Bomb telegraphing**: blast zone drawn pulsing with fuse urgency (red
  '░' smoulder on terminal Empty cells at fuse ≤ 15; full 21×21 zone +
  fuse-arc countdown on canvas). Never leave the bomb as just a dot.
- **Brain panel (web)**: plain-language CPU BRAIN side card — prediction
  glyph + confidence/sample size, source model (flashes on change), actual
  tactical action, last forecast result, projected five-cell path, 7 model
  rows with human names/forecasts/effective scores/hit rates, warm-up and
  retention state, direction habits, and separately scoped round/lifetime
  accuracy against the 25% chance floor. The arena and side panel share the
  browser viewport without overflow; below 980 px the panel stacks and the
  arena keeps its logical aspect ratio. The terminal bottom bar keeps the
  compact `BRAIN rep:… knn:*` line on ≥100-col terminals.

## The benchmark rule (load-bearing, from /opt/rps-ai/CLAUDE.md)

The bench pits the adaptive CPU against scripted opponents and scores
**survival (moves) + food**. Discipline:

1. **FAMILIAR** opponents (wall-follower) are for iterating, not evidence.
2. **HELD-OUT** opponents (not wall-follower restatements) decide what ships.
3. Do not tune constants to make a held-out row look good — that converts it
   to familiar and spends it.
4. A change ships only if the bench improves: adaptive must beat naive.

Harness caveats (found 2026-08-04, read before trusting rows):

- The bench's `food` column is `cycles[1].score` = frames survived + food
  value — dominated by survival. It is not "food eaten".
- Scripted opponents circle 2×2 blocks in open space (right turn every free
  frame) and suicide when food grows them mid-circle. Opponent death time is
  mostly seed luck.
- Scripted bench rows set `game.cpu_autopilot = false`; update() then keeps
  the external steer and records nothing. Never let update() run cpu_decide
  for a scripted row — that was the old bug and it made "naive" a
  fresh-brain adaptive in disguise.
- A fast kill lowers the adaptive row's `moves` — survival delta punishes
  winning quickly. Read wins first, deltas second.

Current baseline (2026-08-04, ensemble + behavior build, board pinned at
120×38 via with_size_seed):

- Familiar: adaptive **98/100 wins** (kills the wall-follower at ~183
  frames; naive never dies on its own).
- Held-out (chaser): adaptive **99/100 wins vs naive 0%**, survival +19.7,
  food +19.8.
- Reward signal: episodes record frames survived *on the current heading*
  (resets on turn, 0 on crash) + 10× food — verified accumulating by test.
- Opponent-model accuracy is tracked live: `cpu_brain.opp_pred_accuracy()`
  (HUD `MEM:` readout on wide terminals). A constant-direction player locks
  in at 80%+ within 5 frames (chance floor is 25%).
- Corpus retention: DECAY_TAU=1500 (~10 games), MAX_EPISODES=4000 — rps-ai's
  corpus never decays; ours used to last ~1 game.

## The mission: dual live memory — LANDED (2026-08-04)

1. **Opponent-model episodes** — `(player-centric situation → next
   direction)` recorded every frame (pre-move context, no label leakage).
2. **Player-centric encoder** — open neighbours, trail/food distances,
   CPU-threat proximity, 4×4 direction-transition matrix.
3. **Use the prediction** — the rps-ai ensemble drives avoid/intercept
   layers with hit-rate confidence; the self-survival k-NN casts one gated
   vote above the wall-follow floor.
4. **Cross-game persistence** — brain survives restart(); ensemble scores
   reset per game (rps-ai's per-game record), k-NN memory persists.
5. **Visible intelligence** — HUD brain panel (per-model scores, active
   driver, live prediction, hit-rate) on ≥100-col terminals, `MEM:` episode
   counts, session WINS scoreboard, game-over brain summary.

## Reference implementation (read it, don't approximate it)

- `/opt/rps-ai/src/model.py` — THE mechanism: 6 models (assumptions, not
  memories) + `score_model` (quadratic recency) + `computer_choice` (argmax
  ensemble). NOTE: there is no k-NN / embedding / temperature machinery in
  rps-ai, and no TypeScript sources — earlier docs here claimed otherwise.
- `/opt/rps-ai/src/game.py` — `beats`/`loses_to` + round resolution.
- `/opt/rps-ai/app.py` + `templates/play-studio.html` — per-round record
  (p1, p2, winner, model_choice, model0..5), scoreboard, history table.

## Working rules

- Read a file before editing it. Minimal diffs. Follow existing style.
- After any cpu_ai.rs change: `cargo test --lib --tests` AND the bench.
- Constants changes must be justified by the bench, not vibes.
