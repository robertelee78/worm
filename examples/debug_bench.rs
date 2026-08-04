use worm::game::WormGame;
use worm::cpu_ai;

fn main() {
    let mut game = WormGame::new();
    game.difficulty = 3;
    let mut brain = worm::CpuBrain::new();
    
    for frame in 0..500 {
        if game.game_over {
            break;
        }
        
        // Wall-follower player
        let dir = cpu_ai::wall_follow(&game);
        game.change_direction(dir);
        
        // CPU decides
        let pred = cpu_ai::predict_player_move(&game, &brain, &brain.player_tail);
        if frame % 50 == 0 {
            println!("Frame {}: pred_dir={:?} confidence={:.3} margin={:.3} support={:.3} maturity={:.3}", 
                frame, pred.predicted_dir, pred.confidence, pred.margin, pred.support, pred.maturity);
            println!("  Player head: {:?} | CPU head: {:?}", game.cycles[0].head, game.cycles[1].head);
        }
        
        let cpu_dir = cpu_ai::cpu_decide(&game, &mut brain, true, &mut |a, b| {
            use std::time::{SystemTime, UNIX_EPOCH};
            let seed = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64;
            let mut s = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            s = s ^ (s >> 13);
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let val = (s >> 33) as f32 / 2147483648.0;
            val * (b - a) + a
        });
        game.cycles[1].change_direction(cpu_dir);
        
        cpu_ai::record_player_episode(&game, &mut brain.opp_brain);
        cpu_ai::record_episode(&game, &mut brain, cpu_dir, frame);
        
        game.update();
    }
    
    println!("\nFinal: time={}, cpu_food={}, cpu_alive={}", game.time, game.cycles[1].score, game.cycles[1].alive);
}
