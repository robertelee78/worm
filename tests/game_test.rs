use worm::WormGame;

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

/// Regression: a tri-shot bolt spawns on the firer's head and advances into the
/// cell the firer's head enters the same frame. Bolts must never hit their own
/// firer — previously firing while moving straight killed you instantly.
#[test]
fn test_tri_shot_does_not_self_kill() {
    let mut game = WormGame::new();
    // Clean arena, park both cycles on far-apart rows, pin food away.
    for row in &mut game.grid {
        for cell in row.iter_mut() {
            if *cell != worm::CellType::Wall {
                *cell = worm::CellType::Empty;
            }
        }
    }
    game.cycles[0].head = (10, 15);
    game.cycles[0].positions = vec![(10, 15)];
    game.cycles[0].direction = worm::Direction::Right;
    game.grid[15][10] = worm::CellType::Player;
    game.cycles[1].head = (5, 5);
    game.cycles[1].positions = vec![(5, 5)];
    game.cycles[1].direction = worm::Direction::Right;
    game.grid[5][5] = worm::CellType::CPU;
    game.food_items.clear();
    game.food_items.push((20, 10, 1));
    game.grid[10][20] = worm::CellType::Food;

    game.cycles[0].held_powerup = Some(worm::game::PowerUpKind::TriShot);
    assert!(game.fire_powerup(0));
    assert_eq!(game.projectiles.len(), 3, "tri-shot spawns three bolts");

    for _ in 0..8 {
        game.update();
        assert!(game.cycles[0].alive, "player must survive its own bolts");
        assert!(!game.game_over, "tri-shot must not self-kill the firer");
    }
    assert!(
        game.projectiles.is_empty(),
        "bolts expire at TRI_SHOT_RANGE without touching the firer"
    );
}

/// Regression: when both cycles step into each other's head cells in the same
/// frame, both must die (DRAW). The sequential player-first crash check used to
/// kill only the player and hand the CPU the win.
#[test]
fn test_head_on_collision_is_draw() {
    let mut game = WormGame::new();
    for row in &mut game.grid {
        for cell in row.iter_mut() {
            if *cell != worm::CellType::Wall {
                *cell = worm::CellType::Empty;
            }
        }
    }
    // Player heads Right into the CPU's head; the CPU heads Left into the
    // player's head — a classic head-on.
    game.cycles[0].head = (10, 15);
    game.cycles[0].positions = vec![(10, 15)];
    game.cycles[0].direction = worm::Direction::Right;
    game.grid[15][10] = worm::CellType::Player;
    game.cycles[1].head = (11, 15);
    game.cycles[1].positions = vec![(11, 15)];
    game.cycles[1].direction = worm::Direction::Left;
    game.grid[15][11] = worm::CellType::CPU;
    game.food_items.clear();
    game.food_items.push((20, 10, 1));
    game.grid[10][20] = worm::CellType::Food;

    game.update();
    assert!(game.game_over);
    assert_eq!(game.winner, None, "head-on must be a draw, not a CPU win");
    assert!(!game.cycles[0].alive && !game.cycles[1].alive);
}
