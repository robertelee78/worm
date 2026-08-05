//! Benchmark harness for the CPU AI.
//!
//! The rps-ai kata is "cold start -> adaptive -> learn -> measure honestly".
//! Measuring honestly here means: do NOT pit the CPU against a suicidal random
//! player (both sides "win" 100% and the number is meaningless). Instead both
//! opponents are real players and we score **round frames** and **actual food**,
//! the two quantities the reward function is actually optimizing.

use worm::{CellType, Direction, WormGame};

mod ai {
    use super::*;

    /// Naive right-hand wall follower (baseline opponent / non-adaptive CPU).
    pub fn wall_follow(game: &WormGame, who: usize) -> Direction {
        let cycle = &game.cycles[who];
        let head = cycle.head;
        let current_dir = cycle.direction;

        let right_map = [
            (Direction::Up, Direction::Right),
            (Direction::Right, Direction::Down),
            (Direction::Down, Direction::Left),
            (Direction::Left, Direction::Up),
        ];
        let left_map = [
            (Direction::Up, Direction::Left),
            (Direction::Left, Direction::Down),
            (Direction::Down, Direction::Right),
            (Direction::Right, Direction::Up),
        ];
        let back_map = [
            (Direction::Up, Direction::Down),
            (Direction::Down, Direction::Up),
            (Direction::Left, Direction::Right),
            (Direction::Right, Direction::Left),
        ];

        let right_dir = right_map
            .iter()
            .find(|(d, _)| *d == current_dir)
            .map(|(_, r)| *r)
            .unwrap_or(current_dir);
        let left_dir = left_map
            .iter()
            .find(|(d, _)| *d == current_dir)
            .map(|(_, l)| *l)
            .unwrap_or(current_dir);
        let back_dir = back_map
            .iter()
            .find(|(d, _)| *d == current_dir)
            .map(|(_, b)| *b)
            .unwrap_or(current_dir);

        for dir in [right_dir, current_dir, left_dir, back_dir] {
            let (dx, dy) = dir.as_delta();
            let new_x = (head.0 as i16 + dx).max(1).min((game.width - 2) as i16) as u16;
            let new_y = (head.1 as i16 + dy).max(1).min((game.height - 2) as i16) as u16;
            if new_x >= 1
                && new_x < game.width - 1
                && new_y >= 1
                && new_y < game.height - 1
                && game.grid[new_y as usize][new_x as usize] == CellType::Empty
            {
                return dir;
            }
        }
        current_dir
    }

    /// Aggressive chaser: moves toward the opponent's head. A real threat that
    /// wall-followers can't handle — the adaptive CPU's opponent model should
    /// learn to predict and avoid/intercept this opponent.
    pub fn chaser(game: &WormGame, who: usize) -> Direction {
        let cpu = &game.cycles[who];
        let head = cpu.head;
        let current_dir = cpu.direction;
        let target_head = game.cycles[1 - who].head;

        // Try to move toward the target head (Manhattan direction).
        let dx = target_head.0 as i16 - head.0 as i16;
        let dy = target_head.1 as i16 - head.1 as i16;

        let mut candidates = Vec::new();
        if dx > 0 {
            candidates.push(Direction::Right);
        }
        if dx < 0 {
            candidates.push(Direction::Left);
        }
        if dy > 0 {
            candidates.push(Direction::Down);
        }
        if dy < 0 {
            candidates.push(Direction::Up);
        }
        // Fallback: keep going straight, then any legal move.
        candidates.push(current_dir);

        let back_map = [
            (Direction::Up, Direction::Down),
            (Direction::Down, Direction::Up),
            (Direction::Left, Direction::Right),
            (Direction::Right, Direction::Left),
        ];
        let back_dir = back_map
            .iter()
            .find(|(d, _)| *d == current_dir)
            .map(|(_, b)| *b)
            .unwrap_or(current_dir);

        for dir in candidates {
            if dir == back_dir {
                continue;
            } // no 180s
            let (ddx, ddy) = dir.as_delta();
            let new_x = (head.0 as i16 + ddx).max(1).min((game.width - 2) as i16) as u16;
            let new_y = (head.1 as i16 + ddy).max(1).min((game.height - 2) as i16) as u16;
            if new_x >= 1
                && new_x < game.width - 1
                && new_y >= 1
                && new_y < game.height - 1
                && game.grid[new_y as usize][new_x as usize] == CellType::Empty
            {
                return dir;
            }
        }
        current_dir
    }
}

