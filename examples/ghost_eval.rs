//! Ghost evaluator (ADR-016): score the CURRENT brain against a REAL
//! player's recorded rounds.
//!
//!     cargo run --release --example ghost_eval -- worm-rounds.json
//!
//! Input: the browser's "EXPORT MY ROUNDS" download. Every round that
//! carries a ghost log is replayed bit-for-bit (both worms driven from the
//! log, `shadow_learning` on), so the brain under evaluation watches the
//! recorded human exactly as it would have watched them live — same
//! learning, same sealed forecasts, same scoring — while never steering.
//! One persistent brain across all rounds, chronologically: the output IS
//! the learning curve this codebase would have had against this human.
//!
//! This closes the loop ADR-013 left open: candidate CPUs are no longer
//! measured only against scripted personas but against the one opponent the
//! product is actually about. Tune with WORM_TUNE_* env for candidates.

use worm::{Direction, WormGame};

fn dir_of(d: u8) -> Direction {
    match d {
        0 => Direction::Up,
        1 => Direction::Down,
        2 => Direction::Left,
        _ => Direction::Right,
    }
}

/// Minimal JSON scraping — the export format is ours, flat, and versioned.
fn field_u64(obj: &str, key: &str) -> Option<u64> {
    let pat = format!("\"{}\":", key);
    let i = obj.find(&pat)? + pat.len();
    let rest = &obj[i..];
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

fn pairs_u32(obj: &str, key: &str) -> Vec<Vec<u32>> {
    let pat = format!("\"{}\":[", key);
    let Some(i) = obj.find(&pat) else {
        return Vec::new();
    };
    let rest = &obj[i + pat.len()..];
    // The array ends at the first ']' not inside a nested '[' pair-array.
    let mut depth = 1;
    let mut end = 0;
    for (j, c) in rest.char_indices() {
        match c {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    end = j;
                    break;
                }
            }
            _ => {}
        }
    }
    rest[..end]
        .split('[')
        .filter(|s| !s.trim().is_empty())
        .map(|s| {
            s.trim_end_matches(|c| c == ']' || c == ',')
                .split(',')
                .filter_map(|n| n.trim().parse().ok())
                .collect()
        })
        .collect()
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: ghost_eval <worm-rounds.json>");
    let text = std::fs::read_to_string(&path).expect("read export file");

    // Split into round objects on the replay marker; ordering in the export
    // is newest-first, so reverse into play order for an honest curve.
    let mut replays: Vec<(u64, u16, u16, Vec<Vec<u32>>, Vec<Vec<u32>>, u64)> = Vec::new();
    for chunk in text.split("\"replay\":").skip(1) {
        let Some(seed) = field_u64(chunk, "seed") else {
            continue;
        };
        let (Some(w), Some(h)) = (field_u64(chunk, "w"), field_u64(chunk, "h")) else {
            continue;
        };
        // endedAt appears BEFORE replay in each record; grab it from the
        // preceding chunk boundary is fragile — sort key falls back to file
        // order when absent, which the reverse below already handles.
        let ended = field_u64(chunk, "endedAt").unwrap_or(0);
        replays.push((
            seed,
            w as u16,
            h as u16,
            pairs_u32(chunk, "dirs"),
            pairs_u32(chunk, "fires"),
            ended,
        ));
    }
    replays.reverse(); // export is newest-first → replay in play order
    if replays.is_empty() {
        eprintln!("no rounds with ghost logs in {path} — play some rounds on the v9+ build first");
        std::process::exit(1);
    }
    println!("{} recorded round(s) — replaying chronologically…\n", replays.len());

    // ONE persistent brain across all rounds, exactly like a live session.
    let mut game = WormGame::with_size_seed(55, 40, 1);
    game.cpu_autopilot = false;
    game.shadow_learning = true;

    let mut round_no = 0;
    for (seed, w, h, dirs, fires, _ended) in &replays {
        round_no += 1;
        game.start_recorded_round(*seed, *w, *h);
        let (mut di, mut fi) = (0usize, 0usize);
        while !game.game_over && game.frame_count < 20_000 {
            while fi < fires.len() && fires[fi][0] == game.frame_count {
                game.fire_powerup(fires[fi][1] as usize);
                fi += 1;
            }
            let next = game.frame_count + 1;
            while di < dirs.len() && dirs[di][0] == next {
                let (who, d) = (dirs[di][1] as usize, dirs[di][2] as u8);
                if who == 0 {
                    game.change_direction(dir_of(d));
                } else {
                    game.cycles[1].direction = dir_of(d);
                }
                di += 1;
            }
            game.update();
            if di >= dirs.len() && game.frame_count > dirs.last().map(|p| p[0]).unwrap_or(0) + 4 {
                break; // log exhausted — recorded round ended here
            }
        }
        let rr = &game.round_read;
        println!(
            "round {:>3}: {:>4} frames · read {:>5.1}% vs your-usual {:>5.1}% · lift {:>+5.1}% · cum lift {:>+5.1}%",
            round_no,
            game.frame_count,
            rr.rate() * 100.0,
            rr.base_rate() * 100.0,
            rr.lift() * 100.0,
            game.cpu_brain.lifetime_read.lift() * 100.0,
        );
    }

    let life = &game.cpu_brain.lifetime_read;
    println!(
        "\n==== THE REAL-HUMAN READ ====\nlifetime lift {:+.1}% over your own base rate · {} scored frames · {}",
        life.lift() * 100.0,
        life.samples,
        if life.is_significant() {
            "statistically significant (McNemar)"
        } else {
            "not yet significant — more rounds needed"
        }
    );
    println!(
        "(evaluate a candidate: WORM_TUNE_<KNOB>=x cargo run --release --example ghost_eval -- {path})"
    );
}
