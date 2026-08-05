//! Does the premise actually work?
//!
//! The whole point of this game is that the CPU learns a specific human and
//! gets better at reading them. That claim is easy to make and easy to fool
//! yourself about, so it gets a test.
//!
//! Scripted personas play the game with a KNOWN habit. The CPU's forecasts are
//! scored **only on the frames where the persona exercised that habit** — the
//! frames where the answer was genuinely a choice about the player rather than
//! a fact about the board. Everything else (continuing down a corridor,
//! turning because there is only one way out) is free accuracy and is excluded.
//!
//! The suite is built to be able to FAIL and to be able to report a NEGATIVE:
//!
//!   * POSITIVE CONTROLS — habits the model provably can express. If these
//!     stop being learned, the learning pipeline has broken.
//!   * NULL CONTROLS — opponents with no habit at all. If the CPU ever reads
//!     these above chance, the test is leaking the answer and every other
//!     number here is void.
//!   * ACCEPTANCE — habits that are relative to the player's own heading
//!     ("break left when cornered"). These are the product goal and they do
//!     NOT currently pass; see the ignored test at the bottom.
//!
//! Run: `cargo test --test persona_learning -- --nocapture`

use worm::{Direction, WormGame};

/// Deterministic PRNG, independent of the game's own stream so a scripted
/// opponent can never perturb food spawns or the CPU's explore rolls.
struct Rng(u64);
impl Rng {
    fn next_f32(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
        ((self.0 >> 33) as f32) / (u32::MAX as f32 / 2.0)
    }
}

fn left_of(d: Direction) -> Direction {
    match d {
        Direction::Up => Direction::Left,
        Direction::Left => Direction::Down,
        Direction::Down => Direction::Right,
        Direction::Right => Direction::Up,
    }
}
fn right_of(d: Direction) -> Direction {
    match d {
        Direction::Up => Direction::Right,
        Direction::Right => Direction::Down,
        Direction::Down => Direction::Left,
        Direction::Left => Direction::Up,
    }
}

