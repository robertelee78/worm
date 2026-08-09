//! SPIKE (task #24, pre-consult): do boxer / breach / slip-run tactics have
//! real opportunity in the frozen human corpus, or would new bandit arms
//! starve? Pure measurement over recorded rounds — no behavior change, no
//! learner. Numbers feed the k3/codex consult and the ADR.
//!
//! Questions (pre-registered):
//!  S1 boxer opportunity: on what fraction of frames does the CPU hold a
//!     space+length advantage with the player constrainable (three
//!     threshold tiers)? What fraction of rounds contain >=1 tight window?
//!  S2 accidental-boxing kill link: of player Wall/OwnTrail deaths, how
//!     many had a mid-tier boxer window open in the final 10 frames
//!     (i.e., boxing already kills by accident -> attribution base rate)?
//!  S3 breach/slip headroom: CPU laser-held frames split by enveloped
//!     (current breach gate) vs not (the offensive headroom the owner
//!     wants); corridor occupancy by either worm; holes punched per round.
//!
//! Usage: cargo run --release --example tactic_opportunity_spike -- /opt/worm/data/rounds

use worm::cpu_ai::{count_open_space, legal_directions, legal_options_from};
use worm::game::{CellType, DeathCause, PowerUpKind, WormGame};

fn main() {
    let dir = std::env::args().nth(1).unwrap_or_else(|| "/opt/worm/data/rounds".into());

    // ---- corpus load: same dedup + ordering discipline as corpus_shootout ----
    let mut rows: Vec<(u64, String, serde_json::Value)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for entry in std::fs::read_dir(&dir).expect("rounds dir") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        for line in text.lines() {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
            let Some(rep) = v.get("replay") else { continue };
            let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let ended = v.get("endedAt").and_then(|x| x.as_u64()).unwrap_or(0);
            let dedup = format!(
                "{}:{}",
                rep.get("seed").and_then(|s| s.as_str()).unwrap_or(""),
                rep.get("ev").and_then(|e| e.as_array()).map(|a| a.len()).unwrap_or(0)
            );
            if seen.insert(dedup) {
                rows.push((ended, id, v));
            }
        }
    }
    rows.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    eprintln!("corpus: {} deduplicated replayable rounds", rows.len());

    // ---- tallies ----
    let mut frames_total = 0u64;
    let mut loose = 0u64;
    let mut mid = 0u64;
    let mut tight = 0u64;
    let mut rounds_with_tight = 0u32;
    let mut rounds_used = 0u32;
    // S2: player Wall/OwnTrail deaths x mid-window-in-last-10-frames
    let mut p_box_deaths = 0u32;
    let mut p_box_deaths_windowed = 0u32;
    // S3
    let mut cpu_laser_frames = 0u64;
    let mut cpu_laser_enveloped = 0u64;
    let mut corridor_frames = [0u64; 2];
    let mut rounds_with_hole = 0u32;
    let mut holes_total = 0u64;

    for (_, _, v) in rows.iter() {
        let rep = &v["replay"];
        let (Some(seed), Some(w), Some(h), Some(fr)) = (
            rep.get("seed").and_then(|s| s.as_str()).and_then(|s| s.parse::<u64>().ok()),
            rep.get("w").and_then(|x| x.as_u64()),
            rep.get("h").and_then(|x| x.as_u64()),
            rep.get("frames").and_then(|x| x.as_u64()),
        ) else {
            continue;
        };
        if !(10..=400).contains(&w) || !(10..=400).contains(&h) || fr > 100_000 {
            continue;
        }
        let arena = rep.get("arena").and_then(|a| a.as_u64()).unwrap_or(1) as u8;
        if arena == 0 || arena > worm::ARENA_VERSION {
            continue;
        }
        let events: Vec<(u32, u8, u8)> = rep["ev"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|e| {
                Some((
                    e.get(0)?.as_u64()? as u32,
                    e.get(1)?.as_u64()? as u8,
                    e.get(2)?.as_u64()? as u8,
                ))
            })
            .collect();
        let mut game = WormGame::with_size_seed(w as u16, h as u16, 1);
        game.start_recorded_round(seed, w as u16, h as u16, arena, events);
        game.shadow_learning = true;
        rounds_used += 1;

        let mut round_tight = false;
        let mut mid_recent: std::collections::VecDeque<bool> = std::collections::VecDeque::new();
        let mut round_holes_max = 0u64;
        let mut steps = 0u32;
        while !game.game_over && game.frame_count < fr as u32 + 4 && steps < 110_000 {
            steps += 1;
            frames_total += 1;

            if game.cycles[0].alive && game.cycles[1].alive {
                // Best reachable open space from each worm's legal next cells
                // (a worm's own head cell is occupied; its options are what
                // count). No cap — count_open_space flows through holes.
                let space_of = |who: usize| -> f32 {
                    let heading = game.cycles[who].prev_direction;
                    let opts = if who == 0 {
                        legal_options_from(&game, 0, heading)
                    } else {
                        legal_directions(&game, &game.cycles[who])
                    };
                    opts.iter()
                        .map(|d| {
                            let (dx, dy) = d.as_delta();
                            let (hx, hy) = game.cycles[who].head;
                            let nx = hx as i32 + dx as i32;
                            let ny = hy as i32 + dy as i32;
                            if nx < 0 || ny < 0 {
                                0.0
                            } else {
                                count_open_space(&game, nx as u16, ny as u16)
                            }
                        })
                        .fold(0.0f32, f32::max)
                };
                let pspace = space_of(0);
                let cspace = space_of(1);
                let p_len = game.cycles[0].positions.len();
                let c_len = game.cycles[1].positions.len();
                let (px, py) = game.cycles[0].head;
                let (cx, cy) = game.cycles[1].head;
                let dist =
                    (px as i32 - cx as i32).abs() + (py as i32 - cy as i32).abs();

                let is_loose = c_len >= p_len && cspace >= 1.2 * pspace.max(1.0);
                let is_mid = is_loose && pspace < 900.0 && dist <= 14;
                let is_tight = is_mid && pspace < 400.0;
                loose += is_loose as u64;
                mid += is_mid as u64;
                tight += is_tight as u64;
                round_tight |= is_tight;
                mid_recent.push_back(is_mid);
                if mid_recent.len() > 10 {
                    mid_recent.pop_front();
                }

                if game.cycles[1].held_powerup == Some(PowerUpKind::Laser) {
                    cpu_laser_frames += 1;
                    cpu_laser_enveloped += game.cpu_enveloped() as u64;
                }
                for (who, cf) in corridor_frames.iter_mut().enumerate() {
                    *cf += game.cycle_in_corridor(who) as u64;
                }
            }

            game.update();

            if steps.is_multiple_of(50) {
                let holes = game
                    .grid
                    .iter()
                    .flatten()
                    .filter(|c| matches!(c, CellType::Hole))
                    .count() as u64;
                round_holes_max = round_holes_max.max(holes);
            }
        }
        // Round end: S2 attribution base rate.
        let player_died = !game.cycles[0].alive;
        if player_died
            && matches!(game.death_cause, Some(DeathCause::Wall) | Some(DeathCause::OwnTrail))
        {
            p_box_deaths += 1;
            if mid_recent.iter().any(|&b| b) {
                p_box_deaths_windowed += 1;
            }
        }
        rounds_with_tight += round_tight as u32;
        let end_holes = game
            .grid
            .iter()
            .flatten()
            .filter(|c| matches!(c, CellType::Hole))
            .count() as u64;
        round_holes_max = round_holes_max.max(end_holes);
        holes_total += round_holes_max;
        rounds_with_hole += (round_holes_max > 0) as u32;
    }

    let pct = |n: u64| 100.0 * n as f64 / frames_total.max(1) as f64;
    println!("== tactic opportunity spike (task #24) ==");
    println!("rounds replayed: {}   frames: {}", rounds_used, frames_total);
    println!(
        "S1 boxer windows: loose {:.2}%  mid {:.2}%  tight {:.2}%  (of all frames)",
        pct(loose),
        pct(mid),
        pct(tight)
    );
    println!(
        "S1 rounds with >=1 tight window: {}/{} ({:.1}%)",
        rounds_with_tight,
        rounds_used,
        100.0 * rounds_with_tight as f64 / rounds_used.max(1) as f64
    );
    println!(
        "S2 player Wall/OwnTrail deaths: {}  with mid-window in last 10 frames: {} ({:.1}%)",
        p_box_deaths,
        p_box_deaths_windowed,
        100.0 * p_box_deaths_windowed as f64 / p_box_deaths.max(1) as f64
    );
    println!(
        "S3 cpu laser-held frames: {}  enveloped (current breach gate): {} ({:.1}%)  offensive headroom: {}",
        cpu_laser_frames,
        cpu_laser_enveloped,
        100.0 * cpu_laser_enveloped as f64 / cpu_laser_frames.max(1) as f64,
        cpu_laser_frames - cpu_laser_enveloped
    );
    println!(
        "S3 corridor frames: player {}  cpu {}   rounds with >=1 hole: {}/{}  holes(max)/round avg: {:.2}",
        corridor_frames[0],
        corridor_frames[1],
        rounds_with_hole,
        rounds_used,
        holes_total as f64 / rounds_used.max(1) as f64
    );
}
