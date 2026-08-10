//! The NOVICE fixture (ADR-018): does a casual first-time human stand a
//! chance against the unread CPU? Visitor feedback says no — "the other
//! snake is too good" — and the PM contract says game one must be a
//! solid-basics opponent that hasn't read you, with dominance EARNED.
//!
//!     cargo run --release --example novice_probe -- [games] [seed]
//!
//! The novice models a casual human, deliberately imperfect:
//!   * reacts only every few frames (attention, not tick-perfect);
//!   * steers greedily toward the nearest food with no route planning;
//!   * avoids walls/trails only one cell ahead (no flood fill);
//!   * occasionally dithers (random legal turn).
//!
//! Runs on the browser board with a COLD brain each game (a first-time
//! visitor has taught the CPU nothing).

use worm::{Direction, WormGame};

struct Rng(u64);
impl Rng {
    fn next_f32(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
        ((self.0 >> 33) as f32) / (u32::MAX as f32 / 2.0)
    }
}

fn novice(game: &WormGame, rng: &mut Rng, tick: u32) -> Option<Direction> {
    // Attention: a casual player adjusts course a few times a second.
    if !tick.is_multiple_of(5) {
        return None;
    }
    let cur = game.cycles[0].direction;
    let legal = worm::legal_options_from(game, 0, cur);
    if legal.is_empty() {
        return None;
    }
    // Dither sometimes — casual hands are not deliberate every beat.
    if rng.next_f32() < 0.10 {
        let i = (rng.next_f32() * legal.len() as f32) as usize;
        return Some(legal[i.min(legal.len() - 1)]);
    }
    // Greedy: head toward the nearest food, no planning.
    let (px, py) = game.cycles[0].head;
    let target = game
        .food_items
        .iter()
        .min_by_key(|&&(x, y, _)| (x as i32 - px as i32).abs() + (y as i32 - py as i32).abs());
    if let Some(&(fx, fy, _)) = target {
        let mut prefs = Vec::new();
        if fx < px { prefs.push(Direction::Left) }
        if fx > px { prefs.push(Direction::Right) }
        if fy < py { prefs.push(Direction::Up) }
        if fy > py { prefs.push(Direction::Down) }
        for d in prefs {
            if legal.contains(&d) {
                return Some(d);
            }
        }
    }
    // Otherwise keep going if possible; turn somewhere legal if not.
    if legal.contains(&cur) {
        None
    } else {
        Some(legal[0])
    }
}

fn main() {
    let games: u32 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(40);
    let seed: u64 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(20260806);
    let mut game = WormGame::with_size_seed(55, 40, seed);
    let mut rng = Rng(seed ^ 0xBEEF);
    let (mut cpu_w, mut nov_w, mut draw) = (0u32, 0u32, 0u32);
    let mut arc = String::new();
    let mut nov_frames = 0u64;
    let mut novice_deaths: std::collections::HashMap<String, u32> =
        std::collections::HashMap::new();
    for g in 0..games {
        if g > 0 {
            game.restart();
        }
        // FIRST-TIME VISITOR: cold wipes per game. Pass "warm" as arg 3 to
        // keep one persistent brain — the full learning arc instead.
        if std::env::args().nth(3).as_deref() != Some("warm") {
            game.cpu_brain = worm::CpuBrain::new();
            game.refresh_read_rate();
        }
        let mut tick = 0u32;
        while !game.game_over && game.frame_count < 4000 {
            tick += 1;
            if let Some(d) = novice(&game, &mut rng, tick) {
                game.change_direction(d);
            }
            game.update();
        }
        nov_frames += game.frame_count as u64;
        match game.winner {
            Some(1) => {
                cpu_w += 1;
                arc.push('C');
                *novice_deaths
                    .entry(format!("{:?}", game.death_cause))
                    .or_insert(0u32) += 1;
            }
            Some(0) => { nov_w += 1; arc.push('n'); }
            _ => { draw += 1; arc.push('·'); }
        }
    }
    println!("arc: {} (read {:.2})", arc, game.read_rate);
    let mut v: Vec<_> = novice_deaths.into_iter().collect();
    v.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    for (cause, n) in v {
        println!("  novice death: {n:3}  {cause}");
    }
    println!(
        "NOVICE vs unread CPU: novice {} · cpu {} · draw {}  (novice wins {:.0}%)  mean round {} frames",
        nov_w,
        cpu_w,
        draw,
        nov_w as f32 / games as f32 * 100.0,
        nov_frames / games as u64,
    );
}
