//! Benchmark harness for the CPU AI.
//!
//! The rps-ai kata is "cold start -> adaptive -> learn -> measure honestly".
//! Measuring honestly here means: do NOT pit the CPU against a suicidal random
//! player (both sides "win" 100% and the number is meaningless). Instead both
//! opponents are real players and we score **survival** (moves) and **food**,
//! the two quantities the reward function is actually optimizing.

use worm::{CellType, Direction, WormGame};

mod ai {
    use super::*;

    /// Naive right-hand wall follower (baseline opponent / non-adaptive CPU).
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
        // Player: use the specified opponent algorithm.
        let dir = match opponent {
            Opponent::WallFollow => ai::wall_follow(&game),
            Opponent::Chaser => ai::chaser(&game, 0),
        };
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
    const SEED: u64 = 42; // Deterministic seed for reproducible benchmarks

    // --- FAMILIAR: wall-follower (for iterating, not evidence) ---
    println!("--- Familiar: wall-follower ---");
    let mut naive = Vec::with_capacity(GAMES);
    let mut adaptive = Vec::with_capacity(GAMES);
    let mut shared_brain = worm::CpuBrain::new();

    for i in 0..GAMES {
        let n_result = run_single_game(false, Opponent::WallFollow, None, Some(SEED + i as u64));
        let a_result = run_single_game(
            true,
            Opponent::WallFollow,
            Some(shared_brain),
            Some(SEED + 1000 + i as u64),
        );
        shared_brain = a_result
            .brain
            .expect("adaptive game should return its brain");

        naive.push(n_result.stats);
        adaptive.push(a_result.stats);
        if i < 4 || i % 25 == 0 {
            println!(
                "  Game {:2}: naive moves={:4} food={:2} | adaptive moves={:4} food={:2}",
                i + 1,
                n_result.stats.moves,
                n_result.stats.cpu_food,
                a_result.stats.moves,
                a_result.stats.cpu_food
            );
        }
    }

    let naive_moves: Vec<f32> = naive.iter().map(|s| s.moves as f32).collect();
    let adaptive_moves: Vec<f32> = adaptive.iter().map(|s| s.moves as f32).collect();
    let naive_food: Vec<f32> = naive.iter().map(|s| s.cpu_food as f32).collect();
    let adaptive_food: Vec<f32> = adaptive.iter().map(|s| s.cpu_food as f32).collect();
    let naive_survived = naive.iter().filter(|s| s.cpu_survived).count();
    let adaptive_survived = adaptive.iter().filter(|s| s.cpu_survived).count();

    println!(
        "  Naive:    survival={:>4} food={:5.1} alive={}/{}",
        mean(&naive_moves) as u32,
        mean(&naive_food),
        naive_survived,
        GAMES
    );
    println!(
        "  Adaptive: survival={:>4} food={:5.1} alive={}/{}",
        mean(&adaptive_moves) as u32,
        mean(&adaptive_food),
        adaptive_survived,
        GAMES
    );
    let dm = mean(&adaptive_moves) - mean(&naive_moves);
    let df = mean(&adaptive_food) - mean(&naive_food);
    // Win rate: alive at end AND game ended before max_moves (opponent crashed).
    let naive_wins = naive
        .iter()
        .filter(|s| s.cpu_survived && s.moves < 4000)
        .count();
    let adaptive_wins = adaptive
        .iter()
        .filter(|s| s.cpu_survived && s.moves < 4000)
        .count();
    println!(
        "  Wins:     naive={}/{} adaptive={}/{}",
        naive_wins, GAMES, adaptive_wins, GAMES
    );
    println!("  Delta:    {:+.1} moves, {:+.1} food\n", dm, df);

    // --- HELD-OUT: chaser (decides what ships) ---
    println!("--- Held-out: chaser ---");
    let mut naive2 = Vec::with_capacity(GAMES);
    let mut adaptive2 = Vec::with_capacity(GAMES);
    let mut shared_brain2 = worm::CpuBrain::new();

