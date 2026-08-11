//! SPIKE (owner report 2026-08-10: "the cpu doesn't really try to use
//! the tri shot much"): the weapon opportunity funnel, warm vs cold.
//! Prints per weapon: held-frames -> gate-pass frames -> fires -> lethal,
//! so "doesn't try" resolves into WHICH stage starves it — pickup (held
//! low), the aim gate (pass low), the unread trigger discipline (fires
//! low while cold), or lethality pricing.

use worm::game::WormGame;
use worm::Direction;

fn run(warm: bool) {
    let mut game = WormGame::with_size_seed(120, 38, 23);
    let mut fr_total = 0u64;
    let mut burned_total = 0u32;
    for g in 0..30 {
        if g > 0 {
            game.restart();
        }
        if warm {
            let lr = &mut game.cpu_brain.lifetime_read;
            lr.lat_samples = 100;
            lr.lat_hits = 90;
            lr.lat_chance = 50.0;
            lr.lat_var = 25.0;
            lr.lat_latched = true;
            game.refresh_read_rate();
        }
        let mut fr = 0u32;
        let mut burn_prev = 0u32;
        while !game.game_over && fr < 2500 {
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
            game.update();
            fr += 1;
            fr_total += 1;
            // Napalm efficiency (gemini bar): segments the CPU's fire
            // actually burned off the player this round.
            if game.burns[0].burned_by == 1 && game.burns[0].taken > burn_prev {
                burned_total += game.burns[0].taken - burn_prev;
            }
            burn_prev = game.burns[0].taken;
        }
    }
    println!("{} ({} frames over 30 rounds):", if warm { "WARM (read latched)" } else { "COLD (unread)" }, fr_total);
    let c = game.cpu_brain.trishot_class_fires;
    println!("  trishot class census: head {} · burn-trap {} · trim-to-box {}", c[0], c[1], c[2]);
    println!("  napalm attrition: {} player segments burned off by CPU fire", burned_total);
    if let Some(e) = game.cpu_brain.ledgers.tactic_attempts.iter().find(|e| e.0 == 7) {
        println!("  coil (ADR-028): attempts {} kills {}", e.3, e.4);
    } else {
        println!("  coil (ADR-028): never attempted");
    }
    for &(id, kind) in &worm::cpu_ai::WEAPON_IDS {
        let e = game
            .cpu_brain
            .ledgers
            .weapon_ops
            .iter()
            .find(|e| e.0 == id)
            .copied()
            .unwrap_or((id, 0, 0, 0, 0));
        println!(
            "  {:?}: held {} frames -> gate-pass {} -> fires {} -> lethal {}",
            kind, e.1, e.2, e.3, e.4
        );
    }
}

fn main() {
    run(false);
    run(true);
}
