//! SPIKE: why doesn't fixture 1 coil? Replicates the contract staging
//! and prints every activation gate.

use worm::game::WormGame;
use worm::Direction;

fn main() {
    let mut game = WormGame::with_size(60, 30);
    game.food_items.clear();
    game.powerups.clear();
    for row in &mut game.grid {
        for cell in row.iter_mut() {
            *cell = worm::CellType::Empty;
        }
    }
    let mut v = [0.0f32; worm::cpu_ai::CPU_FEATURE_DIM];
    v[0] = 1.0;
    for _ in 0..60 {
        game.cpu_brain.remember(v, Direction::Up, 1.0);
    }
    let lr = &mut game.cpu_brain.lifetime_read;
    lr.lat_samples = 100;
    lr.lat_hits = 90;
    lr.lat_chance = 50.0;
    lr.lat_var = 25.0;
    lr.lat_latched = true;
    game.refresh_read_rate();
    for k in 0..=6u16 {
        game.grid[6][k as usize] = worm::CellType::Wall;
        game.grid[k as usize][6] = worm::CellType::Wall;
    }
    game.cycles[0].head = (5, 5);
    game.cycles[0].direction = Direction::Up;
    game.cycles[0].positions = vec![(5, 5), (4, 5)];
    game.grid[5][5] = worm::CellType::Player;
    game.grid[5][4] = worm::CellType::Player;
    let mut pos = Vec::new();
    for i in 0..60usize {
        let x = 11 + (i % 20) as u16;
        let y = 8 + (i / 20) as u16;
        pos.push((x, y));
    }
    game.cycles[1].head = pos[0];
    game.cycles[1].direction = Direction::Left;
    game.cycles[1].positions = pos.clone();
    for &(x, y) in &pos {
        game.grid[y as usize][x as usize] = worm::CellType::CPU;
    }
    println!("sharpness {}", game.discipline_sharpness());
    println!(
        "lens cpu {} opp {}",
        game.cycles[1].positions.len(),
        game.cycles[0].positions.len()
    );
    let (px, py) = game.cycles[0].head;
    let (cx, cy) = game.cycles[1].head;
    println!("dist {}", (px as i16 - cx as i16).abs() + (py as i16 - cy as i16).abs());
    println!("region {:?}", worm::cpu_ai::count_open_space(&game, px, py));
    // Direct probe of the driver before update() muddies the water.
    let cands = [Direction::Left, Direction::Up, Direction::Down];
    let d = worm::cpu_ai::coil_decide(&mut game, &cands);
    println!("direct coil_decide -> {:?}, episode {:?}", d,
        game.cpu_brain.coil.as_ref().map(|c| (c.phase, c.ring.len(), c.cursor)));
    game.cpu_brain.coil = None;
    game.cpu_brain.coil_cooldown_until = 0;
    for fr in 0..12 {
        if game.game_over {
            println!("game over at {fr}, cause {:?}", game.death_cause);
            break;
        }
        game.update();
        if let Some(d) = game.cpu_telemetry.decision.as_ref() {
            println!(
                "f{fr}: reason {:?} coil={:?}",
                d.reason,
                game.cpu_brain.coil.as_ref().map(|c| (c.phase, c.cursor))
            );
        } else {
            println!("f{fr}: no decision (dozed?)");
        }
    }
}