    for i in 0..GAMES {
        let n_result = run_single_game(false, Opponent::Chaser, None, Some(SEED + i as u64));
        let a_result = run_single_game(
            true,
            Opponent::Chaser,
            Some(shared_brain2),
            Some(SEED + 1000 + i as u64),
        );
        shared_brain2 = a_result
            .brain
            .expect("adaptive game should return its brain");

        naive2.push(n_result.stats);
        adaptive2.push(a_result.stats);
        if i < 4 || i % 25 == 0 {
            println!(
                "  Game {:2}: naive moves={:4} food={:2} | adaptive moves={:4} food={:2}",
                i + 1,
                n_result.stats.moves,
                n_result.stats.cpu_food,
                a_result.stats.moves,
                a_result.stats.cpu_food
            );
        }
    }

    let naive2_moves: Vec<f32> = naive2.iter().map(|s| s.moves as f32).collect();
    let adaptive2_moves: Vec<f32> = adaptive2.iter().map(|s| s.moves as f32).collect();
    let naive2_food: Vec<f32> = naive2.iter().map(|s| s.cpu_food as f32).collect();
    let adaptive2_food: Vec<f32> = adaptive2.iter().map(|s| s.cpu_food as f32).collect();
    let naive2_survived = naive2.iter().filter(|s| s.cpu_survived).count();
    let adaptive2_survived = adaptive2.iter().filter(|s| s.cpu_survived).count();

    println!(
        "  Naive:    survival={:>4} food={:5.1} alive={}/{}",
        mean(&naive2_moves) as u32,
        mean(&naive2_food),
        naive2_survived,
        GAMES
    );
    println!(
        "  Adaptive: survival={:>4} food={:5.1} alive={}/{}",
        mean(&adaptive2_moves) as u32,
        mean(&adaptive2_food),
        adaptive2_survived,
        GAMES
    );
    let dm2 = mean(&adaptive2_moves) - mean(&naive2_moves);
    let df2 = mean(&adaptive2_food) - mean(&naive2_food);
    // Win rate: alive at end AND game ended before max_moves (opponent crashed).
    let naive2_wins = naive2
        .iter()
        .filter(|s| s.cpu_survived && s.moves < 4000)
        .count();
    let adaptive2_wins = adaptive2
        .iter()
        .filter(|s| s.cpu_survived && s.moves < 4000)
        .count();
    println!(
        "  Wins:     naive={}/{} adaptive={}/{}",
        naive2_wins, GAMES, adaptive2_wins, GAMES
    );
    println!("  Delta:    {:+.1} moves, {:+.1} food\n", dm2, df2);

    // --- Verdict (held-out decides) ---
    println!("--- Verdict (held-out decides) ---");
    let naive_win_rate = naive2_wins as f32 / GAMES as f32;
    let adaptive_win_rate = adaptive2_wins as f32 / GAMES as f32;
    if adaptive_win_rate > naive_win_rate {
        println!(
            "Adaptive CPU WINS: {:.0}% vs naive's {:.0}% win rate vs chaser ({:+.1} moves, {:+.1} food)",
            adaptive_win_rate * 100.0, naive_win_rate * 100.0, dm2, df2
        );
    } else if dm2 > 0.0 || df2 > 0.0 {
        println!(
            "Adaptive CPU IMPROVEMENT vs chaser: {:+.1} moves, {:+.1} food (naive {:.0}% vs adaptive {:.0}% wins)",
            dm2, df2, naive_win_rate * 100.0, adaptive_win_rate * 100.0
        );
    } else {
        println!(
            "Adaptive CPU is flat/behind vs chaser ({:+.1} moves, {:+.1} food, naive {:.0}% vs adaptive {:.0}% wins) — needs reformulation",
            dm2, df2, naive_win_rate * 100.0, adaptive_win_rate * 100.0
        );
    }
}
