use worm::{Bomb, CellType, Direction, PowerUpKind, WormGame};

/// Teleport a cycle for a scripted scenario: erases its old grid marks, sets
/// head/direction, and lays the given trail (head-first order).
fn place_cycle(game: &mut WormGame, idx: usize, head: (u16, u16), dir: Direction, tail: &[(u16, u16)]) {
    let marker = if idx == 0 { CellType::Player } else { CellType::CPU };
    let old: Vec<(u16, u16)> = game.cycles[idx].positions.clone();
    for (px, py) in old {
        if game.grid[py as usize][px as usize] == marker {
            game.grid[py as usize][px as usize] = CellType::Empty;
        }
    }
    game.cycles[idx].head = head;
    game.cycles[idx].direction = dir;
    game.cycles[idx].positions.clear();
    game.cycles[idx].positions.push(head);
    game.grid[head.1 as usize][head.0 as usize] = marker;
    for &t in tail {
        game.cycles[idx].positions.push(t);
        game.grid[t.1 as usize][t.0 as usize] = marker;
    }
}

#[test]
fn test_new_game_initial_state() {
    let game = WormGame::new();
    assert_eq!(game.cycles.len(), 2);
    assert_eq!(game.cycles[0].positions.len(), 1);
    assert!(!game.game_over);
    assert_eq!(game.score, 0);
}

#[test]
fn test_snake_movement() {
    let mut game = WormGame::new();
    let original_head = game.cycles[0].head;
    game.update();
    assert_ne!(game.cycles[0].head, original_head);
}

#[test]
fn test_food_collection_increases_score() {
    let mut game = WormGame::new();
    let head = game.cycles[0].head;
    game.food_items.clear();
    game.food_items.push((head.0 + 1, head.1, 1));
    game.cycles[0].direction = worm::Direction::Right;
    let old_score = game.score;
    game.update();
    assert!(game.score > old_score);
}

#[test]
fn test_wall_collision() {
    let mut game = WormGame::new();
    let head = (1, game.height / 2);
    game.cycles[0].head = head;
    game.cycles[0].positions.clear();
    game.cycles[0].positions.push(head);
    game.cycles[0].direction = worm::Direction::Left;
    game.grid[head.1 as usize][1] = worm::CellType::Wall;
    game.update();
    assert!(game.game_over);
}

#[test]
fn test_self_collision() {
    let mut game = WormGame::new();
    game.cycles[0].head = (10, 10);
    game.cycles[0].positions.clear();
    game.cycles[0].positions.push((10, 10));
    game.cycles[0].positions.push((9, 10));
    game.cycles[0].positions.push((8, 10));
    game.grid[10][8] = worm::CellType::Wall;
    game.grid[10][9] = worm::CellType::Wall;
    game.grid[10][10] = worm::CellType::Wall;
    game.cycles[0].direction = worm::Direction::Left;
    game.update();
    assert!(game.game_over);
}

#[test]
fn test_no_reverse_direction() {
    let mut game = WormGame::new();
    game.cycles[0].direction = worm::Direction::Right;
    game.cycles[0].change_direction(worm::Direction::Left);
    assert_eq!(game.cycles[0].direction, worm::Direction::Right);
}

#[test]
fn test_food_items_are_on_grid() {
    let game = WormGame::new();
    assert!(!game.food_items.is_empty());
    for &(fx, fy, _) in &game.food_items {
        assert_eq!(
            game.grid[fy as usize][fx as usize],
            worm::CellType::Food,
            "food item at ({fx},{fy}) must be a grid Food cell"
        );
    }
}

