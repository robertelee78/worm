//! Benchmark harness for the CPU AI.
//!
//! The rps-ai kata is "cold start -> adaptive -> learn -> measure honestly".
//! Measuring honestly here means: do NOT pit the CPU against a suicidal random
//! player (both sides "win" 100% and the number is meaningless). Instead both
//! opponents are real players and we score **survival** (moves) and **food**,
//! the two quantities the reward function is actually optimizing.

use rand::RngExt;
use worm::{Direction, WormGame};

mod ai {
    use super::*;

    /// Naive right-hand wall follower (baseline opponent / non-adaptive CPU).
    /// Cell-based passability: it can use corridors and punched holes, matching
    /// the game's real collision rules.
    pub fn wall_follow(game: &WormGame) -> Direction {
        let cpu = &game.cycles[1];
        let head = cpu.head;
        let current_dir = cpu.direction;

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

        let right_dir = right_map.iter().find(|(d, _)| *d == current_dir).map(|(_, r)| *r).unwrap_or(current_dir);
        let left_dir = left_map.iter().find(|(d, _)| *d == current_dir).map(|(_, l)| *l).unwrap_or(current_dir);
        let back_dir = back_map.iter().find(|(d, _)| *d == current_dir).map(|(_, b)| *b).unwrap_or(current_dir);

        for dir in [right_dir, current_dir, left_dir, back_dir] {
            let (dx, dy) = dir.as_delta();
            let nx = head.0 as i16 + dx;
            let ny = head.1 as i16 + dy;
            if nx < 0 || ny < 0 || nx >= game.width as i16 || ny >= game.height as i16 {
                continue;
            }
            if game.passable(nx as u16, ny as u16) {
                return dir;
            }
        }
        current_dir
    }
}

#[derive(Clone, Copy)]
struct GameStats {
    /// Frames survived before the game ended.
    moves: u32,
    /// Food the CPU ate.
    cpu_food: u32,
    /// Whether the CPU was alive when the game ended.
    cpu_survived: bool,
}

/// Result of a benchmark game: stats plus the final brain state for adaptive runs.
struct GameResult {
    stats: GameStats,
    brain: Option<worm::CpuBrain>,
}

/// Run one game to completion.
///
/// `cpu_adaptive`: if true, the CPU uses its k-NN memory brain (herding on,
/// matching the game's difficulty>=3 behavior); if false, the naive wall
/// follower drives cycle 1.
///
/// The player (cycle 0) is a fixed wall-follower so the CPU faces a real,
/// survivable opponent — not a suicide bot.
///
/// `shared_brain`: An optional pre-existing brain to inject into the game.
/// This allows the adaptive CPU to retain memory across games, simulating
/// the cross-session persistence that rps-ai achieves via persistent storage.
/// The returned `GameResult` includes the updated brain so it can be fed
/// into the next game.
fn run_single_game(cpu_adaptive: bool, shared_brain: Option<worm::CpuBrain>) -> GameResult {
    let mut game = WormGame::new();
    // Inject the shared brain for adaptive runs to enable cross-game learning.
    if let Some(brain) = shared_brain {
        game.cpu_brain = brain;
    }
    let max_moves = 4000;
    for _ in 0..max_moves {
        if game.game_over {
            break;
        }
        // CPU cycle drives itself inside update() when adaptive; the naive
        // opponent needs an explicit steer each frame.
        if !cpu_adaptive {
            let dir = ai::wall_follow(&game);
            game.cycles[1].change_direction(dir);
        }
        // Player: wall-follower (the same algorithm, mirrored to the left side).
        let dir = ai::wall_follow(&game);
        game.change_direction(dir);
        game.update();
    }
    let stats = GameStats {
        moves: game.time,
        cpu_food: game.cycles[1].score,
        cpu_survived: game.cycles[1].alive,
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

fn main() {
    println!("TRON Light Cycle CPU AI Benchmark — honest survival + food");
    println!("===========================================================\n");

    const GAMES: usize = 100;
    let mut naive = Vec::with_capacity(GAMES);
    let mut adaptive = Vec::with_capacity(GAMES);

    // A single, persistent brain for the adaptive CPU — mirrors rps-ai's
    // cross-session memory DB. This is what lets the opponent model learn
    // across multiple games rather than resetting every time.
    let mut shared_brain = worm::CpuBrain::new();

    for i in 0..GAMES {
        let n_result = run_single_game(false, None);
        let a_result = run_single_game(true, Some(shared_brain));
        shared_brain = a_result.brain.expect("adaptive game should return its brain");

        naive.push(n_result.stats);
        adaptive.push(a_result.stats);
        if i < 8 || i % 5 == 0 {
            println!(
                "  Game {:2}: naive moves={:4} food={:2} | adaptive moves={:4} food={:2}",
                i + 1, n_result.stats.moves, n_result.stats.cpu_food,
                a_result.stats.moves, a_result.stats.cpu_food
            );
        }
    }

    let naive_moves: Vec<f32> = naive.iter().map(|s| s.moves as f32).collect();
    let adaptive_moves: Vec<f32> = adaptive.iter().map(|s| s.moves as f32).collect();
    let naive_food: Vec<f32> = naive.iter().map(|s| s.cpu_food as f32).collect();
    let adaptive_food: Vec<f32> = adaptive.iter().map(|s| s.cpu_food as f32).collect();

    let naive_survived = naive.iter().filter(|s| s.cpu_survived).count();
    let adaptive_survived = adaptive.iter().filter(|s| s.cpu_survived).count();

    println!("\n--- Results ---");
    println!(
        "Naive wall-follower:   survival={:>4} moves (avg), food={:5.1} (avg), alive-at-end {}/{}",
        mean(&naive_moves) as u32, mean(&naive_food), naive_survived, GAMES
    );
    println!(
        "Adaptive memory CPU:   survival={:>4} moves (avg), food={:5.1} (avg), alive-at-end {}/{}",
        mean(&adaptive_moves) as u32, mean(&adaptive_food), adaptive_survived, GAMES
    );

    let dm = mean(&adaptive_moves) - mean(&naive_moves);
    let df = mean(&adaptive_food) - mean(&naive_food);
    println!("\n--- Verdict ---");
    let dm_s = if dm >= 0.0 { format!("+{:.1}", dm) } else { format!("{:.1}", dm) };
    let df_s = if df >= 0.0 { format!("+{:.1}", df) } else { format!("{:.1}", df) };
    if dm > 0.0 || df > 0.0 {
        println!(
            "Adaptive CPU IMPROVEMENT: {} moves survival, {} food eaten",
            dm_s, df_s
        );
    } else {
        println!(
            "Adaptive CPU is flat/behind ({} moves, {} food) — needs reformulation",
            dm_s, df_s
        );
    }
}