fn can_step(game: &WormGame, d: Direction) -> bool {
    let (hx, hy) = game.cycles[0].head;
    let (dx, dy) = d.as_delta();
    let (nx, ny) = (hx as i16 + dx, hy as i16 + dy);
    nx >= 0
        && ny >= 0
        && nx < game.width as i16
        && ny < game.height as i16
        && game.passable(nx as u16, ny as u16)
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Persona {
    /// Breaks LEFT when it must turn — a habit relative to its own heading.
    /// This is the product goal.
    Lefty,
    /// Prefers a fixed compass direction. An ABSOLUTE habit, which the model
    /// can already express. POSITIVE CONTROL.
    Compass,
    /// Picks uniformly whenever it has a real choice. NULL — there is nothing
    /// to learn and the CPU must not appear to learn it.
    Coinflip,
}

/// What the persona did, and whether it was a real exercise of its habit.
struct Move {
    dir: Direction,
    /// True when the persona had two or more ways to go and its habit — not
    /// the board — picked between them.
    habit_frame: bool,
}

fn act(p: Persona, game: &WormGame, rng: &mut Rng) -> Move {
    let cur = game.cycles[0].direction;
    let (l, r) = (left_of(cur), right_of(cur));
    let straight_open = can_step(game, cur);
    let sides: Vec<Direction> = [l, r].into_iter().filter(|&d| can_step(game, d)).collect();

    match p {
        // Hold the line while it is open; when blocked, break left 85% of the
        // time. The habit only speaks when BOTH sides are available.
        Persona::Lefty => {
            if straight_open {
                return Move { dir: cur, habit_frame: false };
            }
            if sides.len() < 2 {
                return Move {
                    dir: *sides.first().unwrap_or(&cur),
                    habit_frame: false,
                };
            }
            let pick = if rng.next_f32() < 0.85 { l } else { r };
            Move { dir: pick, habit_frame: true }
        }
        // Head Up whenever Up is available and is a genuine alternative.
        Persona::Compass => {
            let want = Direction::Up;
            let alternatives = [cur, l, r]
                .into_iter()
                .filter(|&d| can_step(game, d))
                .count();
            if can_step(game, want) && alternatives >= 2 {
                return Move { dir: want, habit_frame: true };
            }
            let fallback = [cur, l, r].into_iter().find(|&d| can_step(game, d));
            Move { dir: fallback.unwrap_or(cur), habit_frame: false }
        }
        // No habit whatsoever.
        Persona::Coinflip => {
            let opts: Vec<Direction> = [cur, l, r]
                .into_iter()
                .filter(|&d| can_step(game, d))
                .collect();
            if opts.len() < 2 {
                return Move {
                    dir: *opts.first().unwrap_or(&cur),
                    habit_frame: false,
                };
            }
            let i = (rng.next_f32() * opts.len() as f32) as usize;
            Move {
                dir: opts[i.min(opts.len() - 1)],
                habit_frame: true,
            }
        }
    }
}

struct Result {
    habit_frames: u32,
    habit_hits: u32,
    /// Uniform chance summed over the habit frames actually seen.
    expected: f32,
}

impl Result {
    fn rate(&self) -> f32 {
        self.habit_hits as f32 / self.habit_frames.max(1) as f32
    }
    fn chance(&self) -> f32 {
        self.expected / self.habit_frames.max(1) as f32
    }
    /// Standard-normal z of the read rate against uniform chance over the
    /// options the persona actually faced.
    fn z(&self) -> f32 {
        let n = self.habit_frames as f32;
        let p0 = self.chance();
        if n < 1.0 || p0 <= 0.0 || p0 >= 1.0 {
            return 0.0;
        }
        (self.rate() - p0) / (p0 * (1.0 - p0) / n).sqrt()
    }
}

/// Play `games` rounds against `persona`, carrying one brain across restarts
/// exactly as a real session does.
fn play(persona: Persona, games: u32, seed: u64) -> Result {
    let mut game = WormGame::with_size_seed(120, 38, seed);
    let mut rng = Rng(seed ^ 0x9E37_79B9);
    let mut out = Result { habit_frames: 0, habit_hits: 0, expected: 0.0 };

    for g in 0..games {
        if g > 0 {
            game.restart();
        }
        let mut frames = 0;
        while !game.game_over && frames < 4000 {
            let mv = act(persona, &game, &mut rng);
            // How many ways the persona could have gone — the honest
            // denominator for "could it have guessed?".
            let options = worm::option_count(&game, 0).max(1);

            game.change_direction(mv.dir);
            game.update();
            frames += 1;

            if mv.habit_frame {
                if let Some(scored) = game.cpu_telemetry.scored {
                    out.habit_frames += 1;
                    out.expected += 1.0 / options as f32;
                    if scored.hit {
                        out.habit_hits += 1;
                    }
                }
            }
        }
    }
    out
}

fn report(name: &str, r: &Result) {
    println!(
        "{name:<28} n={:<6} read={:>5.1}%  chance={:>5.1}%  z={:>+7.1}",
        r.habit_frames,
        r.rate() * 100.0,
        r.chance() * 100.0,
        r.z()
    );
}

/// If this fails, the learning pipeline itself is broken — not the model's
/// expressiveness. An absolute-direction habit is something it provably can
/// represent, so it is the canary for regressions in recording, persistence,
/// recall or scoring.
#[test]
fn positive_control_an_absolute_habit_is_learned() {
    let r = play(Persona::Compass, 10, 20260805);
    report("compass (POSITIVE)", &r);

    assert!(
        r.habit_frames >= 200,
        "not enough habit frames to conclude anything: {}",
        r.habit_frames
    );
    assert!(
        r.z() > 5.0,
        "an absolute habit must be read far above chance — read {:.1}% vs {:.1}% (z={:.1})",
        r.rate() * 100.0,
        r.chance() * 100.0,
        r.z()
    );
}

/// The falsifiability guard, and the most important test here.
///
/// A suite that cannot report a negative proves nothing. This opponent has no
/// habit at all, so there is nothing to learn; if the CPU reads it above
/// chance, the harness is leaking the answer and every other number in this
/// file is void.
#[test]
fn null_control_an_opponent_with_no_habit_is_not_learned() {
    let r = play(Persona::Coinflip, 10, 4242);
    report("coinflip (NULL)", &r);

    assert!(
        r.habit_frames >= 100,
        "not enough habit frames to conclude anything: {}",
        r.habit_frames
    );
    assert!(
        r.z() < 3.0,
        "a habitless opponent must NOT be readable — read {:.1}% vs {:.1}% (z={:.1}). \
         If this fires, the test is leaking the outcome into the forecast.",
        r.rate() * 100.0,
        r.chance() * 100.0,
        r.z()
    );
}

/// THE ACCEPTANCE TEST FOR THE PRODUCT GOAL — currently failing, deliberately.
///
/// "Breaks left when cornered" is a habit relative to the player's own
/// heading. The model predicts ABSOLUTE directions, so a relative habit smears
/// across all four of them and is unlearnable however much data it sees.
/// Measured: lefty reads at ~38% against a 50% baseline — not merely unlearned
/// but systematically WRONG, because the forecast defaults to the current
/// heading and a turn frame is exactly where that is guaranteed to miss.
///
/// Un-ignore this when the opponent model is retargeted to relative turns.
/// It is the definition of that work being done.
#[test]
#[ignore = "known failure: the model predicts absolute directions, so a \
            heading-relative habit is unlearnable. Acceptance test for the \
            Turn retarget."]
fn acceptance_a_relative_turn_habit_is_learned() {
    let r = play(Persona::Lefty, 40, 20260805);
    report("lefty (ACCEPTANCE)", &r);

    assert!(
        r.habit_frames >= 60,
        "not enough habit frames to conclude anything: {}",
        r.habit_frames
    );
    assert!(
        r.z() > 3.0,
        "a left-breaking habit must become readable — read {:.1}% vs {:.1}% (z={:.1})",
        r.rate() * 100.0,
        r.chance() * 100.0,
        r.z()
    );
}
