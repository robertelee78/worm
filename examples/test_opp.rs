use worm::*;

fn wall_follow(game: &WormGame, who: usize) -> Direction {
    let cpu = &game.cycles[who];
    let head = cpu.head;
    let current_dir = cpu.direction;
    let right_map = [(Direction::Up, Direction::Right), (Direction::Right, Direction::Down), (Direction::Down, Direction::Left), (Direction::Left, Direction::Up)];
    let left_map = [(Direction::Up, Direction::Left), (Direction::Left, Direction::Down), (Direction::Down, Direction::Right), (Direction::Right, Direction::Up)];
    let back_map = [(Direction::Up, Direction::Down), (Direction::Down, Direction::Up), (Direction::Left, Direction::Right), (Direction::Right, Direction::Left)];
    let right_dir = right_map.iter().find(|(d, _)| *d == current_dir).map(|(_, r)| *r).unwrap_or(current_dir);
    let left_dir = left_map.iter().find(|(d, _)| *d == current_dir).map(|(_, l)| *l).unwrap_or(current_dir);
    let back_dir = back_map.iter().find(|(d, _)| *d == current_dir).map(|(_, b)| *b).unwrap_or(current_dir);
    for dir in [right_dir, current_dir, left_dir, back_dir] {
        let (dx, dy) = dir.as_delta();
        let new_x = (head.0 as i16 + dx).max(1).min((game.width - 2) as i16) as u16;
        let new_y = (head.1 as i16 + dy).max(1).min((game.height - 2) as i16) as u16;
        if new_x >= 1 && new_x < game.width - 1 && new_y >= 1 && new_y < game.height - 1
            && game.grid[new_y as usize][new_x as usize] == CellType::Empty
        { return dir; }
    }
    current_dir
}

fn main() {
    let mut brain = CpuBrain::new();
    let mut game = WormGame::new();
    game.difficulty = 3;
    
    for i in 0..200 {
        if game.game_over { break; }
        
        let pdir = wall_follow(&game, 0);
        game.change_direction(pdir);
        
        let cpu_dir = wall_follow_decide(&game, &game.cycles[1]);
        game.cycles[1].change_direction(cpu_dir);
        
        let player_ctx = encode_player_context(&game);
        record_player_episode(&mut brain, player_ctx, game.cycles[0].direction);
        
        game.update();
        
        if i % 50 == 0 {
            let tail = brain.player_tail.clone();
            let pred = predict_player_move(&game, &brain, &tail);
            println!("Frame {}: pred={:?} conf={:.3} margin={:.3} support={:.3} opp_episodes={}",
                i, pred.predicted_dir, pred.confidence, pred.margin, pred.support, brain.opp_brain.episodes.len());
        }
    }
}
