//! SPIKE (owner bug report 2026-08-08: "tri-shot doesn't feel lethal —
//! no flame on hit, no tail shrink"): stage v11 tri-shot geometries
//! against the live engine and report exactly what a hit produces.

use worm::game::{CellType, PowerUpKind, WormGame};
use worm::Direction;

fn stage(cpu_cells: &[(u16, u16)]) -> WormGame {
    let mut game = WormGame::with_size_seed(60, 30, 1);
    for row in &mut game.grid {
        for cell in row.iter_mut() {
            if *cell != CellType::Wall {
                *cell = CellType::Empty;
            }
        }
    }
    // Player at (10,15) heading Right, armed.
    game.cycles[0].head = (10, 15);
    game.cycles[0].positions = vec![(10, 15), (9, 15)];
    game.cycles[0].direction = Direction::Right;
    game.grid[15][10] = CellType::Player;
    game.grid[15][9] = CellType::Player;
    game.cycles[0].held_powerup = Some(PowerUpKind::TriShot);
    // CPU body as prescribed.
    game.cycles[1].head = cpu_cells[0];
    game.cycles[1].positions = cpu_cells.to_vec();
    for &(x, y) in cpu_cells {
        game.grid[y as usize][x as usize] = CellType::CPU;
    }
    game.cycles[1].direction = Direction::Down;
    game
}

fn run(label: &str, cpu_cells: &[(u16, u16)], frames: u32) {
    let mut game = stage(cpu_cells);
    println!("== {label} (arena v{}) ==", game.arena_version);
    let fired = game.fire_powerup(0);
    println!("  fired: {fired}  bolts spawned: {}", game.projectiles.len());
    let len0 = game.cycles[1].positions.len();
    for f in 0..frames {
        game.advance_projectiles();
        game.tick_flames();
        if f < 6 {
            println!(
                "  frame {f}: bolts {}  flames {}  cpu burn contact_ms {} taken {}  cpu len {}",
                game.projectiles.len(),
                game.flames.len(),
                game.burns[1].contact_ms,
                game.burns[1].taken,
                game.cycles[1].positions.len()
            );
        }
    }
    println!(
        "  END: cpu len {} -> {}  burn taken {}  cpu alive {}  cause {:?}",
        len0,
        game.cycles[1].positions.len(),
        game.burns[1].taken,
        game.cycles[1].alive,
        game.death_cause
    );
}

fn main() {
    // Broadside: vertical CPU wall crossing the straight ray at x=20.
    let broadside: Vec<(u16, u16)> =
        (11..=19).map(|y| (20u16, y as u16)).chain([(20, 20)]).collect();
    run("broadside straight ray", &broadside, 120);

    // Diagonal ray target, parity ALIGNED: cells on y = 15 - (x - 10).
    let diag_hit: Vec<(u16, u16)> = (9..=13).map(|y| (16u16, y as u16)).collect();
    run("diagonal ray, worm at x=16 (ray passes (16,9))", &diag_hit, 120);

    // Long-burn check: does a caught worm actually lose 5/3/1 = 9 over 3s?
    run("burn-to-completion on broadside", &broadside, 200);

    // PARITY-MISALIGNED diagonal: the up-right ray from (10,15) occupies
    // (11,14),(12,13),(13,12)... — a vertical worm at x=16 whose cells sit
    // on the OFF-parity diagonal cells (16,10) is ON the ray; instead park
    // the worm one column over at x=17 crossing the ray's corner gaps:
    // ray cells are (16,9),(17,8) — a worm filling (17,9)..(17,13) is
    // crossed BETWEEN (16,9)->(17,8) without cell overlap.
    let diag_tunnel: Vec<(u16, u16)> = (9..=13).map(|y| (17u16, y as u16)).collect();
    run("diagonal ray, worm at x=17 (corner-crossed, never co-celled)", &diag_tunnel, 120);
}