#[test]
fn test_tail_retracts_without_food() {
    let mut game = WormGame::new();
    let head = game.cycles[0].head;
    game.cycles[0].positions.clear();
    game.cycles[0].positions.push(head);
    game.cycles[0].positions.push((head.0 - 1, head.1));
    game.cycles[0].positions.push((head.0 - 2, head.1));
    game.grid[head.1 as usize][head.0 as usize] = worm::CellType::Player;
    game.grid[head.1 as usize][(head.0 - 1) as usize] = worm::CellType::Player;
    game.grid[head.1 as usize][(head.0 - 2) as usize] = worm::CellType::Player;
    game.cycles[0].direction = worm::Direction::Right;
    game.food_items.clear();
    let before = game.cycles[0].positions.len();
    game.update();
    assert_eq!(
        game.cycles[0].positions.len(),
        before,
        "no food eaten → tail retracts, length stays flat"
    );
    assert_eq!(
        game.grid[head.1 as usize][(head.0 - 2) as usize],
        worm::CellType::Empty,
        "vacated tail cell must be cleared"
    );
    assert_eq!(
        game.cycles[0].positions[0],
        (head.0 + 1, head.1),
        "positions must be head-first: index 0 is the new head after a Right move"
    );
}

#[test]
fn test_tail_grows_by_food_value() {
    let mut game = WormGame::new();
    let head = game.cycles[0].head;
    game.cycles[0].positions.clear();
    game.cycles[0].positions.push(head);
    game.cycles[0].positions.push((head.0 - 1, head.1));
    game.cycles[0].positions.push((head.0 - 2, head.1));
    game.grid[head.1 as usize][head.0 as usize] = worm::CellType::Player;
    game.grid[head.1 as usize][(head.0 - 1) as usize] = worm::CellType::Player;
    game.grid[head.1 as usize][(head.0 - 2) as usize] = worm::CellType::Player;
    game.cycles[0].direction = worm::Direction::Right;
    game.food_items.clear();
    game.food_items.push((head.0 + 1, head.1, 5));
    let len_before = game.cycles[0].positions.len();
    game.update();
    assert_eq!(game.cycles[0].pending_growth, 5);
    assert_eq!(game.cycles[0].positions.len(), len_before);
    assert_eq!(game.cycles[0].score, 5);
    // Pin a far-away food so the tray never empties and regenerates onto the path.
    game.food_items.clear();
    game.food_items.push((2, 2, 1));
    for _ in 0..5 {
        game.update();
    }
    assert_eq!(
        game.cycles[0].positions.len(),
        len_before + 5,
        "snake grows exactly by the food value over the following frames"
    );
}

#[test]
fn test_restart_after_game_over() {
    let mut game = WormGame::new();
    game.game_over = true;
    game.restart();
    assert!(!game.game_over);
    assert_eq!(game.score, 0);
    assert_eq!(game.cycles[0].positions.len(), 1);
}

/* ------------------------------ power-ups ------------------------------ */

#[test]
fn test_powerup_pickup_grants_held() {
    let mut game = WormGame::new();
    let head = game.cycles[0].head;
    game.powerups.push((head.0 + 1, head.1, PowerUpKind::Laser));
    game.grid[head.1 as usize][(head.0 + 1) as usize] = CellType::PowerUp;
    game.cycles[0].direction = Direction::Right;
    game.update();
    assert_eq!(game.cycles[0].held_powerup, Some(PowerUpKind::Laser));
}

#[test]
fn test_powerup_spawns_when_timer_fires() {
    let mut game = WormGame::new();
    game.powerup_timer = 0;
    game.update();
    assert!(!game.powerups.is_empty(), "timer expiry spawns a power-up");
    let &(px, py, _) = game.powerups.first().unwrap();
    assert_eq!(game.grid[py as usize][px as usize], CellType::PowerUp);
}

#[test]
fn test_laser_kills_opponent_head_in_line() {
    let mut game = WormGame::new();
    place_cycle(&mut game, 0, (10, 10), Direction::Right, &[]);
    place_cycle(&mut game, 1, (20, 10), Direction::Left, &[]);
    game.cycles[0].held_powerup = Some(PowerUpKind::Laser);
    assert!(game.fire_powerup(0));
    assert!(!game.cycles[1].alive, "beam through the head kills");
    assert!(game.game_over);
    assert_eq!(game.winner, Some(0));
    assert_eq!(game.cycles[0].held_powerup, None, "firing consumes the power-up");
}

#[test]
fn test_laser_blocked_by_wall() {
    let mut game = WormGame::new();
    place_cycle(&mut game, 0, (10, 10), Direction::Right, &[]);
    place_cycle(&mut game, 1, (20, 10), Direction::Left, &[]);
    game.grid[10][15] = CellType::Wall;
    game.cycles[0].held_powerup = Some(PowerUpKind::Laser);
    game.fire_powerup(0);
    assert!(game.cycles[1].alive, "beam stops at the first wall");
    assert!(!game.game_over);
}