#[derive(Clone, Copy)]
struct GameStats {
    /// Total round frames before an outcome or the deterministic cap.
    frames: u32,
    /// Actual food value eaten by the CPU (never its survival-weighted score).
    cpu_food: u32,
    outcome: Outcome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    CpuWin,
    PlayerWin,
    Draw,
    Capped,
}

/// Result of a benchmark game: stats plus the final brain state for adaptive runs.
struct GameResult {
    stats: GameStats,
    brain: Option<worm::CpuBrain>,
}

/// Which scripted opponent the player (cycle 0) uses.
#[derive(Clone, Copy)]
enum Opponent {
    /// Right-hand wall follower (familiar — for iterating).
    WallFollow,
    /// Aggressive chaser (held-out — decides what ships).
    Chaser,
}

/// Run one game to completion.
///
/// `cpu_adaptive`: if true, the CPU uses its k-NN memory brain (autopilot on,
/// learning recorded); if false, autopilot is disabled and the naive wall
/// follower drives cycle 1 through the external steer below — update() will
/// not override it with cpu_decide. The "naive" row is therefore genuinely
/// naive (previously it was a fresh-brain adaptive CPU in disguise).
///
/// The player (cycle 0) uses the specified opponent algorithm.
///
/// `shared_brain`: An optional pre-existing brain to inject into the game.
/// This allows the adaptive CPU to retain memory across games, simulating
/// the cross-session persistence that rps-ai achieves via persistent storage.
/// The returned `GameResult` includes the updated brain so it can be fed
/// into the next game.
///
/// `seed`: Optional RNG seed for deterministic benchmarks.
fn run_single_game(
    cpu_adaptive: bool,
    opponent: Opponent,
    shared_brain: Option<worm::CpuBrain>,
    seed: Option<u64>,
) -> GameResult {
    let mut game = match seed {
        Some(s) => WormGame::with_size_seed(120, 38, s),
        None => WormGame::new(),
    };
    // Inject the shared brain for adaptive runs to enable cross-game learning.
    if let Some(brain) = shared_brain {
        game.cpu_brain = brain;
    }
    // Naive rows: scripted steer only — update() must not run the AI.
    game.cpu_autopilot = cpu_adaptive;
    const MAX_FRAMES: u32 = 4000;
    for _ in 0..MAX_FRAMES {
        if game.game_over {
            break;
        }
        // CPU cycle drives itself inside update() when adaptive; the naive
        // opponent needs an explicit steer each frame.
        if !cpu_adaptive {
            let dir = ai::wall_follow(&game, 1);
            game.cycles[1].change_direction(dir);
        }
        // Player: use the specified opponent algorithm.
        let dir = match opponent {
            Opponent::WallFollow => ai::wall_follow(&game, 0),
            Opponent::Chaser => ai::chaser(&game, 0),
        };
        game.change_direction(dir);
        game.update();
    }
    let outcome = if !game.game_over {
        Outcome::Capped
    } else {
        match game.winner {
            Some(1) => Outcome::CpuWin,
            Some(0) => Outcome::PlayerWin,
            _ => Outcome::Draw,
        }
    };
    let stats = GameStats {
        frames: game.time,
        cpu_food: game.food_eaten_by[1],
        outcome,
    };
    let brain = if cpu_adaptive {
        Some(game.cpu_brain.clone())
    } else {
        None
    };
    GameResult { stats, brain }
}

fn mean(v: &[f32]) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.iter().sum::<f32>() / v.len() as f32
}

#[derive(Clone, Copy)]
struct Summary {
    mean_frames: f32,
    mean_food: f32,
    cpu_wins: usize,
    player_wins: usize,
    draws: usize,
    capped: usize,
}

fn summarize(rows: &[GameStats]) -> Summary {
    let frames: Vec<f32> = rows.iter().map(|s| s.frames as f32).collect();
    let food: Vec<f32> = rows.iter().map(|s| s.cpu_food as f32).collect();
    let summary = Summary {
        mean_frames: mean(&frames),
        mean_food: mean(&food),
        cpu_wins: rows.iter().filter(|s| s.outcome == Outcome::CpuWin).count(),
        player_wins: rows
            .iter()
            .filter(|s| s.outcome == Outcome::PlayerWin)
            .count(),
        draws: rows.iter().filter(|s| s.outcome == Outcome::Draw).count(),
        capped: rows.iter().filter(|s| s.outcome == Outcome::Capped).count(),
    };
    assert_eq!(
        summary.cpu_wins + summary.player_wins + summary.draws + summary.capped,
        rows.len(),
        "every game must have exactly one explicit outcome"
    );
    summary
}

