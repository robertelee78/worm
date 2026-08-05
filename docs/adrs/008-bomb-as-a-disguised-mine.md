# ADR-008: The Bomb Becomes a Mine Disguised as Food

## Status
Implemented

## Date
2026-08-05

## Context

The product owner's verdict on the bomb was blunt: *"the bomb logic is not
great — I think we need to deeply explore better gameplay mechanics for it.
Open to ideas other than increasing radius."*

Measured, it was worse than "weak" — it was **incapable of its stated job**:

- Fuse: `BOMB_FUSE_MS / frame_delay` = **26 frames at the opening tick, 85 at
  the speed floor**.
- Escaping the Chebyshev radius of 10: **11 moves**.

So an attentive target always walked out; kill probability against anything
moving was ~0. Measured over ~150 plants: mean distance to target 7.2 cells at
plant, **15.6–17.0 at detonation**, 14% blast kills.

It also scaled backwards. A millisecond fuse converts to *more frames* as the
game speeds up, so the bomb was strongest in the opening crawl and useless by
the endgame — one item behaving like three over a round.

And it was simultaneously the most destructive thing in the game: a 21×21
blast is **441 cells, 12% of the arena**, and from 62% of rows it opened a
wall breach up to 21 cells wide. All of that was a *side effect* of a kill that
could not land. The bomb's problem was never power; it was that its real effect
was unpriced and unaimed while its nominal effect was impossible.

## Decision

### It is a mine, not a grenade

Planted, inert for `MINE_ARM_FRAMES = 8`, then it detonates the instant an
enemy head enters a `MINE_TRIGGER_CELLS = 2` ring. **You cannot wait it out.**
The only counter is routing around it, which is this game's actual skill.

The arming window does two jobs: the planter needs 3 moves to clear their own
ring, and the opponent gets a genuine dash-through — tight, but real, so a
fresh mine is a decision rather than an instant no-go zone.

### It is disguised as food

A planted mine renders **exactly as a food morsel** — same glyph, same
value-scaled size, same hue — carrying a `disguise` value rolled 1..=9 at plant
time. There is no tell, because there is nothing to tell apart: `food_glyph()`
is shared by real food and its impostor precisely so the two cannot drift, and
on the wire mines are emitted *inside the `food` array* while the `bombs` array
ships empty.

Tracking where the opponent planted theirs is the counter-play. This also hands
the opponent model something genuinely worth learning, because **"does this
human take bait?" is a habit** — which is the point of the game.

### The blast is a cross

A `BOMB_CORE_RADIUS = 2` square plus four axis arms of `BOMB_RADIUS_CELLS = 10`.
Area drops **441 → 65 cells (−85%)** with reach unchanged, so it still
threatens the board. Arms are kept at 10 deliberately: a cross is already far
less lethal than a square, and shortening the arms too would leave it
threatening nothing.

The trigger radius and the core radius are the same number on purpose — the
rule reads as *"the ring that sets it off is the ring that certainly kills
you"*. One thing to learn, not two.

`in_blast()` is a single shared predicate used by the kill test, the sweep and
the terminal preview. Three hand-written copies of a blast shape is how "I
never saw that coming" gets back in.

### The fuse survives only as hygiene

`BOMB_FUSE_FRAMES = 240`, in **frames**, replacing `BOMB_FUSE_MS`. It stops
stale mines accumulating; it is no longer the weapon. Denominating it in frames
also kills the speed inversion — it is now a fixed number of *moves* regardless
of tick rate.

### Remote detonation falls out for free

The owner is immune to the **blast** but not to the **trigger**. Walking back
onto your own mine detonates it deliberately — no new input, no new held state,
no wasm API change. It is paid for in board position, because you have to
physically return.

## Consequences

**The CPU's cheap threat check becomes correct for the first time.** Its threat
gate only ever examines the *next cell*. Against a 441-cell region that is
hopeless — you cannot route around it one step at a time. Against a radius-2
ring it is **exactly sufficient**, with no pathfinding added.

The threat check distinguishes two zones, and the distinction is load-bearing:
the **trigger ring** is entered at your peril, while the **arms** are only
lethal at the moment of detonation and are safe to cross beforehand. Treating
the whole cross as untouchable would wall the CPU out of most of the board.

`should_fire` changes character completely. It was *"player within blast
radius"* — i.e. throw it at them — which planted at the radius edge, exactly
where the target walks out. A mine is **placed**, not aimed: it now plants
where the player's projected path crosses, and only when there is room to leave
its own ring.

**Seeded streams diverge from pre-mine builds**, because rolling the disguise
consumes an RNG draw at plant time. Deliberate; any win-rate figure quoted from
before this change is void.

## Verification

`cargo test` — 112 tests pass. Nine cover the mine directly: inert while
arming, fires on an enemy head entering the ring, the owner's own mine does not
trip on them, the blast is a cross (a diagonal just outside the core survives
where the old square killed it), arms still breach the arena wall but never the
ring-0 frame, and the CPU's threat check treats the trigger ring, the arming
window and its own mines correctly.
