use worm::WormGame;

#[test]
fn test_new_game_initial_state() {
    let game = WormGame::with_size(120, 38);
    assert_eq!(game.cycles.len(), 2);
    assert_eq!(game.cycles[0].positions.len(), 1);
    assert!(!game.game_over);
    assert_eq!(game.score, 0);
}

#[test]
fn test_snake_movement() {
    let mut game = WormGame::with_size(120, 38);
    let original_head = game.cycles[0].head;
    game.update();
    assert_ne!(game.cycles[0].head, original_head);
}

#[test]
fn test_food_collection_increases_score() {
    let mut game = WormGame::with_size(120, 38);
    let head = game.cycles[0].head;
    game.food_items.clear();
    game.food_items.push((head.0 + 1, head.1, 1));
    game.cycles[0].direction = worm::Direction::Right;
    let old_score = game.score;
    game.update();
    assert!(game.score > old_score);
    assert_eq!(
        game.food_eaten_by,
        [1, 0],
        "food HUD counters are symmetric"
    );
}

#[test]
fn test_wall_collision() {
    let mut game = WormGame::with_size(120, 38);
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
    let mut game = WormGame::with_size(120, 38);
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
    let mut game = WormGame::with_size(120, 38);
    game.cycles[0].direction = worm::Direction::Right;
    game.cycles[0].change_direction(worm::Direction::Left);
    assert_eq!(game.cycles[0].direction, worm::Direction::Right);
}