#[test]
fn test_laser_severs_trail_and_deducts_score() {
    let mut game = WormGame::new();
    place_cycle(&mut game, 0, (10, 10), Direction::Right, &[]);
    // CPU head is safely off the beam row; its trail crosses it at (15,10).
    place_cycle(&mut game, 1, (15, 6), Direction::Up, &[(15, 7), (15, 8), (15, 9), (15, 10), (15, 11)]);
    game.cycles[1].score = 10;
    game.cycles[0].held_powerup = Some(PowerUpKind::Laser);
    game.fire_powerup(0);
    assert!(game.cycles[1].alive, "body hit must not kill");
    assert_eq!(game.cycles[1].positions.len(), 4, "tail severed at the struck cell");
    assert_eq!(game.cycles[1].score, 8, "one point deducted per lost tail cell");
    assert_eq!(game.grid[10][15], CellType::Empty, "severed cells cleared");
    assert_eq!(game.grid[11][15], CellType::Empty);
}

#[test]
fn test_trishot_bolts_die_after_range() {
    let mut game = WormGame::new();
    let far = (game.width.saturating_sub(5), game.height / 2);
    place_cycle(&mut game, 0, (10, 10), Direction::Right, &[]);
    place_cycle(&mut game, 1, far, Direction::Left, &[]);
    game.cycles[0].held_powerup = Some(PowerUpKind::TriShot);
    game.fire_powerup(0);
    assert_eq!(game.projectiles.len(), 3, "straight + two diagonals");
    for _ in 0..8 {
        game.advance_projectiles();
    }
    assert!(game.projectiles.is_empty(), "bolts die at TRI_SHOT_RANGE");
}

#[test]
fn test_trishot_severs_trail() {
    let mut game = WormGame::new();
    place_cycle(&mut game, 0, (10, 10), Direction::Right, &[]);
    // CPU trail crosses the straight bolt path (y=10) at x=14; head is elsewhere.
    place_cycle(&mut game, 1, (30, 5), Direction::Up, &[(14, 8), (14, 9), (14, 10), (14, 11)]);
    game.cycles[1].score = 6;
    game.cycles[0].held_powerup = Some(PowerUpKind::TriShot);
    game.fire_powerup(0);
    for _ in 0..4 {
        game.advance_projectiles();
    }
    assert!(game.cycles[1].alive, "body hit must not kill");
    assert_eq!(game.cycles[1].positions.len(), 3, "bolt severed tail at (14,10)");
    assert_eq!(game.cycles[1].score, 4, "lost cells deducted");
}

#[test]
fn test_trishot_passes_through_shooters_head() {
    // Bolt and shooter both advance one cell per frame — the bolt must cross
    // its own shooter's head position harmlessly on the first advance.
    let mut game = WormGame::new();
    let far = (game.width.saturating_sub(5), game.height / 2);
    place_cycle(&mut game, 0, (10, 10), Direction::Right, &[]);
    place_cycle(&mut game, 1, far, Direction::Left, &[]);
    game.cycles[0].held_powerup = Some(PowerUpKind::TriShot);
    game.fire_powerup(0);
    game.advance_projectiles();
    assert!(game.cycles[0].alive, "shooter's bolt never kills the shooter");
    assert!(!game.game_over);
}

#[test]
fn test_bomb_detonation_kills_head_in_radius() {
    let mut game = WormGame::new();
    place_cycle(&mut game, 0, (10, 10), Direction::Right, &[]);
    place_cycle(&mut game, 1, (14, 10), Direction::Left, &[]);
    game.cycles[0].held_powerup = Some(PowerUpKind::Bomb);
    game.fire_powerup(0);
    assert_eq!(game.bombs.len(), 1);
    let escape = (10, game.height.saturating_sub(1)); // well outside R=10 from (10,10)
    place_cycle(&mut game, 0, escape, Direction::Right, &[]);
    game.bombs[0].fuse = 1;
    game.tick_bombs();
    assert!(!game.cycles[1].alive, "opponent head in blast radius dies");
    assert!(game.cycles[0].alive, "planter outside the blast survives");
    assert_eq!(game.winner, Some(0));
}

