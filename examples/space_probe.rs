//! Space-game spike (task #13, kata step 2): measure the SUPPLY of
//! cut-based play in a real corpus before designing any of it.
//!
//! A "cuttable moment": the player's head sits in a region whose
//! connection to the arena's largest open area passes through a narrow
//! throat (≤2 corridor cells within radius 12). If these are frequent
//! and the CPU rarely converts them, the space game has headroom; if
//! they're rare, it doesn't.
use std::collections::VecDeque;
use worm::{CellType, WormGame};

fn open_cell(game: &WormGame, x: i16, y: i16) -> bool {
    x >= 0
        && y >= 0
        && x < game.width as i16
        && y < game.height as i16
        && matches!(
            game.grid[y as usize][x as usize],
            CellType::Empty | CellType::Food | CellType::Hole | CellType::PowerUp
        )
}

/// Corridor cell: open, with exactly two open 4-neighbors, opposite each
/// other (a 1-wide throat).
fn is_throat(game: &WormGame, x: i16, y: i16) -> bool {
    if !open_cell(game, x, y) {
        return false;
    }
    let n = open_cell(game, x, y - 1);
    let s = open_cell(game, x, y + 1);
    let e = open_cell(game, x + 1, y);
    let w = open_cell(game, x - 1, y);
    (n && s && !e && !w) || (e && w && !n && !s)
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: space_probe <player.json>");
    let text = std::fs::read_to_string(&path).expect("read");
    let v: serde_json::Value = serde_json::from_str(&text).expect("json");
    let mut rounds: Vec<&serde_json::Value> =
        v["rounds"].as_array().expect("rounds").iter().collect();
    rounds.sort_by_key(|r| r["endedAt"].as_u64().unwrap_or(0));

    let mut game = WormGame::with_size_seed(55, 40, 1);
    game.cpu_autopilot = false;
    game.shadow_learning = true;

    let mut frames_total = 0u64;
    let mut cuttable_frames = 0u64; // player behind a narrow throat
    let mut cuttable_moments = 0u32; // distinct entries into that state
    let mut converted = 0u32; // player died within 30f of a moment
    let mut cpu_pocket_frames = 0u64; // CPU itself behind a throat
    let mut cpu_pocket_deaths = 0u32;

    for rec in rounds.iter() {
        let Some(replay) = rec.get("replay") else { continue };
        if replay["v"].as_u64() != Some(2) {
            continue;
        }
        let seed: u64 = replay["seed"].as_str().unwrap().parse().unwrap();
        let w = replay["w"].as_u64().unwrap() as u16;
        let h = replay["h"].as_u64().unwrap() as u16;
        let arena = replay.get("arena").and_then(|x| x.as_u64()).unwrap_or(1) as u8;
        let frames = replay["frames"].as_u64().unwrap() as u32;
        let events: Vec<(u32, u8, u8)> = replay["ev"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                let t = e.as_array().unwrap();
                (
                    t[0].as_u64().unwrap() as u32,
                    t[1].as_u64().unwrap() as u8,
                    t[2].as_u64().unwrap() as u8,
                )
            })
            .collect();
        game.start_recorded_round(seed, w, h, arena, events);
        let mut in_cuttable = false;
        let mut last_moment_frame = 0u32;
        let mut cpu_in_pocket = false;
        while !game.game_over && game.frame_count <= frames {
            game.update();
            frames_total += 1;
            // Player side: BFS from head, region capped at 200 cells; count
            // throat cells on the region boundary within radius 12.
            for (who, pocket_flag, pocket_frames, moments) in [
                (0usize, &mut in_cuttable, &mut cuttable_frames, true),
                (1usize, &mut cpu_in_pocket, &mut cpu_pocket_frames, false),
            ]
            .into_iter()
            {
                let (hx, hy) = game.cycles[who].head;
                let mut seen = std::collections::HashSet::new();
                let mut q = VecDeque::new();
                q.push_back((hx as i16, hy as i16));
                seen.insert((hx as i16, hy as i16));
                let mut region = 0u32;
                let mut throats = 0u32;
                let mut big = false;
                while let Some((x, y)) = q.pop_front() {
                    region += 1;
                    if region > 200 {
                        big = true;
                        break;
                    }
                    for (dx, dy) in [(0i16, 1i16), (0, -1), (1, 0), (-1, 0)] {
                        let (nx, ny) = (x + dx, y + dy);
                        if seen.contains(&(nx, ny)) || !open_cell(&game, nx, ny) {
                            continue;
                        }
                        if is_throat(&game, nx, ny)
                            && (nx - hx as i16).abs() + (ny - hy as i16).abs() <= 12
                        {
                            throats += 1;
                            continue; // don't expand past throats
                        }
                        seen.insert((nx, ny));
                        q.push_back((nx, ny));
                    }
                }
                let pocketed = !big && region <= 120 && throats >= 1 && throats <= 2;
                if pocketed {
                    *pocket_frames += 1;
                    if !*pocket_flag && moments {
                        cuttable_moments += 1;
                        last_moment_frame = game.frame_count;
                    }
                }
                *pocket_flag = pocketed;
            }
        }
        // Round outcome vs the last cuttable moment.
        let winner = rec["winner"].as_u64();
        if winner == Some(1) && in_cuttable
            || (winner == Some(1)
                && game.frame_count.saturating_sub(last_moment_frame) <= 30
                && last_moment_frame > 0)
        {
            converted += 1;
        }
        if winner == Some(0) && cpu_in_pocket {
            cpu_pocket_deaths += 1;
        }
    }

    println!("== SPACE-GAME SUPPLY (owner corpus) ==");
    println!(
        "  player cuttable: {} distinct moments, {} frames ({:.1}% of {})",
        cuttable_moments,
        cuttable_frames,
        100.0 * cuttable_frames as f64 / frames_total as f64,
        frames_total
    );
    println!(
        "  CPU converted (player died ≤30f after a moment): {} of {} moments ({:.0}%)",
        converted,
        cuttable_moments,
        100.0 * converted as f64 / cuttable_moments.max(1) as f64
    );
    println!(
        "  CPU self-pocketed: {} frames; {} of its deaths ended pocketed",
        cpu_pocket_frames, cpu_pocket_deaths
    );
}
