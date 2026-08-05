# ADR-009: Learning Must Convert Into Winning

## Status
Implemented

## Date
2026-08-05

## Context

The product owner restated the objective in terms that admit no ambiguity:

> our success here is measured by the computer being able to dominate any human
> player based on the human player's behavior and gameplay — if the CPU can't
> learn the player and dominate the player we have failed at our core objective

The persona suite (ADR-006) proved the CPU **reads** a player. That is not the
claim. The claim is that reading them makes it **beat** them, and a read that
never converts into a win is a statistic, not a game.

Win rate alone cannot show this: the CPU beats scripted opponents ~90% on
survival heuristics alone, so a high number proves nothing about learning. The
only way to isolate the effect is to hold opponent, board and seeds fixed and
vary **only whether the CPU may remember**.

### The experiment said we had failed

```
COLD (cannot learn)  cpu 30 player 0 draw 0  win 100%  lift  6%
WARM (remembers you) cpu 26 player 1 draw 3  win  87%  lift 53%
```

**The CPU that remembered the player won LESS than the one that could not
learn.** Learning was actively making it worse.

Two flaws in the experiment were found and fixed before trusting that, and
both mattered:

1. The cold arm rebuilt the game from the same seed each round, replaying the
   **same board 30 times** while the warm arm got varied boards. That compared
   memory *and* board variety at once. Corrected — both arms now `restart()`
   identically and the cold arm merely wipes the brain. The result survived:
   COLD 97%, WARM 87%.
2. The opponent was too weak. Once the fixes below landed, **both arms won
   ~100%** — a ceiling effect. You cannot measure "memory helps it win"
   against an opponent it already beats every time without learning. The
   persona is now survival-competent (it will not enter a pocket it cannot
   leave, using the same flood fill the CPU uses) so the CPU actually needs
   the read.

## Decision

### The projection assumed the opposite of what it had learned

`predict_player_positions_iterative` fell back to `right_turn(pdir)` at any
corner where the ensemble's prediction was blocked — *"assume the canonical
right-hand turn"*.

So the CPU would learn "this player breaks LEFT", and then its own trajectory
projection would assume they turn right. The intercept layers drove to the
wrong side of the board, and **the more confident the read, the harder it
committed to the wrong place**. That is the mechanism by which learning made
it worse. The fallback now consults the player's learned turn prior.

### The turn prior was never persisted

`PlayerBrain::turn_tally` — the one statistic that makes a heading-relative
habit learnable at all — was written to **no section**. `SEC_OPP_CORE` carries
seq, the absolute tally and the prediction counts, and nothing else. The habit
survived within a session (the brain object persists across `restart()`) and
was silently forgotten between them.

For a game whose entire premise is *"it remembers you"*, dropping the one thing
that makes a human's habit expressible is the worst possible thing to lose. It
now has its own section (`SEC_TURN_PRIOR`), classified with the other
encoding-independent knowledge about the human.

## Consequences

Against a competent habitual opponent, identical boards and seeds, varying only
memory:

| | record | win rate | lift |
|---|---|---|---|
| **COLD** — cannot learn | 27–3–0 | 90% | 0% |
| **WARM** — remembers you | 29–1–0 | **97%** | 56% |

Memory now **converts**: +7 points of win rate, and player wins fall from 3 to
1. The cold arm's lift is genuinely 0% — it cannot learn — which is what makes
the comparison meaningful rather than a restatement.

Separately, against the same opponent over 40 games the warm CPU records 38–2
(95%) at 63% lift.

`tests/domination.rs` holds both as regression guards: `learning_converts_into_winning`
asserts the warm arm never wins *less* than the cold one, and
`a_learned_habitual_player_is_dominated` asserts a read player is dominated
rather than merely beaten.

### What this does not yet prove

The opponent is a script with one habit, not a human with many. The null
control in the persona suite still holds (a habitless opponent remains
unreadable), so the effect is not an artefact — but "dominates **any** human"
is a stronger claim than "dominates a competent single-habit script", and the
gap between them is contextual features: the model still holds one global turn
prior, so it learns *"breaks left"* and not *"breaks left when the wall is
three cells away and the CPU is behind them"*.

## Verification

`cargo test` — 117 tests pass, including the persona controls and both
domination tests.
