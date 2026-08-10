//! SPIKE (owner report 2026-08-10: "the cpu is kind of stupid in the
//! 1st few rounds -- runs into itself and the opponent more than
//! expected"): classify early-round CPU deaths. For every trail death,
//! measure the AGE of the killing cell (frames since it was laid):
//! a fresh cut-off (< 10 frames) is the earned Tron kill ADR-018
//! protects; a stale cell (>= 30 frames) is scenery, and the ADR's own
//! contract says the CPU must never faceplant scenery.

use std::collections::HashMap;
use worm::game::{DeathCause, WormGame};
use worm::Direction;

fn main() {
    let mut game = WormGame::with_size_seed(120, 38, 17);
    let mut deaths: Vec<(u32, u32, DeathCause, Option<u32>, usize, bool)> = Vec::new();

    for g in 0..30u32 {
        if g > 0 {
            game.restart();
        }
        let mut fr = 0u32;
        // (cell -> frame it was laid) for both worms, this round.
        let mut laid: HashMap<(u16, u16), u32> = HashMap::new();
        while !game.game_over && fr < 2500 {
            // Menace player (same shape as coil_probe): drive at the CPU
            // in pulses, otherwise lap the arena — lays trail across the
            // CPU's lines without suiciding.
            let (px, py) = game.cycles[0].head;
            let (cx, cy) = game.cycles[1].head;
            let dist = (px as i32 - cx as i32).abs() + (py as i32 - cy as i32).abs();
            let d = if dist > 12 && fr % 200 < 120 {
                if (px as i32 - cx as i32).abs() > (py as i32 - cy as i32).abs() {
                    if px > cx { Direction::Left } else { Direction::Right }
                } else if py > cy {
                    Direction::Up
                } else {
                    Direction::Down
                }
            } else if py <= 6 && px < 110 {
                Direction::Right
            } else if px >= 110 && py < 32 {
                Direction::Down
            } else if py >= 32 && px > 10 {
                Direction::Left
            } else {
                Direction::Up
            };
            let legal = worm::legal_options_from(&game, 0, game.cycles[0].direction);
            let d = if legal.contains(&d) { d } else { *legal.first().unwrap_or(&game.cycles[0].direction) };
            game.change_direction(d);
            for i in 0..2 {
                if game.cycles[i].alive {
                    laid.insert(game.cycles[i].head, game.frame_count);
                }
            }
            game.update();
            fr += 1;
        }
        if game.winner == Some(0) {
            // CPU died: age of the cell it died in, if it was a trail —
            // plus the codex audit contract: how many survivable options
            // it had at the moment of death (0 = enclosure, not blindness).
            let cell = game.cycles[1].head;
            let age = laid.get(&cell).map(|&f| game.frame_count.saturating_sub(f));
            let opts =
                worm::legal_options_from(&game, 1, game.cycles[1].direction).len();
            let decided = game.last_cpu_decision_frame == game.frame_count;
            deaths.push((g + 1, fr, game.death_cause.unwrap(), age, opts, decided));
        }
    }

    println!("round  frames  cause                 cell-age");
    let mut fresh = 0u32;
    let mut stale = 0u32;
    for (r, fr, cause, age, opts, decided) in &deaths {
        let a = age.map(|a| a.to_string()).unwrap_or_else(|| "-".into());
        let c = format!("{cause:?}");
        println!("{r:5}  {fr:6}  {c:<20}  {a:<8}  opts {opts}  decided {decided}");
        if matches!(cause, DeathCause::EnemyTrail | DeathCause::OwnTrail) {
            match age {
                Some(a) if *a < 10 => fresh += 1,
                Some(_) => stale += 1,
                None => {}
            }
        }
    }
    let early: Vec<_> = deaths.iter().filter(|d| d.1 < 100).collect();
    println!(
        "\ncpu deaths {}  under-100-frame deaths {}  trail deaths fresh(<10f) {fresh}  stale(>=10f) {stale}",
        deaths.len(),
        early.len()
    );
}