#[test]
fn test_bomb_kills_planter_who_stays() {
    let mut game = WormGame::new();
    place_cycle(&mut game, 0, (10, 10), Direction::Right, &[]);
    place_cycle(&mut game, 1, (14, 10), Direction::Left, &[]);
    game.cycles[0].held_powerup = Some(PowerUpKind::Bomb);
    game.fire_powerup(0);
    game.bombs[0].fuse = 1;
    game.tick_bombs();
    assert!(game.game_over);
    assert_eq!(game.winner, None, "both heads in the blast -> draw");
}

#[test]
fn test_bomb_severs_tail_when_head_outside_blast() {
    let mut game = WormGame::new();
    let far = (game.width.saturating_sub(3), game.height.saturating_sub(1));
    place_cycle(&mut game, 0, (10, 10), Direction::Right, &[]);
    // CPU head well outside the blast; its whole trail dips inside it.
    place_cycle(&mut game, 1, far, Direction::Up, &[(14, 10), (15, 10), (16, 10)]);
    game.cycles[1].score = 5;
    game.cycles[0].held_powerup = Some(PowerUpKind::Bomb);
    game.fire_powerup(0);
    place_cycle(&mut game, 0, far, Direction::Right, &[]); // planter escaped
    game.bombs[0].fuse = 1;
    game.tick_bombs();
    assert!(game.cycles[1].alive);
    assert_eq!(game.cycles[1].positions.len(), 1, "tail inside blast severed away");
    assert_eq!(game.cycles[1].score, 2);
}

#[test]
fn test_wallpunch_creates_hole_not_in_frame() {
    let mut game = WormGame::new();
    // Facing the arena wall (ring 2) from the interior.
    place_cycle(&mut game, 0, (5, 4), Direction::Up, &[]);
    game.cycles[0].held_powerup = Some(PowerUpKind::WallPunch);
    game.fire_powerup(0);
    assert_eq!(game.grid[2][5], CellType::Hole, "arena wall punched open");
    assert!(game.passable(5, 2), "hole is passable");

    // Facing the outer frame from the corridor — never punchable.
    place_cycle(&mut game, 0, (5, 1), Direction::Up, &[]);
    game.cycles[0].held_powerup = Some(PowerUpKind::WallPunch);
    game.fire_powerup(0);
    assert_eq!(game.grid[0][5], CellType::Wall, "outer frame survives");
}

#[test]
fn test_corridor_pacman_traversal() {
    let mut game = WormGame::new();
    // Exit through a punched hole, travel the outer corridor (the pacman tunnel).
    place_cycle(&mut game, 0, (5, 3), Direction::Up, &[]);
    let (cw, ch) = (game.width / 2, game.height / 2);
    place_cycle(&mut game, 1, (cw, ch), Direction::Right, &[]);
    game.grid[2][5] = CellType::Hole;
    game.update();
    assert_eq!(game.cycles[0].head, (5, 2), "entered the hole");
    assert!(game.cycles[0].alive);
    game.update();
    assert_eq!(game.cycles[0].head, (5, 1), "into the outer corridor");
    assert!(game.cycles[0].alive);
    game.change_direction(Direction::Right);
    game.update();
    assert_eq!(game.cycles[0].head, (6, 1), "travelling the corridor ring");
    assert!(game.cycles[0].alive);
}

#[test]
fn test_restart_clears_powerup_state() {
    let mut game = WormGame::new();
    game.cycles[0].held_powerup = Some(PowerUpKind::Laser);
    game.bombs.push(Bomb { x: 10, y: 10, fuse: 5 });
    game.grid[2][5] = CellType::Hole;
    game.restart();
    assert!(game.bombs.is_empty());
    assert!(game.projectiles.is_empty());
    assert!(game.powerups.is_empty());
    assert_eq!(game.cycles[0].held_powerup, None);
    assert_eq!(game.grid[2][5], CellType::Wall, "holes sealed on restart");
}
