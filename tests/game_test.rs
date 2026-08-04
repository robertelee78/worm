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
