//! SPIKE (owner report 2026-08-09: the CPU "just goes around and around
//! and around"): reproduce and MEASURE the self-coil — wall-follow
//! hugging the CPU's own trail and winding inward. Player scripted to
//! circle far away so no pressure/read dynamics interfere; we measure
//! how much of the CPU's life is spent adjacent to its own trail with
//! shrinking open space, and how many rounds end in self-trail deaths.

use std::collections::HashMap;
use worm::game::{DeathCause, WormGame};
use worm::Direction;

fn main() {
    let mut coil_frames = 0u64;
    let mut frames = 0u64;
    let mut self_deaths = 0u32;
    let mut cpu_losses = 0u32;
    let mut rounds = 0u32;
    let mut coil_reasons: HashMap<&'static str, u32> = HashMap::new();
    let mut game = WormGame::with_size_seed(120, 38, 9);

    for g in 0..30 {
        if g > 0 {
            game.restart();
        }
        rounds += 1;
        let mut fr = 0u32;
        let mut last_open = f32::MAX;
        let mut ring: Vec<(u16, u16, u8)> = Vec::new();
        while !game.game_over && fr < 2500 {
            // Scripted MENACE: drive at the CPU until close, then veer
            // off and lap — periodic pressure like an aggressive player,
            // pushing the CPU off its lines without killing it.
            let (px, py) = game.cycles[0].head;
            let (cx, cy) = game.cycles[1].head;
            let dist = (px as i32 - cx as i32).abs() + (py as i32 - cy as i32).abs();
            let d = if dist > 12 && fr % 200 < 120 {
                // approach phase: close on the CPU
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
            // Never step into an immediately fatal cell: keep the menace alive.
            let legal = worm::legal_options_from(&game, 0, game.cycles[0].direction);
            let d = if legal.contains(&d) { d } else { *legal.first().unwrap_or(&game.cycles[0].direction) };
            game.change_direction(d);
            game.update();
            fr += 1;
            frames += 1;
            if game.cycles[1].alive {
                let (cx, cy) = game.cycles[1].head;
                // HONEST coil metric (the Manhattan-1 version counted
                // every post-turn frame): a LOOP = the head's positions
                // over the last 28 frames stay inside a small bounding
                // box (<= 8x8) while at least 4 turns occurred — i.e.
                // it is going around inside a pocket-sized area.
                ring.push((cx, cy, game.cycles[1].direction as u8));
                if ring.len() > 28 {
                    ring.remove(0);
                }
                if ring.len() == 28 {
                    let minx = ring.iter().map(|r| r.0).min().unwrap();
                    let maxx = ring.iter().map(|r| r.0).max().unwrap();
                    let miny = ring.iter().map(|r| r.1).min().unwrap();
                    let maxy = ring.iter().map(|r| r.1).max().unwrap();
                    let turns = ring.windows(2).filter(|w| w[0].2 != w[1].2).count();
                    if maxx - minx <= 8 && maxy - miny <= 8 && turns >= 4 {
                        coil_frames += 1;
                        if let Some(d) = game.cpu_telemetry.decision.as_ref() {
                            *coil_reasons.entry(d.reason.as_str()).or_insert(0) += 1;
                        }
                    }
                }
                let _ = last_open;
                last_open = worm::count_open_space(&game, cx, cy);
            }
        }
        if game.winner == Some(0) && game.death_cause == Some(DeathCause::OwnTrail) {
            self_deaths += 1;
        }
        if game.winner == Some(0) {
            cpu_losses += 1;
            eprintln!("  cpu death: {:?} at {} frames", game.death_cause, fr);
        }
    }
    println!(
        "rounds {rounds}  frames {frames}  coil frames {coil_frames} ({:.1}%)  self-trail deaths {self_deaths}  cpu losses {cpu_losses}",
        100.0 * coil_frames as f64 / frames.max(1) as f64
    );
    let mut v: Vec<_> = coil_reasons.into_iter().collect();
    v.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    for (r, n) in v {
        println!("  coil reason: {n:5}  {r}");
    }
}