#[test]
fn test_food_items_are_on_grid() {
    let game = WormGame::with_size(120, 38);
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
    let mut game = WormGame::with_size(120, 38);
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
    let mut game = WormGame::with_size(120, 38);
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
    let mut game = WormGame::with_size(120, 38);
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
    let mut game = WormGame::with_size(120, 38);
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
    let mut game = WormGame::with_size(120, 38);
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

/// Regression: a planted bomb must never kill its own planter (mirrors the
/// tri-shot `from` exclusion). Previously the player died to their own bomb —
/// and, worse, instantly when a laser detonated it, voiding the 3s fuse.
#[test]
fn test_bomb_never_kills_its_planter() {
    let mut game = WormGame::with_size(120, 38);
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
    game.cycles[1].head = (70, 5);
    game.cycles[1].positions = vec![(70, 5)];
    game.cycles[1].direction = worm::Direction::Right;
    game.grid[5][70] = worm::CellType::CPU;
    game.food_items.clear();
    game.food_items.push((20, 10, 1));
    game.grid[10][20] = worm::CellType::Food;

    // Player plants a bomb on their own head cell.
    game.cycles[0].held_powerup = Some(worm::game::PowerUpKind::Bomb);
    assert!(game.fire_powerup(0));
    assert_eq!(game.bombs.len(), 1);
    assert_eq!(game.bombs[0].owner, 0, "bomb must carry the planter's id");

    // Force the fuse to detonate next tick while the player is still inside
    // the Chebyshev blast radius (distance 0 — right on top of it).
    game.bombs[0].fuse = 1;
    game.tick_bombs();
    assert!(game.cycles[0].alive, "planter must survive its own bomb");
    assert!(!game.game_over, "own-bomb blast must not end the game");
    assert!(game.bombs.is_empty(), "bomb detonated");
}

/// Regression: firing a laser through your OWN planted bomb must not self-kill.
/// The beam detonates the bomb (voiding its fuse), and the blast radius covers
/// the firer — the owner exclusion must keep the planter alive.
#[test]
fn test_laser_detonating_own_bomb_does_not_self_kill() {
    let mut game = WormGame::with_size(120, 38);
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
    // CPU far away (out of beam row and out of the 10-cell blast radius).
    game.cycles[1].head = (70, 5);
    game.cycles[1].positions = vec![(70, 5)];
    game.cycles[1].direction = worm::Direction::Right;
    game.grid[5][70] = worm::CellType::CPU;
    game.food_items.clear();
    game.food_items.push((20, 10, 1));
    game.grid[10][20] = worm::CellType::Food;

    // Player's own bomb sits 5 cells ahead in the beam path.
    game.bombs.push(worm::game::Bomb {
        x: 15,
        y: 15,
        fuse: 999,
        owner: 0,
    });
    game.cycles[0].held_powerup = Some(worm::game::PowerUpKind::Laser);

    assert!(game.fire_powerup(0));
    assert!(
        game.bombs.is_empty(),
        "laser detonates the bomb in its path"
    );
    assert!(
        game.cycles[0].alive,
        "planter must survive the beam-triggered blast"
    );
    assert!(
        !game.game_over,
        "own-bomb laser chain must not end the game"
    );
}

/// Regression: when a bolt kills the CPU and a bomb kills the player in the
/// SAME frame, the game is a DRAW — the later event must not overwrite the
/// earlier kill's winner (previously the bomb credited the CPU with a win it
/// never earned).
#[test]
fn test_same_frame_kills_are_a_draw() {
    let mut game = WormGame::with_size(120, 38);
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
    game.cycles[1].head = (12, 15);
    game.cycles[1].positions = vec![(12, 15)];
    game.cycles[1].direction = worm::Direction::Right;
    game.grid[15][12] = worm::CellType::CPU;
    game.food_items.clear();
    game.food_items.push((20, 10, 1));
    game.grid[10][20] = worm::CellType::Food;

    // Deterministic: no update()/AI — just advance both bolts in one frame.
    // Player bolt (from 0): one cell left of the CPU head, will hit (12,15).
    game.projectiles.push(worm::game::Projectile {
        x: 11,
        y: 15,
        dx: 1,
        dy: 0,
        steps_left: 5,
        from: 0,
    });
    // CPU bolt (from 1): one cell left of the PLAYER head, will hit (10,15).
    game.projectiles.push(worm::game::Projectile {
        x: 9,
        y: 15,
        dx: 1,
        dy: 0,
        steps_left: 5,
        from: 1,
    });

    game.advance_projectiles();
    assert!(game.game_over);
    assert_eq!(game.winner, None, "both heads died this frame -> draw");
    assert!(!game.cycles[0].alive && !game.cycles[1].alive);
}

/// Speed is earned by eating, not by the clock: a fresh game opens at a
/// relaxed 115ms and each food value point shaves time off the frame down to
/// the 35ms floor. Saturates no matter how much is eaten.
#[test]
fn test_frame_delay_is_food_driven() {
    let mut game = WormGame::with_size(120, 38);
    assert_eq!(game.frame_delay(), std::time::Duration::from_millis(115));
    assert_eq!(game.speed_pct(), 0);
    game.food_eaten_total = 20; // 10 value-points' worth of shaving
    assert_eq!(game.frame_delay(), std::time::Duration::from_millis(105));
    game.food_eaten_total = 160;
    assert_eq!(game.frame_delay(), std::time::Duration::from_millis(35));
    assert_eq!(game.speed_pct(), 100);
    game.food_eaten_total = u32::MAX; // must saturate, never panic/wrap
    assert_eq!(game.frame_delay(), std::time::Duration::from_millis(35));
}

/// The displayed scoreboard counts the game that JUST ended (the banked
/// counter updates at restart, one game late — the stale champion bug).
#[test]
fn test_displayed_wins_include_current_game() {
    let mut game = WormGame::with_size(120, 38);
    game.winner = Some(1);
    game.game_over = true;
    assert_eq!(game.displayed_wins(), [0, 1]);
    game.restart(); // banks the win
    assert_eq!(game.displayed_wins(), [0, 1]);
    game.winner = Some(1);
    game.game_over = true;
    assert_eq!(game.displayed_wins(), [0, 2]);
}

/// Regression: snapshot_direction is now called every frame so prev_direction
/// tracks the direction actually moved last frame. Between an external turn
/// and the next update, prev_direction must still hold the pre-turn heading.
#[test]
fn test_prev_direction_snapshots_each_frame() {
    let mut game = WormGame::with_size(120, 38);
    game.update(); // player moves Right (initial heading)
    assert_eq!(game.cycles[0].prev_direction, worm::Direction::Right);
    game.change_direction(worm::Direction::Down); // legal from Right
    assert_eq!(game.cycles[0].direction, worm::Direction::Down);
    assert_eq!(
        game.cycles[0].prev_direction,
        worm::Direction::Right,
        "prev_direction holds the last executed heading until the next frame"
    );
}

/// Regression: the survival reward counter was incremented then immediately
/// reset EVERY frame, so every episode recorded survived_frames == 1 and the
/// signal was dead. It must accumulate while the heading is unchanged.
#[test]
fn test_cpu_survival_reward_accumulates_on_straights() {
    let mut game = WormGame::with_size(120, 38);
    // Clean arena, then wall off above/below the CPU's row so wall-follow
    // (cold-start policy) drives straight Right without turning.
    for row in &mut game.grid {
        for cell in row.iter_mut() {
            if *cell != worm::CellType::Wall {
                *cell = worm::CellType::Empty;
            }
        }
    }
    game.cycles[1].head = (10, 10);
    game.cycles[1].positions = vec![(10, 10)];
    game.cycles[1].direction = worm::Direction::Right;
    game.grid[10][10] = worm::CellType::CPU;
    for x in 10..=15 {
        game.grid[9][x] = worm::CellType::Wall;
        game.grid[11][x] = worm::CellType::Wall;
    }
    // Player circles harmlessly far away; one pinned food keeps the tray full.
    let prow = game.height - 6;
    game.cycles[0].head = (70, prow);
    game.cycles[0].positions = vec![(70, prow)];
    game.cycles[0].direction = worm::Direction::Right;
    game.grid[prow as usize][70] = worm::CellType::Player;
    game.food_items.clear();
    game.food_items.push((4, prow, 1));
    game.grid[prow as usize][4] = worm::CellType::Food;

    for _ in 0..3 {
        game.update();
        assert!(!game.game_over, "setup must keep both cycles alive");
    }
    let rewards: Vec<f32> = game.cpu_brain.episodes.iter().map(|e| e.reward).collect();
    assert_eq!(
        rewards,
        vec![1.0, 2.0, 3.0],
        "survival reward must accumulate per frame on an unchanged heading"
    );
}

/// Regression: the player tail was pushed BEFORE encode_player_context, so
/// the recorded context's transition matrix contained the very direction
/// being stored as the label. With the ordering fixed, a straight-running
/// player's second episode has zero transition mass (tail has one entry —
/// no adjacent pair exists yet).
#[test]
fn test_player_context_has_no_label_leakage() {
    let mut game = WormGame::with_size(120, 38);
    game.update(); // player moves Right; episode 0 from an empty tail
    game.update(); // player still Right; episode 1 from a 1-entry tail
    assert_eq!(game.cpu_brain.opp_brain.episodes.len(), 2);
    for (i, e) in game.cpu_brain.opp_brain.episodes.iter().enumerate() {
        let transition_mass: f32 = e.vector[13..29].iter().sum();
        assert_eq!(
            transition_mass, 0.0,
            "episode {i} must not encode a transition pair ending in its own label"
        );
        assert_eq!(e.next_dir, worm::Direction::Right);
    }
}

/// Regression: the threat gate protected only wall-follow — the tier-1 food
/// grab could then step onto a cell with a projectile landing on it. With a
/// warm brain (past cold start), the CPU must not take threatened food.
#[test]
fn test_cpu_decide_avoids_threatened_food() {
    let mut game = WormGame::with_size(120, 38);
    for row in &mut game.grid {
        for cell in row.iter_mut() {
            if *cell != worm::CellType::Wall {
                *cell = worm::CellType::Empty;
            }
        }
    }
    game.cycles[1].head = (10, 10);
    game.cycles[1].positions = vec![(10, 10)];
    game.cycles[1].direction = worm::Direction::Right;
    game.grid[10][10] = worm::CellType::CPU;
    let prow = game.height - 6;
    game.cycles[0].head = (70, prow);
    game.cycles[0].positions = vec![(70, prow)];
    game.cycles[0].direction = worm::Direction::Right;
    game.grid[prow as usize][70] = worm::CellType::Player;
    // Food directly ahead (adjacent — tier 1 would grab it pre-fix).
    game.food_items.clear();
    game.food_items.push((11, 10, 5));
    game.grid[10][11] = worm::CellType::Food;
    // Player bolt flying left along row 10 — lands on (11,10) and (12,10).
    game.projectiles.push(worm::game::Projectile {
        x: 13,
        y: 10,
        dx: -1,
        dy: 0,
        steps_left: 7,
        from: 0,
    });
    // Warm the brain past the cold-start gate so the food layer runs.
    let mut v = [0.0f32; worm::cpu_ai::CPU_FEATURE_DIM];
    v[0] = 1.0;
    for _ in 0..worm::cpu_ai::CPU_FEATURE_DIM * 3 {
        game.cpu_brain.remember(v, worm::Direction::Up, 1.0);
    }

    let decision = worm::cpu_decide(&mut game);
    assert_ne!(
        decision,
        worm::Direction::Right,
        "must not step onto threatened food (bolt lands there)"
    );
}

/// Regression: with cpu_autopilot = false (benchmark scripted opponents),
/// update() must not override the externally-steered heading with cpu_decide
/// and must not record self-episodes into the learner's brain. Cold-start
/// wall-follow would turn Down here, so holding Right proves the steer wins.
#[test]
fn test_cpu_autopilot_false_keeps_scripted_heading() {
    let mut game = WormGame::with_size(120, 38);
    for row in &mut game.grid {
        for cell in row.iter_mut() {
            if *cell != worm::CellType::Wall {
                *cell = worm::CellType::Empty;
            }
        }
    }
    game.cycles[1].head = (10, 10);
    game.cycles[1].positions = vec![(10, 10)];
    game.cycles[1].direction = worm::Direction::Right;
    game.grid[10][10] = worm::CellType::CPU;
    let prow = game.height - 6;
    game.cycles[0].head = (70, prow);
    game.cycles[0].positions = vec![(70, prow)];
    game.cycles[0].direction = worm::Direction::Right;
    game.grid[prow as usize][70] = worm::CellType::Player;
    game.food_items.clear();
    game.food_items.push((4, prow, 1));
    game.grid[prow as usize][4] = worm::CellType::Food;
    game.cpu_autopilot = false;

    game.update();
    assert_eq!(
        game.cycles[1].direction,
        worm::Direction::Right,
        "scripted heading must not be overridden by cpu_decide"
    );
    assert!(
        game.cpu_brain.episodes.is_empty(),
        "scripted CPU leaves no self-episodes in the brain"
    );
    assert!(game.cycles[1].alive);
}

/// The HUD's "CPU action" is instrumentation of the actual decision layer,
/// not a relabeling of whichever prediction model currently ranks highest.
#[test]
fn test_cpu_decision_reason_reports_actual_layer() {
    let mut game = WormGame::with_size(120, 38);
    let _ = worm::cpu_decide(&mut game);
    assert_eq!(
        game.cpu_decision_reason,
        worm::cpu_ai::CpuDecisionReason::WarmingUp
    );
    assert!(
        game.cpu_predicted_path.is_empty(),
        "cold models do not project a path"
    );
}

/// The opponent-model accuracy counter: last frame's prediction is scored
/// against this frame's actual move. A player that holds one direction is
/// maximally predictable — after the prior sees a few moves the hit rate
/// must clear the 25% chance floor by a wide margin.
#[test]
fn test_opp_prediction_accuracy_tracking() {
    let mut game = WormGame::with_size(120, 38);
    // Warm the self-brain past the cold-start gate so the prediction block
    // in cpu_decide actually runs (it early-returns while cold).
    let mut v = [0.0f32; worm::cpu_ai::CPU_FEATURE_DIM];
    v[0] = 1.0;
    for _ in 0..60 {
        game.cpu_brain.remember(v, worm::Direction::Up, 1.0);
    }
    // Player holds Right (its initial direction) in the open arena.
    for _ in 0..6 {
        game.update();
        assert!(!game.game_over, "setup must keep both cycles alive");
    }
    let b = &game.cpu_brain;
    assert_eq!(b.opp_pred_total, 5, "frames 2..=6 are scored");
    assert!(
        b.opp_pred_hits >= 4,
        "constant-direction player must be predicted reliably (hits={})",
        b.opp_pred_hits
    );
    assert!(b.opp_pred_accuracy() > 0.5);
    assert_eq!(b.last_opp_prediction, Some(worm::Direction::Right));
    assert_eq!(game.round_pred_total, 5, "round evidence has its own scope");
    assert_eq!(game.round_pred_hits, b.opp_pred_hits);
    assert_eq!(game.last_scored_prediction, Some(worm::Direction::Right));
    assert_eq!(game.last_player_actual, Some(worm::Direction::Right));
    assert_eq!(game.last_prediction_hit, Some(true));
}

/// The ensemble: a player holding one direction must be caught by the cheap
/// models immediately (no 60-frame warm-up like the k-NN) — `frq` should be
/// perfect from frame 2 and drive the prediction. Counterfactual scoring:
/// every model is scored every frame, not just the driver.
#[test]
fn test_ensemble_catches_constant_player_immediately() {
    let mut game = WormGame::with_size(120, 38);
    for _ in 0..8 {
        game.update(); // player holds Right (initial heading)
        assert!(!game.game_over);
    }
    let e = &game.cpu_brain.ensemble;
    assert_eq!(e.total[2], 7, "frq scored from frame 2 (frames 2..=8)");
    assert_eq!(e.hits[2], 7, "frq is perfect against a constant player");
    assert!(
        e.score(2) > 0.9,
        "quadratic score saturates: {:.3}",
        e.score(2)
    );
    // Several models are perfect against a constant player (pat/frq/wall all
    // predict Right); the driver is whichever perfect model wins the tie —
    // what matters is that the DRIVER is near-perfect and predicts Right.
    assert!(
        e.score(e.active) > 0.9,
        "the driving model is near-perfect ({}@{} = {:.3})",
        worm::cpu_ai::MODEL_NAMES[e.active],
        e.active,
        e.score(e.active)
    );
    assert_eq!(e.predicted_dir, Some(worm::Direction::Right));
    // Counterfactual: the wall models also predicted straight every frame and
    // must have been scored even though they never drove.
    assert!(e.total[4] >= 7, "wlR was scored without driving");
}

/// rps-ai wipes its per-game record on restart: ensemble scores reset, while
/// the k-NN memory beneath (the corpus) persists. Session wins are banked.
#[test]
fn test_ensemble_scores_reset_on_restart() {
    let mut game = WormGame::with_size(120, 38);
    for _ in 0..6 {
        game.update();
    }
    assert!(game.cpu_brain.ensemble.total[2] > 0);
    let opp_before = game.cpu_brain.opp_brain.episodes.len();
    let lifetime_predictions_before = game.cpu_brain.opp_pred_total;
    assert!(opp_before > 0);

    game.winner = Some(1);
    game.restart();

    let e = &game.cpu_brain.ensemble;
    assert!(e.den.iter().all(|&d| d == 0.0), "scores wiped per game");
    assert!(
        e.total.iter().all(|&t| t == 0),
        "hit counters wiped per game"
    );
    assert_eq!(e.predicted_dir, None);
    assert_eq!(game.round_pred_total, 0, "round evidence resets");
    assert_eq!(game.round_pred_hits, 0, "round evidence resets");
    assert_eq!(game.last_scored_prediction, None);
    assert_eq!(game.last_player_actual, None);
    assert_eq!(game.last_prediction_hit, None);
    assert_eq!(game.food_eaten_by, [0, 0]);
    assert_eq!(
        game.cpu_brain.opp_pred_total, lifetime_predictions_before,
        "lifetime prediction evidence persists"
    );
    assert_eq!(
        game.cpu_decision_reason,
        worm::cpu_ai::CpuDecisionReason::Opening
    );
    assert!(game.cpu_predicted_path.is_empty());
    assert_eq!(
        game.cpu_brain.opp_brain.episodes.len(),
        opp_before,
        "k-NN memory persists across games"
    );
    assert_eq!(
        game.session_wins[1], 1,
        "finished game banked to scoreboard"
    );
}

/// The sophisticated model abstains while cold and joins once warm —
/// rps-ai's NN/DT needed 5-7 rounds; ours needs COLD_START_EPISODES.
#[test]
fn test_knn_model_abstains_cold_joins_warm() {
    let mut game = WormGame::with_size(120, 38);
    let (pending, _, _) = worm::cpu_ai::compute_ensemble(&game, &game.cpu_brain);
    assert_eq!(pending[6], None, "knn abstains with an empty memory");

    let ctx = [0.1f32; worm::cpu_ai::PLAYER_FEATURE_DIM];
    for _ in 0..60 {
        worm::record_player_episode(&mut game.cpu_brain, ctx, worm::Direction::Right);
    }
    let (pending, _, _) = worm::cpu_ai::compute_ensemble(&game, &game.cpu_brain);
    assert!(pending[6].is_some(), "knn predicts once warm");
}

/// Item seeking, tier 1: an adjacent power-up is grabbed (the CPU used to
/// walk straight past power-ups). Placed UP so wall-follow (Down) would miss
/// it — a discriminating direction.
#[test]
fn test_cpu_grabs_adjacent_powerup() {
    let mut game = WormGame::with_size(120, 38);
    for row in &mut game.grid {
        for cell in row.iter_mut() {
            if *cell != worm::CellType::Wall {
                *cell = worm::CellType::Empty;
            }
        }
    }
    game.cycles[1].head = (10, 10);
    game.cycles[1].positions = vec![(10, 10)];
    game.cycles[1].direction = worm::Direction::Right;
    game.grid[10][10] = worm::CellType::CPU;
    let prow = game.height - 6;
    game.cycles[0].head = (70, prow);
    game.cycles[0].positions = vec![(70, prow)];
    game.cycles[0].direction = worm::Direction::Right;
    game.grid[prow as usize][70] = worm::CellType::Player;
    game.food_items.clear();
    game.powerups.clear();
    game.powerups.push((10, 9, worm::game::PowerUpKind::Laser));
    game.grid[9][10] = worm::CellType::PowerUp;
    // Warm brain past cold start; stub points Down so the self-vote agrees
    // with wall-follow and cannot mask the item grab.
    let mut v = [0.0f32; worm::cpu_ai::CPU_FEATURE_DIM];
    v[0] = 1.0;
    for _ in 0..60 {
        game.cpu_brain.remember(v, worm::Direction::Down, 1.0);
    }
    assert_eq!(worm::cpu_decide(&mut game), worm::Direction::Up);
}

/// Item seeking, tier 3: BFS routes to a distant power-up when the path is
/// open (previously food-only — power-ups were never sought).
#[test]
fn test_cpu_seeks_distant_powerup() {
    let mut game = WormGame::with_size(120, 38);
    for row in &mut game.grid {
        for cell in row.iter_mut() {
            if *cell != worm::CellType::Wall {
                *cell = worm::CellType::Empty;
            }
        }
    }
    game.cycles[1].head = (10, 10);
    game.cycles[1].positions = vec![(10, 10)];
    game.cycles[1].direction = worm::Direction::Right;
    game.grid[10][10] = worm::CellType::CPU;
    let prow = game.height - 6;
    game.cycles[0].head = (70, prow);
    game.cycles[0].positions = vec![(70, prow)];
    game.cycles[0].direction = worm::Direction::Right;
    game.grid[prow as usize][70] = worm::CellType::Player;
    game.food_items.clear();
    game.powerups.clear();
    game.powerups.push((14, 7, worm::game::PowerUpKind::Laser));
    game.grid[7][14] = worm::CellType::PowerUp;
    let mut v = [0.0f32; worm::cpu_ai::CPU_FEATURE_DIM];
    v[0] = 1.0;
    for _ in 0..60 {
        game.cpu_brain.remember(v, worm::Direction::Down, 1.0);
    }
    // wall-follow is Down; the power-up is up-right — BFS seeds Up first.
    assert_eq!(worm::cpu_decide(&mut game), worm::Direction::Up);
}

/// Anti-kamikaze: with the defensive layer outranking the hunt layers, a
/// head-on approach must end in a sidestep, not a collision. The CPU is
/// boxed into a leftward corridor (walled above) heading straight at the
/// oncoming player.
#[test]
fn test_cpu_dodges_head_on() {
    let mut game = WormGame::with_size(120, 38);
    for row in &mut game.grid {
        for cell in row.iter_mut() {
            if *cell != worm::CellType::Wall {
                *cell = worm::CellType::Empty;
            }
        }
    }
    // Corridor wall above the collision row.
    for x in 30..=46 {
        game.grid[9][x] = worm::CellType::Wall;
    }
    game.cycles[1].head = (40, 10);
    game.cycles[1].positions = vec![(40, 10)];
    game.cycles[1].direction = worm::Direction::Left;
    game.grid[10][40] = worm::CellType::CPU;
    game.cycles[0].head = (34, 10);
    game.cycles[0].positions = vec![(34, 10)];
    game.cycles[0].direction = worm::Direction::Right;
    game.grid[10][34] = worm::CellType::Player;
    game.food_items.clear();
    game.food_items.push((4, game.height - 6, 1));
    game.grid[(game.height - 6) as usize][4] = worm::CellType::Food;
    // Warm brain so the prediction/defensive layers run immediately.
    let mut v = [0.0f32; worm::cpu_ai::CPU_FEATURE_DIM];
    v[0] = 1.0;
    for _ in 0..60 {
        game.cpu_brain.remember(v, worm::Direction::Left, 1.0);
    }
    for _ in 0..6 {
        game.update();
        if game.game_over {
            break;
        }
    }
    // The kamikaze outcome is the CPU dying (player win or mutual draw).
    // A live CPU — game running, or won by the player crashing into the
    // CPU's trail after the dodge — is the anti-kamikaze success case.
    assert!(
        game.cycles[1].alive && game.winner != Some(0),
        "defensive dodge must keep the CPU alive (winner={:?}, cpu_dir={:?})",
        game.winner,
        game.cycles[1].direction
    );
}

/// Death cause is recorded: wall crash and bomb blast are distinguishable on
/// the game-over screen.
#[test]
fn test_death_cause_wall_vs_bomb() {
    let mut game = WormGame::with_size(120, 38);
    let head = (1, game.height / 2);
    game.cycles[0].head = head;
    game.cycles[0].positions.clear();
    game.cycles[0].positions.push(head);
    game.cycles[0].direction = worm::Direction::Left;
    game.grid[game.cycles[0].head.1 as usize][1] = worm::CellType::Wall;
    game.update();
    assert_eq!(game.death_cause, Some(worm::game::DeathCause::Wall));

    let mut game = WormGame::with_size(120, 38);
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
    game.cycles[1].head = (70, 5);
    game.cycles[1].positions = vec![(70, 5)];
    game.grid[5][70] = worm::CellType::CPU;
    game.food_items.clear();
    game.food_items.push((20, 10, 1));
    game.grid[10][20] = worm::CellType::Food;
    // CPU's bomb detonates next tick next to the player's head.
    game.bombs.push(worm::game::Bomb {
        x: 12,
        y: 15,
        fuse: 1,
        owner: 1,
    });
    game.tick_bombs();
    assert!(game.game_over);
    assert_eq!(game.death_cause, Some(worm::game::DeathCause::BombBlast));
}

/// Smoke: render must not panic across a few frames (it does per-cell index
/// math for trails, heads, bombs, particles and the HUD).
#[test]
fn test_render_smoke() {
    let mut game = WormGame::with_size(120, 38);
    let mut out = std::io::stdout();
    for _ in 0..5 {
        game.update();
        game.render(&mut out);
    }
    // Plus a game-over render.
    game.game_over = true;
    game.render(&mut out);
}

/// Browser sfx wire protocol: [kind, freq_hz, dur_ms, delay_ms] quads, with
/// `kind` = worm::game::SfxKind (the JS-side patch contract — see the
/// protocol comment in game.rs). The queue itself is wasm-only; the wire
/// formatter + kind discriminants are the native-testable surface.
#[test]
fn test_sfx_protocol_wire_format() {
    use worm::game::{format_sfx_json, SfxKind};
    // Kind discriminants are the JS contract — pin every one.
    assert_eq!(SfxKind::Food as u8, 0);
    assert_eq!(SfxKind::PowerUp as u8, 1);
    assert_eq!(SfxKind::Laser as u8, 2);
    assert_eq!(SfxKind::TriShot as u8, 3);
    assert_eq!(SfxKind::BombPlant as u8, 4);
    assert_eq!(SfxKind::Detonate as u8, 5);
    assert_eq!(SfxKind::WallPunch as u8, 6);
    assert_eq!(SfxKind::DeathRiff as u8, 7);
    // A typed quad serializes with the kind first; an empty drain is "[]".
    let events = [(SfxKind::DeathRiff as u8, 440, 100, 0)];
    assert_eq!(format_sfx_json(&events), "[[7,440,100,0]]");
    assert_eq!(format_sfx_json(&[]), "[]");
}

/* ---- swarm-audit regression tests (2026-08) ---- */

#[test]
fn test_player_crash_is_a_cpu_win_when_cpu_can_turn() {
    // The CPU's straight-ahead cell is blocked but a side turn is open — a
    // routine wall-follow turn frame. The player crashing that frame must
    // score a CPU win, not a draw (the old check probed only the heading).
    let mut game = WormGame::with_size(120, 38);
    game.food_items.clear();
    game.cycles[0].head = (10, 10);
    game.cycles[0].positions = vec![(10, 10)];
    game.cycles[0].direction = worm::Direction::Left;
    game.grid[10][9] = worm::CellType::Wall;
    game.cycles[1].head = (30, 20);
    game.cycles[1].positions = vec![(30, 20)];
    game.cycles[1].direction = worm::Direction::Right;
    game.grid[20][31] = worm::CellType::Wall;
    game.update();
    assert!(game.game_over);
    assert_eq!(
        game.winner,
        Some(1),
        "the CPU could turn Up or Down and survive — its win must not be scored a draw"
    );
    assert!(game.cycles[1].alive);
}

#[test]
fn test_player_crash_is_a_draw_when_cpu_is_boxed_in() {
    // No non-reverse CPU escape: straight, up and down all blocked. Both die
    // this frame — a genuine draw.
    let mut game = WormGame::with_size(120, 38);
    game.food_items.clear();
    game.cycles[0].head = (10, 10);
    game.cycles[0].positions = vec![(10, 10)];
    game.cycles[0].direction = worm::Direction::Left;
    game.grid[10][9] = worm::CellType::Wall;
    game.cycles[1].head = (30, 20);
    game.cycles[1].positions = vec![(30, 20)];
    game.cycles[1].direction = worm::Direction::Right;
    game.grid[20][31] = worm::CellType::Wall;
    game.grid[19][30] = worm::CellType::Wall;
    game.grid[21][30] = worm::CellType::Wall;
    game.update();
    assert!(game.game_over);
    assert_eq!(game.winner, None, "a truly boxed-in CPU dies too — draw");
    assert!(!game.cycles[1].alive);
}

#[test]
fn test_player_survives_entering_cpu_vacating_tail_cell() {
    // The CPU's tail-tip vacates this same frame; the CPU's own crash check
    // (running after both retractions) survives the mirror move, so the
    // player must too.
    let mut game = WormGame::with_size(120, 38);
    game.cpu_autopilot = false;
    game.food_items.clear();
    game.cycles[1].head = (20, 10);
    game.cycles[1].direction = worm::Direction::Left;
    game.cycles[1].positions = vec![(20, 10), (21, 10), (22, 10)];
    for &(x, y) in &[(20u16, 10u16), (21, 10), (22, 10)] {
        game.grid[y as usize][x as usize] = worm::CellType::CPU;
    }
    game.cycles[0].head = (22, 11);
    game.cycles[0].direction = worm::Direction::Up;
    game.cycles[0].positions = vec![(22, 11)];
    game.grid[11][22] = worm::CellType::Player;
    game.update();
    assert!(
        !game.game_over,
        "entering a same-frame-vacated tail cell is safe for the player"
    );
    assert_eq!(game.cycles[0].head, (22, 10));
    assert_eq!(
        game.grid[10][22],
        worm::CellType::Player,
        "the CPU's tail pop must not erase the player's head marker"
    );
}

#[test]
fn test_bomb_blast_trims_positions_with_grid() {
    // detonate() clears trail cells from the grid; the owning cycle's
    // positions must shrink in lockstep, and a surviving owner's head marker
    // must not be swept.
    let mut game = WormGame::with_size(120, 38);
    game.food_items.clear();
    // Player trail from (25,10) to a head at (50,10); the head is outside
    // the blast radius, a long stretch of trail is inside.
    let positions: Vec<(u16, u16)> = (25..=50u16).rev().map(|x| (x, 10)).collect();
    for &(x, y) in &positions {
        game.grid[y as usize][x as usize] = worm::CellType::Player;
    }
    game.cycles[0].head = (50, 10);
    game.cycles[0].positions = positions;
    // The bomb's owner (CPU) sits inside its own blast: it must survive AND
    // keep its head marker on the grid.
    game.cycles[1].head = (35, 10);
    game.cycles[1].positions = vec![(35, 10)];
    game.grid[10][35] = worm::CellType::CPU;
    game.bombs.push(worm::game::Bomb {
        x: 32,
        y: 10,
        fuse: 1,
        owner: 1,
    });
    game.tick_bombs();
    assert!(!game.game_over, "no head dies: player out of range, CPU is the owner");
    for x in 25..=42u16 {
        if x == 35 {
            continue; // the owner's living head cell
        }
        assert_eq!(
            game.grid[10][x as usize],
            worm::CellType::Empty,
            "blast must clear trail cell ({x},10)"
        );
        assert!(
            !game.cycles[0].positions.contains(&(x, 10)),
            "cleared cell ({x},10) must leave positions too"
        );
    }
    for x in 43..=50u16 {
        assert_eq!(game.grid[10][x as usize], worm::CellType::Player);
        assert!(game.cycles[0].positions.contains(&(x, 10)));
    }
    assert_eq!(
        game.grid[10][35],
        worm::CellType::CPU,
        "a surviving owner's head marker must not be swept from the grid"
    );
    assert!(game.cycles[1].alive);
}

#[test]
fn test_bolt_hits_head_on_swap_crossing() {
    // Post-move state: the player's head moved Left (11,10)->(10,10) while
    // the bolt at (10,10) moves Right — they exchange cells in one frame.
    // Post-move-only comparison tunneled straight through.
    let mut game = WormGame::with_size(120, 38);
    game.cycles[0].head = (10, 10);
    game.cycles[0].positions = vec![(10, 10), (11, 10)];
    game.projectiles.push(worm::game::Projectile {
        x: 10,
        y: 10,
        dx: 1,
        dy: 0,
        steps_left: 5,
        from: 1,
    });
    game.advance_projectiles();
    assert!(game.game_over, "a crossing swap is a hit, not a miss");
    assert_eq!(game.winner, Some(1));
    assert!(!game.cycles[0].alive);
}

#[test]
fn test_no_180_via_two_quick_turns_in_one_tick() {
    // Moving Right; Up then Left arrive within the same tick. The latch
    // compares against the direction actually moved (prev_direction), so
    // Left — a net 180 into the neck cell — must be rejected while Up stands.
    let mut game = WormGame::with_size(120, 38);
    game.change_direction(worm::Direction::Up);
    game.change_direction(worm::Direction::Left);
    assert_eq!(game.cycles[0].direction, worm::Direction::Up);
    // After a tick actually moving Up, Left becomes legal again...
    game.update();
    game.change_direction(worm::Direction::Left);
    assert_eq!(game.cycles[0].direction, worm::Direction::Left);
    // ...but Down (the 180 of the tick just moved) is not.
    game.change_direction(worm::Direction::Down);
    assert_eq!(game.cycles[0].direction, worm::Direction::Left);
}