fn print_summary(label: &str, summary: Summary) {
    println!(
        "  {label:<9} frames={:>6.1} actual-food={:>4.1} outcomes cpu/player/draw/cap={}/{}/{}/{}",
        summary.mean_frames,
        summary.mean_food,
        summary.cpu_wins,
        summary.player_wins,
        summary.draws,
        summary.capped,
    );
}

fn main() {
    println!("TRON Light Cycle CPU AI Benchmark — honest survival + food");
    println!("===========================================================\n");

    const GAMES: usize = 100;
    const SEED: u64 = 42; // Deterministic seed for reproducible benchmarks

    // --- FAMILIAR: wall-follower (for iterating, not evidence) ---
    println!("--- Familiar: wall-follower ---");
    let mut naive = Vec::with_capacity(GAMES);
    let mut adaptive = Vec::with_capacity(GAMES);
    let mut shared_brain = worm::CpuBrain::new();

    for i in 0..GAMES {
        let pair_seed = SEED + i as u64;
        let n_result = run_single_game(false, Opponent::WallFollow, None, Some(pair_seed));
        let a_result = run_single_game(
            true,
            Opponent::WallFollow,
            Some(shared_brain),
            Some(pair_seed),
        );
        shared_brain = a_result
            .brain
            .expect("adaptive game should return its brain");

        naive.push(n_result.stats);
        adaptive.push(a_result.stats);
        if i < 4 || i % 25 == 0 {
            println!(
                "  Game {:3}: naive frames={:4} food={:2} | adaptive frames={:4} food={:2}",
                i + 1,
                n_result.stats.frames,
                n_result.stats.cpu_food,
                a_result.stats.frames,
                a_result.stats.cpu_food
            );
        }
    }

    let naive_summary = summarize(&naive);
    let adaptive_summary = summarize(&adaptive);
    print_summary("Naive", naive_summary);
    print_summary("Adaptive", adaptive_summary);
    println!(
        "  Delta:     {:+.1} round-frames, {:+.1} actual-food\n",
        adaptive_summary.mean_frames - naive_summary.mean_frames,
        adaptive_summary.mean_food - naive_summary.mean_food,
    );

    // --- HELD-OUT: chaser (decides what ships) ---
    println!("--- Held-out: chaser ---");
    let mut naive2 = Vec::with_capacity(GAMES);
    let mut adaptive2 = Vec::with_capacity(GAMES);
    let mut shared_brain2 = worm::CpuBrain::new();

    for i in 0..GAMES {
        let pair_seed = SEED + i as u64;
        let n_result = run_single_game(false, Opponent::Chaser, None, Some(pair_seed));
        let a_result =
            run_single_game(true, Opponent::Chaser, Some(shared_brain2), Some(pair_seed));
        shared_brain2 = a_result
            .brain
            .expect("adaptive game should return its brain");

        naive2.push(n_result.stats);
        adaptive2.push(a_result.stats);
        if i < 4 || i % 25 == 0 {
            println!(
                "  Game {:3}: naive frames={:4} food={:2} | adaptive frames={:4} food={:2}",
                i + 1,
                n_result.stats.frames,
                n_result.stats.cpu_food,
                a_result.stats.frames,
                a_result.stats.cpu_food
            );
        }
    }

    let naive2_summary = summarize(&naive2);
    let adaptive2_summary = summarize(&adaptive2);
    print_summary("Naive", naive2_summary);
    print_summary("Adaptive", adaptive2_summary);
    let frame_delta = adaptive2_summary.mean_frames - naive2_summary.mean_frames;
    let food_delta = adaptive2_summary.mean_food - naive2_summary.mean_food;
    println!(
        "  Delta:     {:+.1} round-frames, {:+.1} actual-food\n",
        frame_delta, food_delta,
    );

    // --- Verdict (held-out decides) ---
    println!("--- Verdict (held-out decides) ---");
    let naive_win_rate = naive2_summary.cpu_wins as f32 / GAMES as f32;
    let adaptive_win_rate = adaptive2_summary.cpu_wins as f32 / GAMES as f32;
    println!(
        "Adaptive CPU held-out wins: {:.0}% vs naive {:.0}% ({:+.1} round-frames, {:+.1} actual-food)",
        adaptive_win_rate * 100.0,
        naive_win_rate * 100.0,
        frame_delta,
        food_delta,
    );
    assert!(
        adaptive2_summary.cpu_wins > naive2_summary.cpu_wins,
        "HELD-OUT GATE FAILED: adaptive CPU wins {} must beat naive wins {}",
        adaptive2_summary.cpu_wins,
        naive2_summary.cpu_wins,
    );
}
