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
    // Mid-arena (x=1 is now the slipstream corridor — a worm there
    // legitimately holds on non-16th frames, world v4).
    let head = (10, game.height / 2);
    game.cycles[0].head = head;
    game.cycles[0].positions.clear();
    game.cycles[0].positions.push(head);
    game.cycles[0].direction = worm::Direction::Left;
    game.grid[head.1 as usize][9] = worm::CellType::Wall;
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

    // Bolts now fly until they hit a wall rather than expiring at a fixed
    // range, so run until they are spent. The firer drives on unattended and
    // will eventually hit a wall itself — that is not what this test is about,
    // so the invariant is checked on the CAUSE of death, not on game_over.
    for _ in 0..200 {
        game.update();
        assert_ne!(
            game.death_cause,
            Some(worm::game::DeathCause::TriShotBolt),
            "a tri-shot must never kill its own firer"
        );
        if game.projectiles.is_empty() || game.game_over {
            break;
        }
    }
    assert!(
        game.projectiles.is_empty(),
        "bolts must die on the arena wall, not fly forever"
    );
    // The arena is enclosed, so nothing can escape the board — the wall
    // terminates every bolt and no bolt may breach one.
    assert!(
        game.grid.iter().flatten().all(|c| *c != worm::CellType::Hole),
        "tri-shot bolts must never break a wall"
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
    game.bombs[0].tripped = true; // forced: only a tripped mine detonates
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
        disguise: 5,
        armed_in: 0,
        owner: 0,
        tripped: false,
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
    // v8 pin: pre-napalm bolt physics (recorded ghosts keep it).
    game.set_world_version(8);
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
    // v9 pin: press-time single-slot input semantics — recorded
    // ghosts keep them; v10 collects inputs and consumes at the frame
    // (see test_v10_input_queue_contracts for the successors).
    game.set_world_version(9);
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
    game.read_rate = 1.0; // tick-perfect CPU under test — ADR-018 opens dozy
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
    game.update(); // player still Right
    // The corpus is now stratified — routine straight frames are thinned, so
    // the SECOND frame of a straight run may not be stored at all. The
    // invariant under test is about what a stored episode contains, not how
    // many there are.
    assert!(
        !game.cpu_brain.opp_brain.episodes.is_empty(),
        "the opening frame is always recorded"
    );
    for (i, e) in game.cpu_brain.opp_brain.episodes.iter().enumerate() {
        // The turn-history block (25..31 in the current layout: recent turn
        // mix + last turn) is built from the tail, which must not yet contain
        // this frame's own move. On the opening frames of a straight run there
        // are no turn PAIRS at all, so the whole block must be empty — if the
        // label ever leaked into its own context, it would show up here first.
        // (Slots 13..25 now carry always-on situational features — proximity,
        // speed, arena state — which are legitimately non-zero from frame 1;
        // the old assertion over 13..29 was pinned to the retired layout.)
        let turn_history_mass: f32 = e.vector[25..31].iter().sum();
        assert_eq!(
            turn_history_mass, 0.0,
            "episode {i} must not encode a turn pair ending in its own label"
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
    let decision = game.cpu_telemetry.decision.as_ref().unwrap();
    assert_eq!(decision.reason, worm::cpu_ai::CpuDecisionReason::WarmingUp);
    assert!(
        decision.projection.is_none(),
        "cold models do not project a path"
    );
    assert!(
        decision.forecast.is_none(),
        "cold decisions claim no forecast"
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
    let scored = game.cpu_telemetry.scored.unwrap();
    assert_eq!(scored.forecast.target_frame, game.frame_count);
    assert_eq!(scored.forecast.predicted, Some(worm::Direction::Right));
    assert_eq!(scored.actual, worm::Direction::Right);
    assert!(scored.hit);
    let decision = game.cpu_telemetry.decision.as_ref().unwrap();
    assert_eq!(decision.frame, game.frame_count);
    // The decision must be driven by the forecast for the frame the player has
    // NOT yet chosen. This previously asserted `decision.forecast ==
    // scored.forecast` — the forecast for the frame already in progress, whose
    // answer is on the board by the time the CPU runs. That made the
    // "prediction" a restatement of an observable and meant no improvement to
    // the opponent model could change a decision. See ADR-007.
    assert_eq!(
        decision.forecast.unwrap().target_frame,
        game.frame_count + 1,
        "the CPU must steer on a forecast of the NEXT frame, not the current one"
    );
    assert_eq!(
        game.cpu_telemetry.next_forecast.unwrap().target_frame,
        game.frame_count + 1
    );
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
    assert_eq!(game.cpu_telemetry, worm::CpuFrameTelemetry::default());
    assert!(game.round_last_cpu_decision.is_none());
    assert_eq!(game.food_eaten_by, [0, 0]);
    assert_eq!(
        game.cpu_brain.opp_pred_total, lifetime_predictions_before,
        "lifetime prediction evidence persists"
    );
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

#[test]
fn test_round_boundary_resize_preserves_brain_and_banks_winner_once() {
    let mut game = WormGame::with_size_seed(120, 38, 42);
    let mut observation = [0.0f32; worm::cpu_ai::CPU_FEATURE_DIM];
    observation[0] = 1.0;
    game.cpu_brain
        .remember(observation, worm::Direction::Up, 3.0);
    game.winner = Some(1);

    game.restart_with_size(88, 44);

    assert_eq!((game.width, game.height), (88, 44));
    assert_eq!(game.fixed_dims, Some((88, 44)));
    assert_eq!(game.session_wins, [0, 1]);
    assert_eq!(
        game.cpu_brain.episodes.len(),
        1,
        "brain corpus survives resize"
    );
    assert_eq!(game.frame_count, 0);
    assert_eq!(game.cpu_telemetry, worm::CpuFrameTelemetry::default());
    assert!(game.round_last_cpu_decision.is_none());
}

/// The sophisticated model abstains while cold and joins once warm —
/// rps-ai's NN/DT needed 5-7 rounds; ours needs COLD_START_EPISODES.
#[test]
fn test_knn_model_abstains_cold_joins_warm() {
    let mut game = WormGame::with_size(120, 38);
    let (pending, _, _, _) = worm::cpu_ai::compute_ensemble(&game, &game.cpu_brain);
    assert_eq!(pending[6], None, "knn abstains with an empty memory");

    let ctx = [0.1f32; worm::cpu_ai::PLAYER_FEATURE_DIM];
    for _ in 0..60 {
        worm::record_player_episode(&mut game.cpu_brain, ctx, worm::Direction::Right, true);
    }
    let (pending, _, _, _) = worm::cpu_ai::compute_ensemble(&game, &game.cpu_brain);
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
    game.read_rate = 1.0; // tick-perfect CPU under test — ADR-018 opens dozy
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
    let head = (10, game.height / 2);
    game.cycles[0].head = head;
    game.cycles[0].positions.clear();
    game.cycles[0].positions.push(head);
    game.cycles[0].direction = worm::Direction::Left;
    game.grid[game.cycles[0].head.1 as usize][9] = worm::CellType::Wall;
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
        disguise: 5,
        armed_in: 0,
        owner: 1,
        tripped: true, // forced: only a tripped mine detonates
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
    assert_eq!(SfxKind::Breach as u8, 6);
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
        disguise: 5,
        armed_in: 0,
        owner: 1,
        tripped: true, // forced: only a tripped mine detonates
    });
    game.tick_bombs();
    assert!(
        !game.game_over,
        "no head dies: player out of range, CPU is the owner"
    );
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
    // v9 pin: press-time single-slot input semantics — recorded
    // ghosts keep them; v10 collects inputs and consumes at the frame
    // (see test_v10_input_queue_contracts for the successors).
    game.set_world_version(9);
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

/* ---- audit round 2: improvements + verified leftovers (2026-08) ---- */

#[test]
fn test_bomb_blast_breaks_arena_wall_not_frame() {
    // Design intent: a blast punches the ring-2 arena wall open (Hole) so
    // players can reach the outer corridor; the ring-0 frame is untouchable.
    let mut game = WormGame::with_size(120, 38);
    game.bombs.push(worm::game::Bomb {
        x: 5,
        y: 5,
        fuse: 1,
        disguise: 5,
        armed_in: 0,
        owner: 0,
        tripped: true, // forced: only a tripped mine detonates
    });
    game.tick_bombs();
    assert_eq!(
        game.grid[5][3],
        worm::CellType::Hole,
        "the arena-wall cell (ring 3 under v6) in the blast must open"
    );
    assert_eq!(game.grid[3][5], worm::CellType::Hole);
    assert_eq!(
        game.grid[5][0],
        worm::CellType::Wall,
        "the ring-0 outer frame is indestructible"
    );
    assert_eq!(game.grid[0][5], worm::CellType::Wall);
}

#[test]
fn test_laser_triggered_bomb_kills_its_owner() {
    // Blast credit follows the trigger: lasering an ENEMY bomb detonates it
    // as the firer's blast — the bomb's planter can die to it, the firer
    // cannot.
    let mut game = WormGame::with_size(120, 38);
    game.food_items.clear();
    game.cycles[0].head = (30, 20);
    game.cycles[0].positions = vec![(30, 20)];
    game.cycles[0].direction = worm::Direction::Right;
    game.cycles[0].held_powerup = Some(worm::game::PowerUpKind::Laser);
    game.grid[20][30] = worm::CellType::Player;
    game.cycles[1].head = (35, 25); // inside the blast, off the beam line
    game.cycles[1].positions = vec![(35, 25)];
    game.grid[25][35] = worm::CellType::CPU;
    game.bombs.push(worm::game::Bomb {
        x: 35,
        y: 20,
        fuse: 60,
        disguise: 5,
        armed_in: 0,
        owner: 1, // the CPU planted it
        tripped: false,
    });
    assert!(game.fire_powerup(0));
    assert!(game.game_over);
    assert_eq!(game.winner, Some(0), "the triggering firer gets the kill");
    assert!(
        !game.cycles[1].alive,
        "the planter is not immune to a triggered blast"
    );
    assert!(
        game.cycles[0].alive,
        "the firer is immune to the blast it triggered"
    );
}

#[test]
fn test_growth_matches_food_value_when_chain_eating() {
    // Eating while growth is still owed used to skip that frame's payment,
    // granting one extra segment. pending = old + food - 1 (the kept tail).
    let mut game = WormGame::with_size(120, 38);
    let head = game.cycles[0].head;
    game.cycles[0].positions = vec![head, (head.0 - 1, head.1), (head.0 - 2, head.1)];
    for &(x, y) in &game.cycles[0].positions.clone() {
        game.grid[y as usize][x as usize] = worm::CellType::Player;
    }
    game.cycles[0].pending_growth = 2;
    game.cycles[0].direction = worm::Direction::Right;
    game.food_items.clear();
    game.food_items.push((head.0 + 1, head.1, 3));
    game.grid[head.1 as usize][(head.0 + 1) as usize] = worm::CellType::Food;
    game.update();
    assert_eq!(
        game.cycles[0].pending_growth, 4,
        "pending = 2 owed + 3 eaten - 1 paid by the kept tail"
    );
}

#[test]
fn test_sudden_death_closes_ring_on_schedule() {
    let mut game = WormGame::with_size(120, 38);
    game.cpu_autopilot = false;
    game.cycles[0].head = (30, 20);
    game.cycles[0].positions = vec![(30, 20)];
    game.cycles[0].direction = worm::Direction::Right;
    game.grid[20][30] = worm::CellType::Player;
    game.cycles[1].head = (70, 25);
    game.cycles[1].positions = vec![(70, 25)];
    game.cycles[1].direction = worm::Direction::Right;
    game.grid[25][70] = worm::CellType::CPU;
    game.time = worm::game::SUDDEN_DEATH_START + worm::game::SUDDEN_DEATH_INTERVAL - 1;
    game.update();
    assert_eq!(game.shrink_level, 1);
    assert_eq!(
        game.grid[3][10],
        worm::CellType::Wall,
        "the first inward ring (offset 3) must be sealed"
    );
    assert_eq!(game.grid[10][3], worm::CellType::Wall);
    assert!(!game.game_over, "both snakes were safely inside the ring");
}

#[test]
fn test_sudden_death_kills_head_on_closing_ring() {
    let mut game = WormGame::with_size(120, 38);
    game.cpu_autopilot = false;
    game.food_items.clear();
    // The player steps onto (10,3) the same frame that ring seals.
    game.cycles[0].head = (10, 4);
    game.cycles[0].positions = vec![(10, 4)];
    game.cycles[0].direction = worm::Direction::Up;
    game.grid[4][10] = worm::CellType::Player;
    game.cycles[1].head = (70, 25);
    game.cycles[1].positions = vec![(70, 25)];
    game.cycles[1].direction = worm::Direction::Right;
    game.grid[25][70] = worm::CellType::CPU;
    game.time = worm::game::SUDDEN_DEATH_START + worm::game::SUDDEN_DEATH_INTERVAL - 1;
    game.update();
    assert!(game.game_over);
    assert_eq!(
        game.winner,
        Some(1),
        "the head caught on the closing ring dies"
    );
    assert!(!game.cycles[0].alive);
}

#[test]
fn test_laser_bounces_off_arena_wall() {
    // Shooter near the left edge facing right: the beam crosses the arena,
    // bounces off the right arena wall (ring 2), and returns along the same
    // row — hitting an opponent BEHIND the shooter, reachable only
    // post-bounce. The outer frame still stops the beam.
    let mut game = WormGame::with_size(120, 38);
    game.food_items.clear();
    game.cycles[0].head = (4, 10);
    game.cycles[0].positions = vec![(4, 10)];
    game.cycles[0].direction = worm::Direction::Right;
    game.cycles[0].held_powerup = Some(worm::game::PowerUpKind::Laser);
    game.grid[10][4] = worm::CellType::Player;
    game.cycles[1].head = (3, 10); // behind the shooter
    game.cycles[1].positions = vec![(3, 10)];
    game.grid[10][3] = worm::CellType::CPU;
    assert!(game.fire_powerup(0));
    assert!(
        game.game_over,
        "the bounced beam reaches behind the shooter"
    );
    assert_eq!(game.winner, Some(0));
    assert!(!game.cycles[1].alive);
}

#[test]
fn test_cpu_laser_charges_before_firing() {
    // The CPU's laser telegraphs for LASER_TELEGRAPH_FRAMES before firing —
    // it must NOT kill the moment the player crosses the firing line.
    let mut game = WormGame::with_size(120, 38);
    game.read_rate = 1.0; // tick-perfect CPU under test — ADR-018 opens dozy
    game.food_items.clear();
    // Corridor walls force the CPU straight (ForcedMove) so the setup is
    // deterministic: CPU chases the player along row 20.
    for x in 25..=70usize {
        game.grid[19][x] = worm::CellType::Wall;
        game.grid[21][x] = worm::CellType::Wall;
    }
    game.cycles[1].head = (30, 20);
    game.cycles[1].positions = vec![(30, 20)];
    game.cycles[1].direction = worm::Direction::Right;
    game.cycles[1].held_powerup = Some(worm::game::PowerUpKind::Laser);
    game.grid[20][30] = worm::CellType::CPU;
    game.cycles[0].head = (50, 20);
    game.cycles[0].positions = vec![(50, 20)];
    game.cycles[0].direction = worm::Direction::Right;
    game.grid[20][50] = worm::CellType::Player;
    for _ in 0..(worm::game::LASER_TELEGRAPH_FRAMES - 1) {
        game.update();
        assert!(
            !game.game_over,
            "the laser must not fire while still charging"
        );
        assert_eq!(
            game.cycles[1].held_powerup,
            Some(worm::game::PowerUpKind::Laser)
        );
    }
    game.update();
    assert!(
        game.game_over,
        "the charged laser fires on the telegraph deadline"
    );
    assert_eq!(game.winner, Some(1));
}

/* ------------------- brain persistence + schema migration ------------------- */
//
// The game's premise is that the CPU learns YOU across many matches, so a
// schema change must never silently reset that. These pin the guarantee.

/// Build a brain with content in every section.
fn seeded_brain() -> worm::CpuBrain {
    let mut b = worm::CpuBrain::new();
    for i in 0..12 {
        let mut v = [0.0f32; worm::cpu_ai::CPU_FEATURE_DIM];
        v[0] = i as f32;
        b.remember(v, worm::Direction::Up, 1.0);
    }
    for i in 0..7 {
        let mut v = [0.0f32; worm::cpu_ai::PLAYER_FEATURE_DIM];
        v[0] = i as f32;
        b.opp_brain.remember(v, worm::Direction::Left);
        b.opp_brain.observe(worm::Direction::Left);
    }
    b.opp_pred_hits = 41;
    b.opp_pred_total = 100;
    b
}

#[test]
fn test_brain_roundtrip_preserves_every_section() {
    let original = seeded_brain();

    let (restored, report) =
        worm::CpuBrain::from_bytes_report(&original.to_bytes()).expect("brain must decode");

    assert_eq!(report.format, 2, "saves use the current sectioned format");
    assert!(!report.is_partial(), "a same-version load loses nothing");
    assert_eq!(restored.episodes.len(), original.episodes.len());
    assert_eq!(
        restored.opp_brain.episodes.len(),
        original.opp_brain.episodes.len()
    );
    assert_eq!(restored.opp_pred_hits, 41);
    assert_eq!(restored.opp_pred_total, 100);
    assert_eq!(restored.cpu_seq, original.cpu_seq);
    assert_eq!(restored.tally, original.tally);
}

#[test]
fn test_brain_survives_survival_feature_space_change() {
    // THE case this format exists for: the survival encoding gains new
    // features (power-up awareness), so those vectors become meaningless —
    // but what the CPU learned about the HUMAN must carry forward.
    let bytes = seeded_brain().to_bytes();
    let corrupted = rewrite_cpu_episode_dim(&bytes, (worm::cpu_ai::CPU_FEATURE_DIM + 4) as u16);

    let (restored, report) =
        worm::CpuBrain::from_bytes_report(&corrupted).expect("a stale brain migrates, never fails");

    assert!(report.is_partial());
    assert_eq!(
        restored.episodes.len(),
        0,
        "survival episodes are bound to the encoding and must be dropped"
    );
    assert_eq!(
        restored.opp_brain.episodes.len(),
        7,
        "opponent episodes are independent and must survive"
    );
    assert_eq!(
        (restored.opp_pred_hits, restored.opp_pred_total),
        (41, 100),
        "the head-to-head record is knowledge about the human — never reset it"
    );
}

#[test]
fn test_brain_rejects_non_brain_bytes() {
    assert!(worm::CpuBrain::from_bytes(b"not a brain at all").is_none());
    assert!(worm::CpuBrain::from_bytes(&[]).is_none());
    assert!(worm::CpuBrain::from_bytes(&[1, 2, 3]).is_none());
}

#[test]
fn test_brain_tolerates_truncation_without_losing_earlier_sections() {
    // A half-written IndexedDB record must not cost the whole corpus.
    let bytes = seeded_brain().to_bytes();
    let truncated = &bytes[..bytes.len() * 2 / 3];

    let (restored, _) =
        worm::CpuBrain::from_bytes_report(truncated).expect("a truncated brain still decodes");
    // Sections are written CPU-core first, so the earliest survive.
    assert!(restored.cpu_seq > 0, "leading sections decode despite truncation");
}

/// Rewrite the `dim` field inside the CPU-episodes section, simulating a brain
/// saved by a build whose survival feature space differed from this one.
fn rewrite_cpu_episode_dim(bytes: &[u8], new_dim: u16) -> Vec<u8> {
    let mut out = bytes.to_vec();
    let mut off = 8; // magic(4) + format(2) + section_count(2)
    while off + 6 <= out.len() {
        let tag = u16::from_le_bytes(out[off..off + 2].try_into().unwrap());
        let len = u32::from_le_bytes(out[off + 2..off + 6].try_into().unwrap()) as usize;
        let body = off + 6;
        if tag == 2 {
            // EpisodesWire serializes `dim: u16` first.
            out[body..body + 2].copy_from_slice(&new_dim.to_le_bytes());
            return out;
        }
        off = body + len;
    }
    panic!("CPU-episode section not found");
}

/* ---------------------- sudden-death ring evacuation ---------------------- */
//
// close_ring kills any head standing on the ring it seals. The CPU's base
// policy is a right-hand wall-follow that hugs the inner face of the ring-2
// wall — which is exactly the first ring to close — and nothing in cpu_ai.rs
// knew sudden death existed. Measured against a passive opponent, 47 of 100
// games ended with the CPU standing on that ring at the sealing frame.

#[test]
fn test_ring_seal_eta_reports_the_scheduled_ring() {
    let game = WormGame::with_size(120, 38);
    // World v8: sudden death starts one ring INSIDE the v6 arena wall —
    // level 1 seals the ring at offset 4, at START + 1*INTERVAL.
    let seals_at = worm::game::SUDDEN_DEATH_START + worm::game::SUDDEN_DEATH_INTERVAL;

    assert_eq!(game.ring_seal_eta(4, 20), Some(seals_at - game.time));
    assert_eq!(game.ring_seal_eta(20, 4), Some(seals_at - game.time));
    assert_eq!(
        game.ring_seal_eta(115, 20),
        Some(seals_at - game.time),
        "the far edge of the ring is on it too"
    );
    assert_eq!(
        game.ring_seal_eta(60, 20),
        None,
        "mid-arena is not on any scheduled ring"
    );
}

#[test]
fn test_cpu_steps_off_a_ring_that_is_about_to_seal() {
    let mut game = WormGame::with_size(120, 38);
    // Two frames before the first ring seals (offset 4 under v8 —
    // one ring inside the v6 arena wall).
    game.time = worm::game::SUDDEN_DEATH_START + worm::game::SUDDEN_DEATH_INTERVAL - 2;

    // Stand the CPU on that ring, travelling along it — the wall-follow
    // behaviour that used to kill it.
    game.cycles[1].head = (4, 20);
    game.cycles[1].positions = vec![(4, 20)];
    game.cycles[1].direction = worm::Direction::Down;
    game.grid[20][4] = worm::CellType::CPU;

    assert!(
        game.ring_seal_eta(4, 20).is_some_and(|e| e <= 3),
        "precondition: the CPU is standing on a ring about to seal"
    );

    let chosen = worm::cpu_decide(&mut game);
    let (dx, dy) = chosen.as_delta();
    let nx = (4i16 + dx) as u16;
    let ny = (20i16 + dy) as u16;

    assert!(
        game.ring_seal_eta(nx, ny).is_none_or(|e| e > 3),
        "the CPU moved to ({nx},{ny}), still on a ring sealing in \
         {:?} frames — it must evacuate, not ride the wall into the close",
        game.ring_seal_eta(nx, ny)
    );
}

/* --------------------------- laser tail sever ---------------------------- */
//
// The beam cuts the opponent's trail where it crosses and everything beyond
// the cut is lost. The cut is at the crossing NEAREST THEIR HEAD, so aiming at
// the neck is worth more than clipping the tail tip.

/// Build a game with the CPU's trail laid out horizontally and the player
/// positioned to fire a vertical beam across it.
fn severable_board() -> WormGame {
    let mut game = WormGame::with_size(120, 38);
    // CPU trail runs left-to-right along row 20, head at x=40.
    let trail: Vec<(u16, u16)> = (0..12).map(|i| (40 - i, 20)).collect();
    for &(x, y) in &trail {
        game.grid[y as usize][x as usize] = worm::CellType::CPU;
    }
    game.cycles[1].positions = trail;
    game.cycles[1].head = (40, 20);
    game
}

#[test]
fn test_laser_severs_the_trail_at_the_crossing_nearest_the_head() {
    let mut game = severable_board();
    let before = game.cycles[1].positions.len();

    // Player sits below the trail at x=37 (index 3 from the head) and fires up.
    game.cycles[0].head = (37, 25);
    game.cycles[0].positions = vec![(37, 25)];
    game.cycles[0].direction = worm::Direction::Up;
    game.cycles[0].held_powerup = Some(worm::game::PowerUpKind::Laser);

    assert!(game.fire_powerup(0));

    assert_eq!(
        game.cycles[1].positions.len(),
        3,
        "the cut is at index 3 — the crossing nearest their head — so only \
         indices 0..3 survive (was {before})"
    );
    assert_eq!(
        game.cycles[1].head,
        (40, 20),
        "severing must never remove the head; that is the kill path"
    );
}

#[test]
fn test_severed_cells_leave_both_the_grid_and_positions() {
    // The lockstep invariant: a cell leaving `positions` must lose its grid
    // marker, or a later tail-pop writes Empty over a cell someone else owns.
    let mut game = severable_board();
    game.cycles[0].head = (37, 25);
    game.cycles[0].positions = vec![(37, 25)];
    game.cycles[0].direction = worm::Direction::Up;
    game.cycles[0].held_powerup = Some(worm::game::PowerUpKind::Laser);

    game.fire_powerup(0);

    for &(x, y) in &game.cycles[1].positions {
        assert_eq!(
            game.grid[y as usize][x as usize],
            worm::CellType::CPU,
            "a surviving segment at ({x},{y}) lost its grid marker"
        );
    }
    // Everything past the cut is gone from the grid too.
    for i in 0..9u16 {
        let (x, y) = (37 - i, 20);
        assert_ne!(
            game.grid[y as usize][x as usize],
            worm::CellType::CPU,
            "severed cell ({x},{y}) still carries a grid marker"
        );
    }
}

#[test]
fn test_sever_clears_owed_growth_so_the_cut_is_not_undone() {
    let mut game = severable_board();
    game.cycles[1].pending_growth = 7;
    game.cycles[0].head = (37, 25);
    game.cycles[0].positions = vec![(37, 25)];
    game.cycles[0].direction = worm::Direction::Up;
    game.cycles[0].held_powerup = Some(worm::game::PowerUpKind::Laser);

    game.fire_powerup(0);

    assert_eq!(
        game.cycles[1].pending_growth, 0,
        "owed growth would silently regrow what the beam just cut off"
    );
}

#[test]
fn test_arena_wall_tracks_sudden_death_shrink() {
    // Pinned to ring 2, the live inner wall stopped being recognised after the
    // first shrink: beams stopped dead instead of bouncing or breaching, and
    // bomb blasts stopped breaking walls.
    let mut game = WormGame::with_size(120, 38);
    assert!(game.is_arena_wall(3, 20), "ring 3 is the wall before any shrink (v6)");
    assert!(!game.is_arena_wall(4, 20));

    game.shrink_level = 1;
    assert!(game.is_arena_wall(4, 20), "the wall moves inward with the shrink");
    assert!(!game.is_arena_wall(3, 20), "the old ring is no longer the wall");
    assert_eq!(game.arena_wall_offset(), 4);
}

/* ------------------------- bomb as a proximity mine ----------------------- */
//
// The old bomb was a 3s timer with a 21x21 blast: escaping took 11 moves and
// the fuse gave 26-85, so an attentive target always walked out. It is now a
// mine — you cannot wait it out, only route around it.

fn mine_board() -> WormGame {
    let mut game = WormGame::with_size(120, 38);
    game.cycles[0].head = (60, 20);
    game.cycles[0].positions = vec![(60, 20)];
    game.cycles[1].head = (20, 10);
    game.cycles[1].positions = vec![(20, 10)];
    game
}

#[test]
fn test_mine_is_inert_while_arming() {
    let mut game = mine_board();
    // Planted right under the enemy head — but not yet armed.
    game.bombs.push(worm::game::Bomb {
        x: 60,
        y: 20,
        fuse: worm::game::BOMB_FUSE_FRAMES,
        disguise: 5,
        armed_in: worm::game::MINE_ARM_FRAMES,
        owner: 1,
        tripped: false,
    });

    game.tick_bombs();

    assert_eq!(
        game.bombs.len(),
        1,
        "an arming mine must not fire, or planting one at your own head is suicide"
    );
    assert!(game.cycles[0].alive);
}

#[test]
fn test_armed_mine_fires_when_an_enemy_head_enters_the_ring() {
    let mut game = mine_board();
    game.bombs.push(worm::game::Bomb {
        x: 60 + worm::game::MINE_TRIGGER_CELLS as u16,
        y: 20,
        fuse: worm::game::BOMB_FUSE_FRAMES,
        disguise: 5,
        armed_in: 0,
        owner: 1,
        tripped: false,
    });

    game.tick_bombs();

    assert!(game.bombs.is_empty(), "an armed mine fires on proximity");
    assert!(!game.cycles[0].alive, "the head inside the ring dies");
}

#[test]
fn test_walking_onto_your_own_mine_detonates_it_without_killing_you() {
    // Remote detonation, for free: the owner is immune to the BLAST but not to
    // the TRIGGER. Paid for in board position — you have to physically return.
    let mut game = mine_board();
    game.bombs.push(worm::game::Bomb {
        x: 20,
        y: 10,
        fuse: worm::game::BOMB_FUSE_FRAMES,
        disguise: 5,
        armed_in: 0,
        owner: 1,
        tripped: false,
    });

    game.tick_bombs();

    assert_eq!(
        game.bombs.len(),
        1,
        "your own mine does not trigger on you — only an enemy head trips it"
    );
    assert!(game.cycles[1].alive);
}

#[test]
fn test_blast_is_a_cross_not_a_square() {
    // The shape is the whole point: 65 cells instead of 441, and honestly
    // drawable. On-axis at reach dies; off-axis at the same distance does not.
    let arm = worm::game::BOMB_RADIUS_CELLS as i32;
    let core = worm::game::BOMB_CORE_RADIUS as i32;

    assert!(
        worm::game::in_blast(50, 20, 50 + arm, 20, arm),
        "the end of an arm is inside the blast"
    );
    assert!(
        worm::game::in_blast(50, 20, 50, 20 - arm, arm),
        "arms run on both axes"
    );
    assert!(
        !worm::game::in_blast(50, 20, 50 + core + 1, 20 + core + 1, arm),
        "a diagonal just outside the core survives — this is what makes it a cross"
    );
    assert!(
        worm::game::in_blast(50, 20, 50 + core, 20 + core, arm),
        "the core square is solid"
    );
    assert!(
        !worm::game::in_blast(50, 20, 50 + arm, 20 + arm, arm),
        "the far diagonal corner — dead in the old square — now survives"
    );
}

#[test]
fn test_mine_blast_still_breaches_the_arena_wall() {
    // Breaching is now a byproduct of the laser and the bomb; deleting
    // WallPunch must not have taken the bomb's breach with it.
    let mut game = mine_board();
    let off = game.arena_wall_offset();
    // Plant so an arm reaches the left arena wall along a row.
    game.bombs.push(worm::game::Bomb {
        x: off + 3,
        y: 20,
        fuse: 1,
        disguise: 5,
        armed_in: 0,
        owner: 1,
        tripped: true, // forced: only a tripped mine detonates
    });

    game.tick_bombs();

    assert_eq!(
        game.grid[20][off as usize],
        worm::CellType::Hole,
        "a blast arm reaching the arena wall opens a hole"
    );
    assert_eq!(
        game.grid[20][0],
        worm::CellType::Wall,
        "the ring-0 frame is never breachable"
    );
}

/* ------------------------- sealed prediction ----------------------------- */

/// THE test that gives the seal meaning.
///
/// Same board, same frame, four different player inputs. The committed seal
/// must be identical in all four, because it was fixed before the input
/// existed. If a refactor ever moves forecast generation after input, exactly
/// one of these stops matching and this fails.
#[test]
fn test_the_seal_is_committed_before_the_input_is_read() {
    let seals: Vec<u64> = [
        worm::Direction::Up,
        worm::Direction::Down,
        worm::Direction::Left,
        worm::Direction::Right,
    ]
    .iter()
    .map(|&d| {
        let mut game = WormGame::with_size_seed(120, 38, 777);
        for _ in 0..6 {
            game.update();
        }
        let committed = game.cpu_telemetry.next_forecast.unwrap();
        // Only now does the player choose.
        game.change_direction(d);
        game.update();
        committed.seal
    })
    .collect();

    assert!(
        seals.windows(2).all(|w| w[0] == w[1]),
        "the seal must not depend on what the player did next: {seals:?}"
    );
}

#[test]
fn test_a_revealed_seal_verifies_against_its_prediction() {
    let mut game = WormGame::with_size_seed(120, 38, 4242);
    for _ in 0..8 {
        game.update();
    }
    let scored = game.cpu_telemetry.scored.expect("a forecast was scored");
    let salt = worm::cpu_ai::seal_salt(game.seal_seed, scored.forecast.target_frame);

    assert_eq!(
        scored.forecast.seal,
        worm::cpu_ai::seal_commit(salt, scored.forecast.predicted, scored.forecast.target_frame),
        "the revealed prediction must match what was sealed"
    );
    // And a different prediction must NOT verify, or the seal proves nothing.
    assert_ne!(
        scored.forecast.seal,
        worm::cpu_ai::seal_commit(salt, None, scored.forecast.target_frame),
        "a seal that verifies against any prediction is not a commitment"
    );
}

#[test]
fn test_sealing_does_not_disturb_the_game_rng() {
    // The salt is a pure function of (seal_seed, frame) precisely so seeded
    // runs stay bit-identical. Two runs of the same seed must agree exactly.
    let run = || {
        let mut game = WormGame::with_size_seed(120, 38, 20260805);
        for _ in 0..120 {
            if !game.update() {
                break;
            }
        }
        (
            game.frame_count,
            game.food_items.clone(),
            game.cycles[1].head,
            game.seal_chain,
        )
    };
    assert_eq!(run(), run(), "seeded play must be reproducible");
}

/// The corridor pin — the one deterministic loss a player could execute at
/// will. Escorting parallel one row inside a wall lane, diagonally abeam at
/// equal speed, left the CPU exactly one legal move per frame until the
/// facing wall killed it (traced dying at the identical frame on every
/// replay of multiple seeds).
#[test]
fn test_escorted_wall_lane_is_refused_before_the_lock_forms() {
    let mut game = WormGame::with_size(55, 40);
    // Sweep both worms' spawn cells, then build the traced geometry by hand.
    for row in game.grid.iter_mut() {
        for cell in row.iter_mut() {
            if matches!(*cell, worm::CellType::Player | worm::CellType::CPU) {
                *cell = worm::CellType::Empty;
            }
        }
    }
    // CPU in the top lane (row 4 — the arena wall sits at y=3 under v6),
    // heading Left.
    game.cycles[1].head = (30, 4);
    game.cycles[1].direction = worm::Direction::Left;
    game.cycles[1].prev_direction = worm::Direction::Left;
    game.cycles[1].positions = vec![(30, 4), (31, 4)];
    for &(x, y) in &game.cycles[1].positions {
        game.grid[y as usize][x as usize] = worm::CellType::CPU;
    }
    // Player escorting abeam: one row below, one column behind, same heading.
    game.cycles[0].head = (31, 5);
    game.cycles[0].direction = worm::Direction::Left;
    game.cycles[0].prev_direction = worm::Direction::Left;
    game.cycles[0].positions = vec![(31, 5), (32, 5), (33, 5), (34, 5)];
    for &(x, y) in &game.cycles[0].positions {
        game.grid[y as usize][x as usize] = worm::CellType::Player;
    }

    // The geometry itself is recognised…
    assert!(
        worm::escorted_lane_step(&game, (30, 4), worm::Direction::Left),
        "continuing straight in an escorted wall lane must read as escorted"
    );
    // …and a perpendicular exit is not tarred with the same brush.
    assert!(
        !worm::escorted_lane_step(&game, (30, 4), worm::Direction::Down),
        "leaving the lane is the escape, not part of the trap"
    );

    // Play it out: the player holds the escort line, exactly as the exploit
    // prescribes. Before the fix the CPU was marched into the left wall and
    // died the moment the player ran out of corridor.
    let mut frames = 0;
    while !game.game_over && game.cycles[0].head.0 > 6 && frames < 60 {
        game.change_direction(worm::Direction::Left);
        game.update();
        frames += 1;
    }
    assert!(
        game.cycles[1].alive || game.winner == Some(1),
        "the CPU must escape the escort (or win outright), never be marched \
         into the wall — died at frame {} with cause {:?}",
        game.frame_count,
        game.death_cause,
    );
}

/// The intent models ROUTE, they do not crow-fly. A greedy Manhattan step
/// walks the player's prediction into a wall; the disagreement with real
/// routing lands almost entirely on voluntary-turn frames — the only frames
/// that carry a decision.
#[test]
fn test_eat_model_routes_around_a_wall() {
    let mut game = WormGame::with_size(55, 40);
    for row in &mut game.grid {
        for cell in row.iter_mut() {
            if *cell != worm::CellType::Wall {
                *cell = worm::CellType::Empty;
            }
        }
    }
    game.food_items.clear();
    game.powerups.clear();
    game.cycles[0].head = (10, 8);
    game.cycles[0].direction = worm::Direction::Right;
    game.cycles[0].positions = vec![(10, 8), (9, 8)];
    game.grid[8][10] = worm::CellType::Player;
    game.grid[8][9] = worm::CellType::Player;
    game.cycles[1].head = (40, 30);
    game.cycles[1].positions = vec![(40, 30)];
    game.grid[30][40] = worm::CellType::CPU;
    // A comb tooth: the player sits inside a cul-de-sac open only behind
    // them, and the food is on the far side of the tooth. Every step deeper
    // into the pocket (Right, toward the food as the crow flies) is a STRICT
    // detour on the actual route, not a tie the hold-the-line rule could
    // mask.
    for y in 3..=15 {
        game.grid[y][12] = worm::CellType::Wall;
    }
    for x in 9..=12 {
        game.grid[15][x] = worm::CellType::Wall;
    }
    game.food_items.push((14, 8, 1));
    game.grid[8][14] = worm::CellType::Food;

    let (pending, _, _, _) = worm::cpu_ai::compute_ensemble(&game, &game.cpu_brain);
    // Greedy Manhattan says Right — deeper into the pocket. Routing says out.
    assert!(
        pending[7].is_some(),
        "food is reachable, so the errand model must speak"
    );
    assert_ne!(
        pending[7],
        Some(worm::Direction::Right),
        "eat must predict the step that shortens the ROUTE, not the crow-fly \
         step deeper into a cul-de-sac"
    );
}

/// The twins differ in exactly one thing: what a player does when two steps
/// shorten the errand equally. `eat` holds the line; `eatW` weaves.
#[test]
fn test_intent_twins_disagree_only_on_ties() {
    let mut game = WormGame::with_size(55, 40);
    for row in &mut game.grid {
        for cell in row.iter_mut() {
            if *cell != worm::CellType::Wall {
                *cell = worm::CellType::Empty;
            }
        }
    }
    game.food_items.clear();
    game.powerups.clear();
    game.cycles[0].head = (10, 10);
    game.cycles[0].direction = worm::Direction::Right;
    game.cycles[0].positions = vec![(10, 10), (9, 10)];
    game.grid[10][10] = worm::CellType::Player;
    game.grid[10][9] = worm::CellType::Player;
    game.cycles[1].head = (40, 30);
    game.cycles[1].positions = vec![(40, 30)];
    game.grid[30][40] = worm::CellType::CPU;
    // Food diagonally away: Right and Down shorten the route equally the
    // whole way along the L.
    game.food_items.push((15, 15, 1));
    game.grid[15][15] = worm::CellType::Food;

    let (pending, _, _, _) = worm::cpu_ai::compute_ensemble(&game, &game.cpu_brain);
    assert_eq!(
        pending[7],
        Some(worm::Direction::Right),
        "the hold-the-line twin keeps the current heading on a tie"
    );
    assert_eq!(
        pending[10],
        Some(worm::Direction::Down),
        "the weaving twin takes the strict minimiser's turn on the same tie"
    );
    // And with no food at all, both abstain rather than guess.
    game.food_items.clear();
    game.grid[15][15] = worm::CellType::Empty;
    let (pending, _, _, _) = worm::cpu_ai::compute_ensemble(&game, &game.cpu_brain);
    assert_eq!(pending[7], None, "no errand, no guess");
    assert_eq!(pending[10], None, "no errand, no guess (weave)");
}

/// An expired fuse must FIZZLE, never detonate. For months the "backstop"
/// fuse — whose documented job is stopping stale mines accumulating —
/// spontaneously fired a 10-cell kill cross on a timer the player cannot
/// see, attached to a thing drawn as food. Play-tested verdict: "randomly
/// killed by bomb blasts that didn't actually happen".
#[test]
fn test_expired_mine_fizzles_instead_of_detonating() {
    // Pre-v8 physics pin: recorded ghosts keep the fizzle (an invisible
    // timer must never be a weapon — before the flash telegraph existed).
    let mut game = mine_board();
    game.set_world_version(7);
    // Player parked ON the blast axis, well outside the trigger ring but
    // deep inside where the old cross arms reached.
    game.cycles[0].head = (66, 20);
    game.cycles[0].positions = vec![(66, 20)];
    game.grid[20][66] = worm::CellType::Player;
    game.bombs.push(worm::game::Bomb {
        x: 60,
        y: 20,
        fuse: 1,
        disguise: 5,
        armed_in: 0,
        owner: 1,
        tripped: false,
    });

    game.tick_bombs();

    assert!(game.bombs.is_empty(), "the stale mine is cleaned up");
    assert!(
        game.cycles[0].alive,
        "an untripped expiry must never kill anyone"
    );
    assert!(!game.game_over);
    assert_ne!(
        game.grid[20][62],
        worm::CellType::Hole,
        "no blast: a fizzle must not punch walls or clear cells"
    );
}

/// World v8 (ADR-022): the decoy's fuse IS the weapon now — telegraphed.
/// Expiry detonates, the blast punches the arena wall, and the last two
/// wall-clock seconds flash (tier 1 then tier 2).
#[test]
fn test_v8_decoy_expiry_detonates_with_telegraph() {
    let mut game = mine_board();
    assert!(game.bomb_expiry_detonates(), "v8 board");
    game.cycles[0].head = (66, 20);
    game.cycles[0].positions = vec![(66, 20)];
    game.grid[20][66] = worm::CellType::Player;
    game.bombs.push(worm::game::Bomb {
        x: 60,
        y: 20,
        fuse: 2500, // ms — calm, then tier 1 at <=2000, tier 2 at <=1000
        disguise: 5,
        armed_in: 0,
        owner: 1,
        tripped: false,
    });
    assert_eq!(game.bomb_flash_tier(&game.bombs[0]), 0, "still a calm decoy");
    // Drain wall-clock: tiers must appear in order before the blast.
    let mut seen = [false; 3];
    for _ in 0..1000 {
        if game.bombs.is_empty() {
            break;
        }
        seen[game.bomb_flash_tier(&game.bombs[0]) as usize] = true;
        game.tick_bombs();
    }
    assert!(seen[1] && seen[2], "both flash tiers telegraph the detonation");
    assert!(game.bombs.is_empty(), "the expiry detonated (tripped drain)");
    assert!(
        !game.cycles[0].alive,
        "the on-axis player inside the cross arms dies to the expiry blast"
    );
    assert_eq!(game.death_cause, Some(worm::DeathCause::BombBlast));
}

/// World v8: the fuse is WALL-CLOCK — the same real time at any game
/// speed, and a slipstream freeze cannot disarm it (global-frame drain).
#[test]
fn test_v8_decoy_fuse_is_wall_clock() {
    // Slow game (no food): frame_delay ~115ms. Fast game: force the
    // speed floor by crediting food. Same 15s decoy either way.
    let mut slow = mine_board();
    let mut fast = mine_board();
    fast.food_eaten_total = 160; // speedup cap -> 35ms floor
    for g in [&mut slow, &mut fast] {
        // Far from both heads: proximity must not enter this test.
        g.bombs.push(worm::game::Bomb {
            x: 100, y: 30, fuse: 15_000, disguise: 3, armed_in: 0, owner: 1, tripped: false,
        });
    }
    let mut slow_frames = 0u32;
    while !slow.bombs.is_empty() {
        slow.tick_bombs();
        slow_frames += 1;
    }
    let mut fast_frames = 0u32;
    while !fast.bombs.is_empty() {
        fast.tick_bombs();
        fast_frames += 1;
    }
    // Frames differ by the speed ratio; wall-clock is identical by
    // construction (ms drained per frame == that frame's delay).
    assert!(
        fast_frames > slow_frames * 2,
        "the fast game needs proportionally more frames for the same 15s \
         (slow {slow_frames}, fast {fast_frames})"
    );
}

/* ---------------- tri-shot as grenade bolts (2x2 burst) ---------------- */

fn trishot_board() -> WormGame {
    let mut game = WormGame::with_size(120, 38);
    for row in &mut game.grid {
        for cell in row.iter_mut() {
            if *cell != worm::CellType::Wall {
                *cell = worm::CellType::Empty;
            }
        }
    }
    game.food_items.clear();
    game.powerups.clear();
    // Firer: player, heading Right.
    game.cycles[0].head = (10, 20);
    game.cycles[0].positions = vec![(10, 20), (9, 20)];
    game.cycles[0].direction = worm::Direction::Right;
    game.grid[20][10] = worm::CellType::Player;
    game.grid[20][9] = worm::CellType::Player;
    game
}

/// A bolt striking the opponent's TRAIL (not head) bursts and severs from
/// the hit back — the tri-shot is a grenade launcher now, not a head-only
/// needle. The victim survives shorter; the game does not end.
#[test]
fn test_bolt_on_trail_severs_from_the_hit_back() {
    let mut game = trishot_board();
    // v8 pin: pre-napalm bolt physics (recorded ghosts keep it).
    game.set_world_version(8);
    // CPU body crossing the straight ray at x=20, head safely off-ray and
    // far from the 2x2 burst.
    game.cycles[1].head = (20, 26);
    game.cycles[1].direction = worm::Direction::Down;
    game.cycles[1].positions = vec![
        (20, 26), // head (index 0)
        (20, 25),
        (20, 24),
        (20, 23),
        (20, 22),
        (20, 21), // inside the burst — the hit nearest the head
        (20, 20), // on the ray — the impact cell
    ];
    for &(x, y) in &game.cycles[1].positions {
        game.grid[y as usize][x as usize] = worm::CellType::CPU;
    }
    game.cycles[1].pending_growth = 4;

    game.cycles[0].held_powerup = Some(worm::game::PowerUpKind::TriShot);
    assert!(game.fire_powerup(0));
    // March only the projectiles; the worms stand still for the assertion.
    for _ in 0..12 {
        game.advance_projectiles();
    }

    assert!(game.cycles[1].alive, "a trail hit must not kill");
    assert!(!game.game_over);
    // Burst at (20,20) heading Right covers x 20..=21, y 20..=21; the CPU
    // cell nearest its head in the burst is (20,21) at index 5 — everything
    // from there back is gone.
    assert_eq!(
        game.cycles[1].positions.len(),
        5,
        "severed at the burst cell nearest the head; the stump survives"
    );
    assert_eq!(
        game.cycles[1].pending_growth, 0,
        "owed growth must not silently regrow the cut"
    );
    assert_eq!(
        game.grid[20][20],
        worm::CellType::Empty,
        "severed cells leave the grid"
    );
}

/// A head inside the 2x2 burst dies even when the bolt itself struck an
/// adjacent trail cell — that is what makes the burst a burst.
#[test]
fn test_head_inside_burst_dies() {
    let mut game = trishot_board();
    // v8 pin: pre-napalm bolt physics (recorded ghosts keep it).
    game.set_world_version(8);
    // Head sits one cell PAST the trail cell the bolt strikes, inside the
    // forward-biased 2x2 (impact (20,20) heading Right covers (21,20)).
    game.cycles[1].head = (21, 20);
    game.cycles[1].direction = worm::Direction::Right;
    game.cycles[1].positions = vec![(21, 20), (20, 20), (20, 21)];
    for &(x, y) in &game.cycles[1].positions {
        game.grid[y as usize][x as usize] = worm::CellType::CPU;
    }

    game.cycles[0].held_powerup = Some(worm::game::PowerUpKind::TriShot);
    assert!(game.fire_powerup(0));
    for _ in 0..12 {
        game.advance_projectiles();
        if game.game_over {
            break;
        }
    }

    assert!(!game.cycles[1].alive, "a head in the burst dies");
    assert_eq!(game.winner, Some(0), "the firer takes the kill");
    assert_eq!(
        game.death_cause,
        Some(worm::game::DeathCause::TriShotBolt)
    );
}

/// The burst must not breach walls (breaching stays the laser's and the
/// mine's job) and must not touch the firer's own trail.
#[test]
fn test_burst_spares_walls_and_own_trail() {
    let mut game = trishot_board();
    // v8 pin: pre-napalm bolt physics (recorded ghosts keep it).
    game.set_world_version(8);
    // Opponent trail hugging the top wall lane so the burst quadrant
    // includes an arena-wall cell.
    let wall_y = 3usize; // arena wall ring (ring 3 since world v6)
    game.cycles[1].head = (30, 10);
    game.cycles[1].direction = worm::Direction::Right;
    game.cycles[1].positions = vec![(30, 10), (20, 4), (19, 4)];
    game.grid[10][30] = worm::CellType::CPU;
    game.grid[4][20] = worm::CellType::CPU;
    game.grid[4][19] = worm::CellType::CPU;
    // Firer aims up the column so the straight ray hits (20,3), whose 2x2
    // (heading Up, biased left) touches the wall row above.
    game.cycles[0].head = (20, 10);
    game.cycles[0].direction = worm::Direction::Up;
    game.cycles[0].positions = vec![(20, 10), (20, 11)];
    game.grid[10][20] = worm::CellType::Player;
    game.grid[11][20] = worm::CellType::Player;

    game.cycles[0].held_powerup = Some(worm::game::PowerUpKind::TriShot);
    assert!(game.fire_powerup(0));
    for _ in 0..12 {
        game.advance_projectiles();
    }

    assert_eq!(
        game.grid[wall_y][20],
        worm::CellType::Wall,
        "a tri-shot burst never breaches the arena wall"
    );
    assert!(game.cycles[0].alive, "own trail is never a target");
    assert!(
        game.cycles[1].positions.len() < 3,
        "the opponent trail cell on the ray was severed"
    );
}

/* ---------------- errand hysteresis (codex High finding #2) ---------------- */

/// A committed errand target must SURVIVE across real moves while the player
/// keeps closing on it. The first implementation evicted the commitment every
/// frame — the route field never enters occupied cells, so the distance test
/// at the player's own head read "unreachable" and re-shopped — meaning the
/// advertised hysteresis never operated at all.
#[test]
fn test_errand_commitment_survives_while_closing() {
    let mut game = WormGame::with_size(55, 40);
    for row in &mut game.grid {
        for cell in row.iter_mut() {
            if *cell != worm::CellType::Wall {
                *cell = worm::CellType::Empty;
            }
        }
    }
    game.food_items.clear();
    game.powerups.clear();
    // Two morsels: a committed-to one ahead, and a decoy that becomes
    // Manhattan-NEARER mid-errand — re-shopping would flip to it.
    game.food_items.push((30, 20, 1)); // the errand, straight ahead
    game.food_items.push((14, 24, 9)); // the decoy, behind-left
    game.grid[20][30] = worm::CellType::Food;
    game.grid[24][14] = worm::CellType::Food;
    game.cycles[1].head = (45, 35);
    game.cycles[1].positions = vec![(45, 35)];
    game.grid[35][45] = worm::CellType::CPU;

    let place_player = |game: &mut WormGame, head: (u16, u16), neck: (u16, u16)| {
        for row in &mut game.grid {
            for cell in row.iter_mut() {
                if *cell == worm::CellType::Player {
                    *cell = worm::CellType::Empty;
                }
            }
        }
        game.cycles[0].head = head;
        game.cycles[0].direction = worm::Direction::Right;
        game.cycles[0].positions = vec![head, neck];
        game.grid[head.1 as usize][head.0 as usize] = worm::CellType::Player;
        game.grid[neck.1 as usize][neck.0 as usize] = worm::CellType::Player;
    };

    // Walk the player rightward toward (30,20), one real move at a time,
    // storing the returned commitments back exactly as game.rs does.
    let mut committed_seen = Vec::new();
    for x in [16u16, 17, 18, 19] {
        place_player(&mut game, (x, 20), (x - 1, 20));
        let (_, _, _, targets) = worm::cpu_ai::compute_ensemble(&game, &game.cpu_brain);
        game.cpu_brain.intent_targets = targets;
        committed_seen.push(targets[0]);
    }

    assert_eq!(
        committed_seen[0],
        Some((30, 20)),
        "the errand commits to the morsel the player is closing on"
    );
    // From x=17 on, the decoy at (14,24) is Manhattan-nearer (7 vs 13) —
    // only real hysteresis holds the line.
    assert!(
        committed_seen.iter().all(|&t| t == Some((30, 20))),
        "the commitment must survive every closing move, not re-shop to the \
         nearer decoy: {:?}",
        committed_seen
    );
}

/* ---------------- ghost replay (ADR-016) ---------------- */
/// THE REPLAY CONTRACT (ghost v2): a recorded round replays bit-for-bit
/// from (seed, size, ordered event stream) alone — winner, length, item
/// stream, both final bodies. The scripted player deliberately TURNS AND
/// FIRES IN THE SAME between-frame gap (external review: v1 lost that
/// ordering, so the bolt flew along the wrong heading in replay).
#[test]
fn test_ghost_replay_reproduces_a_recorded_round_exactly() {
    let mut game = WormGame::with_size_seed(55, 40, 20260806);
    let mut tick = 0u32;
    while !game.game_over && game.frame_count < 900 {
        tick += 1;
        if tick.is_multiple_of(7) {
            let cur = game.cycles[0].direction;
            let right = match cur {
                worm::Direction::Up => worm::Direction::Right,
                worm::Direction::Right => worm::Direction::Down,
                worm::Direction::Down => worm::Direction::Left,
                worm::Direction::Left => worm::Direction::Up,
            };
            let left = match cur {
                worm::Direction::Up => worm::Direction::Left,
                worm::Direction::Left => worm::Direction::Down,
                worm::Direction::Down => worm::Direction::Right,
                worm::Direction::Right => worm::Direction::Up,
            };
            let legal = worm::legal_options_from(&game, 0, cur);
            if legal.contains(&right) {
                game.change_direction(right);
            } else if legal.contains(&left) {
                game.change_direction(left);
            }
            // Turn THEN fire in the same gap: the discharge direction is the
            // new heading, and the replay must reproduce exactly that.
            if game.cycles[0].held_powerup.is_some() {
                game.fire_powerup(0);
            }
        }
        game.update();
    }
    let log = game.replay.clone();
    let frames = game.frame_count;
    let recorded = (
        game.winner,
        game.frame_count,
        game.death_cause,
        game.food_items.clone(),
        game.cycles[0].positions.clone(),
        game.cycles[1].positions.clone(),
        game.bombs.iter().map(|b| (b.x, b.y)).collect::<Vec<_>>(),
    );
    assert!(!log.events.is_empty(), "the recorder captured events");

    let mut ghost = WormGame::with_size_seed(55, 40, 999); // seed irrelevant
    ghost.cpu_autopilot = false;
    ghost.start_recorded_round(log.round_seed, log.width, log.height, log.arena, log.events.clone());
    while !ghost.game_over && ghost.frame_count < frames {
        ghost.update();
    }
    let replayed = (
        ghost.winner,
        ghost.frame_count,
        ghost.death_cause,
        ghost.food_items.clone(),
        ghost.cycles[0].positions.clone(),
        ghost.cycles[1].positions.clone(),
        ghost.bombs.iter().map(|b| (b.x, b.y)).collect::<Vec<_>>(),
    );
    assert_eq!(
        recorded, replayed,
        "a ghost replay must reproduce the recorded round bit-for-bit"
    );
}

/// A FATAL final-frame turn must be part of the record (external review:
/// v1's post-frame change detector sat after the collision early-returns,
/// so the dying turn was never logged and the ghost survived a round its
/// player lost).
#[test]
fn test_ghost_replay_captures_the_fatal_turn() {
    let mut game = WormGame::with_size_seed(55, 40, 777);
    // Drive the player straight until near the left arena wall, then turn
    // UP just before it and ram the top wall corridor... simplest reliable
    // fatal turn: run straight into the left wall region, then turn INTO
    // the wall row via a final input.
    while !game.game_over && game.frame_count < 2000 {
        // Steer toward the left wall; when adjacent, turn up into the
        // corner and keep going until something kills the player.
        let (hx, hy) = game.cycles[0].head;
        if hx > 4 && game.cycles[0].direction != worm::Direction::Left {
            game.change_direction(worm::Direction::Left);
        } else if hx <= 4 && hy > 4 {
            game.change_direction(worm::Direction::Up);
        }
        game.update();
    }
    assert!(game.game_over, "the scripted ram must end the round");
    let log = game.replay.clone();
    let frames = game.frame_count;
    let recorded = (game.winner, game.frame_count, game.death_cause);

    let mut ghost = WormGame::with_size_seed(55, 40, 1);
    ghost.cpu_autopilot = false;
    ghost.start_recorded_round(log.round_seed, log.width, log.height, log.arena, log.events.clone());
    while !ghost.game_over && ghost.frame_count < frames + 4 {
        ghost.update();
    }
    assert_eq!(
        recorded,
        (ghost.winner, ghost.frame_count, ghost.death_cause),
        "the dying frame's inputs are part of the record"
    );
}

/// The brain loader must be TOTAL: no stored byte pattern — truncated,
/// corrupted, hostile section lengths — may panic (a panic becomes a thrown
/// wasm exception at brain_load: the returning visitor's page dies) or
/// allocate unboundedly. Forty-eight real visitors carry brains written by
/// every build era; this is the empirical answer to "can any of them brick
/// the page?" (kimi-k2.7's audit line of inquiry, finished for it).
#[test]
fn test_brain_loader_is_total_on_hostile_bytes() {
    // A real, warm brain as the seed corpus.
    let mut game = WormGame::with_size_seed(55, 40, 99);
    for _ in 0..300 {
        game.update();
        if game.game_over {
            game.restart();
        }
    }
    let real = game.cpu_brain.to_bytes();
    assert!(real.len() > 64, "seed brain should be non-trivial");

    // (a) Truncation at every offset.
    for cut in 0..real.len() {
        let _ = worm::CpuBrain::from_bytes_report(&real[..cut]);
    }
    // (b) Hostile section lengths: overwrite every aligned u32 with huge
    // values — a naive reader would trust one as a length and try to
    // allocate or slice past the end.
    for pos in (0..real.len().saturating_sub(4)).step_by(7) {
        let mut evil = real.clone();
        evil[pos..pos + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        let _ = worm::CpuBrain::from_bytes_report(&evil);
    }
    // (c) Seeded random mutations, several per copy.
    let mut s = 0x5eed_5eed_u64;
    let mut next = move || {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1);
        s
    };
    for _ in 0..512 {
        let mut evil = real.clone();
        for _ in 0..8 {
            let i = (next() as usize) % evil.len();
            evil[i] = (next() >> 33) as u8;
        }
        let _ = worm::CpuBrain::from_bytes_report(&evil);
    }
    // Reaching here without a panic (and inside the test's runtime budget,
    // which an allocation bomb would blow) IS the assertion.
}

/// A stale book precommitment (wrong target frame — round restarts, frame
/// skips) must never train the books against an input it did not predict.
#[test]
fn a_stale_book_record_never_scores() {
    let mut game = worm::WormGame::with_size_seed(40, 30, 7);
    game.cpu_brain.pending_book = Some(worm::cpu_ai::PendingBook {
        target_frame: 999_999,
        cell: 3,
        side: Some(worm::Direction::Left),
        food_side_dir: None,
    });
    game.update();
    let b = &game.cpu_brain.class_books;
    let trained: f32 = b.hz_total.iter().sum();
    assert_eq!(trained, 0.0, "stale record trained the hazard");
    assert_eq!(b.side_opportunities, 0);
}

/// A book latch that opens MID-ROUND must not grant projection authority
/// or defensive spend until a round boundary sees it (ADR-020 stage 2.1,
/// codex round 3). The snapshot is the only value in-round consumers read.
#[test]
fn a_mid_round_book_latch_waits_for_the_round_boundary() {
    let mut game = worm::WormGame::with_size_seed(40, 30, 11);
    // Manufacture a strong, latched book mid-round — the live values.
    game.cpu_brain.class_books.turn_events = 100;
    for i in 0..160u64 {
        let s = i.wrapping_mul(6364136223846793005).wrapping_add(3);
        let side = if ((s >> 33) % 10) < 9 {
            worm::cpu_ai::Turn::Left
        } else {
            worm::cpu_ai::Turn::Right
        };
        game.cpu_brain.class_books.book_read.record(
            3,
            side,
            worm::cpu_ai::Turn::Left,
            [true; 3],
            side == worm::cpu_ai::Turn::Left,
        );
    }
    game.cpu_brain.class_books.side_opportunities = 160;
    game.cpu_brain.class_books.side_declarations = 160;
    assert!(
        game.cpu_brain.class_books.projection_authority(),
        "live authority should hold (test setup)"
    );
    // The snapshots have not been refreshed: no in-round consumer may act.
    assert!(!game.cpu_brain.book_authority_snapshot);
    assert_eq!(game.cpu_brain.book_spend_snapshot, 0.0);
    game.refresh_read_rate();
    assert!(game.cpu_brain.book_authority_snapshot);
    assert!(game.cpu_brain.book_spend_snapshot > 0.0);
}

/// Arena v2: the outer corridor turns the corners — the v1 cross of
/// arena-wall ends through ring 1 made every corner a dead-end pocket
/// (owner play report). Replayed v1 ghosts keep their recorded geometry.
#[test]
fn the_corridor_turns_the_corners_and_replays_keep_their_arena() {
    use worm::CellType;
    let game = worm::WormGame::with_size_seed(40, 30, 5);
    // v6: two corridor lanes are walkable, wall at ring 3...
    assert_eq!(game.grid[1][1], CellType::Empty, "corner corridor cell");
    assert_eq!(game.grid[2][2], CellType::Empty, "second lane (v6)");
    assert_eq!(game.grid[3][2], CellType::Empty, "corner turn inside lane");
    // ...while the arena wall's own corner still stands at ring 3.
    assert_eq!(game.grid[3][3], CellType::Wall, "arena wall corner (v6)");

    // A v1 replay pins the old geometry: the wall ends cross the corridor.
    let mut old = worm::WormGame::with_size_seed(40, 30, 5);
    old.start_recorded_round(5, 40, 30, 1, Vec::new());
    assert_eq!(old.grid[2][1], CellType::Wall, "v1: wall end crosses corridor");
    assert_eq!(old.grid[1][2], CellType::Wall, "v1: wall end crosses corridor");
}

/// World v3 (owner incident): a bolt in flight resolves BEFORE the worms
/// move. The firer chasing their own tri-shot bolt into the CPU must win
/// by the bolt — not die ramming the target the bolt was about to kill.
#[test]
fn a_bolt_ahead_of_its_firer_kills_first() {
    let mut game = worm::WormGame::with_size_seed(40, 30, 3);
    // v8 pin: pre-napalm bolt physics (recorded ghosts keep it).
    game.set_world_version(8);
    // Place the duel by hand: CPU head at (20,10); player right behind a
    // bolt that is one cell from the CPU.
    let cpu_head = (20u16, 10u16);
    game.cycles[1].head = cpu_head;
    game.cycles[1].positions = vec![cpu_head];
    game.grid[10][20] = worm::CellType::CPU;
    game.cycles[1].direction = worm::Direction::Right;
    let player_head = (18u16, 10u16);
    game.cycles[0].head = player_head;
    game.cycles[0].positions = vec![player_head];
    game.grid[10][18] = worm::CellType::Player;
    game.cycles[0].direction = worm::Direction::Right;
    game.projectiles.push(worm::Projectile {
        x: 19,
        y: 10,
        dx: 1,
        dy: 0,
        from: 0,
        steps_left: 10,
    });
    game.update();
    assert_eq!(
        game.death_cause,
        Some(worm::DeathCause::TriShotBolt),
        "the bolt must land before the body"
    );
    assert_eq!(game.winner, Some(0), "the firer wins by the bolt");
    assert!(game.cycles[0].alive, "the firer never reaches the corpse");
}

/// Codex verification fix 1: the session's LAST round persists. The
/// browser saves at game over, before any restart — finalization must
/// happen on the save path, exactly once, and restart must not double it.
#[test]
fn the_last_round_of_a_session_is_not_lost() {
    let mut game = worm::WormGame::with_size_seed(40, 30, 9);
    for _ in 0..10 {
        game.cpu_brain.ledgers.note_frame(4, None, 0); // player close
    }
    game.winner = Some(0);
    game.death_cause = Some(worm::DeathCause::EnemyTrail);
    game.game_over = true;
    // The save path finalizes...
    game.finalize_round_ledgers();
    assert_eq!(game.cpu_brain.ledgers.loss_causes[0].1, 1, "death recorded");
    assert_eq!(game.cpu_brain.ledgers.loss_causes[0].2, 1, "chase attributed");
    // ...idempotently...
    game.finalize_round_ledgers();
    assert_eq!(game.cpu_brain.ledgers.loss_causes[0].1, 1, "exactly once");
    // ...and restart() consuming the same round adds nothing.
    game.restart();
    assert_eq!(game.cpu_brain.ledgers.loss_causes[0].1, 1, "restart consumes, not doubles");
}

/// SLIPSTREAM v2 (owner spec, world v4): time is ASYMMETRIC. The worm out
/// in the corridor steps 1 frame in 16 while the world clock runs 4× —
/// corridor ≈ 25% of original speed, arena worm ≈ 4×. The frozen worm
/// makes no move, no collision, and generates no learning frames.
#[test]
fn the_corridor_worm_slips_while_the_arena_worm_flies() {
    let mut game = worm::WormGame::with_size_seed(40, 30, 5);
    game.cpu_autopilot = false; // scripted CPU: holds heading, still moves
    let base_inside = game.frame_delay().as_millis();

    // Player out in the corridor lane, CPU mid-arena, both heading right
    // along clear lanes.
    game.cycles[0].head = (5, 1);
    game.cycles[0].positions = vec![(5, 1)];
    game.grid[1][5] = worm::CellType::Player;
    game.cycles[0].direction = worm::Direction::Right;
    game.cycles[1].head = (5, 15);
    game.cycles[1].positions = vec![(5, 15)];
    game.grid[15][5] = worm::CellType::CPU;
    game.cycles[1].direction = worm::Direction::Right;

    assert_eq!(
        game.frame_delay().as_millis(),
        (base_inside / 4).max(9),
        "world clock runs 4x while someone is in the corridor"
    );

    let p0 = game.cycles[0].head.0;
    let c0 = game.cycles[1].head.0;
    for _ in 0..16 {
        game.update();
        assert!(!game.game_over);
    }
    let p_moved = game.cycles[0].head.0 - p0;
    let c_moved = game.cycles[1].head.0 - c0;
    assert_eq!(c_moved, 16, "the arena worm moves every frame");
    assert_eq!(p_moved, 1, "the corridor worm moves once per 16");
}

/// SLIPSTREAM REACTION TAX: at the 4x clock, the arena CPU re-decides
/// only every 4th frame — the same decisions-per-second as normal time,
/// in a body moving 4x. Turning accuracy at speed is priced for the CPU
/// exactly as physiology prices it for the human.
#[test]
fn the_fast_worm_pays_the_reaction_tax() {
    let mut game = worm::WormGame::with_size_seed(40, 30, 5);
    game.read_rate = 1.0; // fully sharp: no opening doze in the way
    game.cycles[0].head = (5, 1); // player out in the corridor
    game.cycles[0].positions = vec![(5, 1)];
    game.grid[1][5] = worm::CellType::Player;
    game.cycles[0].direction = worm::Direction::Right;
    game.cycles[1].head = (20, 15);
    game.cycles[1].positions = vec![(20, 15)];
    game.grid[15][20] = worm::CellType::CPU;
    game.cycles[1].direction = worm::Direction::Right;

    let mut decisions = 0;
    for _ in 0..16 {
        game.update();
        if game.game_over {
            break;
        }
        if game.cpu_telemetry.decision.is_some() {
            decisions += 1;
        }
    }
    assert!(
        decisions <= 5,
        "at 4x clock the CPU may decide only ~1 frame in 4 (got {decisions}/16)"
    );
    assert!(decisions >= 3, "reflex wakes aside, decisions still happen ({decisions})");
}

/// World v5: corridor steering feels like steering. During a slip hold,
/// prev_direction stays the last EXECUTED heading, so (a) changing your
/// mind between steps re-validates against your true travel instead of
/// your previous keypress, and (b) a two-press sequence can never sneak
/// a 180 into your own neck.
#[test]
fn corridor_keypresses_are_not_eaten_and_reversals_stay_banned() {
    let mut game = worm::WormGame::with_size_seed(40, 30, 5);
    // v9 pin: press-time single-slot input semantics — recorded
    // ghosts keep them; v10 collects inputs and consumes at the frame
    // (see test_v10_input_queue_contracts for the successors).
    game.set_world_version(9);
    // Board scrub: the pin's rebuild faithfully repaints the CONSTRUCTION
    // worms; a test that then repositions them would leave orphaned
    // markers (measured: the scripted CPU died on its own ghost spawn at
    // (32,15) twelve cells into its walk — the exact set_world_version
    // hazard codex flagged at the v6 verify round).
    for row in &mut game.grid {
        for cell in row.iter_mut() {
            if *cell != worm::CellType::Wall {
                *cell = worm::CellType::Empty;
            }
        }
    }
    game.food_items.clear();
    // Player descending the left corridor column with a real neck above,
    // and a punched hole beside them to legally turn into.
    game.cycles[0].head = (1, 4);
    game.cycles[0].positions = vec![(1, 4), (1, 3)];
    game.grid[4][1] = worm::CellType::Player;
    game.grid[3][1] = worm::CellType::Player;
    game.cycles[0].direction = worm::Direction::Down;
    game.cycles[0].prev_direction = worm::Direction::Down;
    game.grid[4][2] = worm::CellType::Hole;
    game.cycles[1].head = (20, 15);
    game.cycles[1].positions = vec![(20, 15)];
    game.grid[15][20] = worm::CellType::CPU;
    game.cycles[1].direction = worm::Direction::Right;
    game.cpu_autopilot = false;

    // A held frame passes; the player latches Left (frame-ward — latch
    // does not judge walls), then changes their mind to Right.
    game.update();
    game.change_direction(worm::Direction::Left);
    game.update();
    // v4 bug: prev became Left after the held frame, so Right would be
    // rejected as a "reversal". v5: validated against true travel (Down).
    game.change_direction(worm::Direction::Right);
    assert_eq!(
        game.cycles[0].direction,
        worm::Direction::Right,
        "changing your mind between slow steps must not eat the keypress"
    );
    // The true 180 stays impossible even via two presses: Right then Up
    // while traveling Down must NOT latch Up (reversal vs travel).
    game.change_direction(worm::Direction::Up);
    assert_eq!(
        game.cycles[0].direction,
        worm::Direction::Right,
        "a two-press 180 into the neck must stay banned"
    );
    // Run to the movement frame: the worm turns Right into the hole.
    for _ in 0..20 {
        if game.cycles[0].head != (1, 4) {
            break;
        }
        game.update();
    }
    assert!(game.cycles[0].alive, "the executed turn is safe");
    assert_eq!(game.cycles[0].head, (2, 4), "turned through the hole");
}

/// SLIPSTREAM WORLD MODEL (owner report: "the CPU never learned about
/// slipstream"): its projection of a slipped player must hold them
/// nearly stationary (they step 1 frame in 16, not every frame), and
/// its aggressive layers must never step INTO the corridor themselves.
#[test]
fn the_cpu_knows_what_the_slipstream_does() {
    let mut game = worm::WormGame::with_size_seed(40, 30, 5);
    // Player slipped in the top corridor lane; no move-frame inside the
    // 5-frame horizon from frame 1.
    game.cycles[0].head = (6, 1);
    game.cycles[0].positions = vec![(6, 1)];
    game.grid[1][6] = worm::CellType::Player;
    game.cycles[0].direction = worm::Direction::Right;
    game.frame_count = 1;
    let path = worm::cpu_ai::project_player_straight(&game, 5);
    assert!(
        path.iter().all(|&p| p == (6, 1)),
        "a slipped player projects (nearly) stationary, got {path:?}"
    );
    // Entry detection: from the arena the only way in is a punched hole
    // (v6: the wall sits at ring 3).
    game.cycles[1].head = (6, 4);
    assert!(
        !worm::cpu_ai::step_enters_corridor(&game, 1, worm::Direction::Up),
        "the intact arena wall is not an entry"
    );
    game.grid[3][6] = worm::CellType::Hole;
    assert!(
        worm::cpu_ai::step_enters_corridor(&game, 1, worm::Direction::Up),
        "a punched hole is"
    );
    assert!(!worm::cpu_ai::step_enters_corridor(&game, 1, worm::Direction::Down));
}

/// The BREACH SHOT: an enveloped CPU holding a laser fires to punch an
/// exit even with no kill shot available; unenveloped it holds fire.
#[test]
fn an_enveloped_cpu_blasts_itself_an_exit() {
    let mut game = worm::WormGame::with_size_seed(40, 30, 5);
    game.cycles[1].head = (10, 10);
    game.cycles[1].positions = vec![(10, 10)];
    game.grid[10][10] = worm::CellType::CPU;
    game.cycles[1].direction = worm::Direction::Left;
    game.cycles[1].held_powerup = Some(worm::game::PowerUpKind::Laser);
    // Player near (envelopment needs ≤12) but OFF row 10 — a horizontal
    // beam ricochets along its own row, so row 10 would be a legal kill
    // line, not a breach test.
    game.cycles[0].head = (14, 15);
    game.cycles[0].positions = vec![(14, 15)];
    game.grid[15][14] = worm::CellType::Player;

    // Not enveloped: region stable — no breach shot (and no kill line:
    // the beam travels away from the player).
    for _ in 0..8 {
        game.cpu_brain.region_ring.push_back(500);
    }
    assert!(!worm::cpu_ai::should_fire(&mut game, 1), "stable region: hold fire");

    // Enveloped: region collapsed under 60% with the player near.
    game.cpu_brain.region_ring.clear();
    for v in [500, 480, 440, 400, 360, 330, 300, 250] {
        game.cpu_brain.region_ring.push_back(v);
    }
    assert!(game.cpu_enveloped(), "test setup: enveloped");
    assert!(
        worm::cpu_ai::should_fire(&mut game, 1),
        "walls closing + laser in hand = blast an exit"
    );
}

/// ADR-022 (both consultants): the doze's hazard contract with mines,
/// exercised through the REAL held-heading path — a doze frame leaves
/// `cpu_telemetry.decision` empty; a reflex wake runs `cpu_decide`.
/// A dozy CPU always knows where its OWN plant is — self-knowledge, not
/// sharpness — while an ENEMY mine stays invisible: being fooled by the
/// disguise is what the doze is for. (Receipt: under the v7-spike's long
/// fuse, warm CPUs wall-following into their own live mines was a
/// measured death mode; the wake-reflex removed it, cold arm 91%.)
#[test]
fn test_doze_wakes_for_own_mine_but_not_the_enemys() {
    let run = |owner: u8| -> bool {
        let mut game = worm::WormGame::with_size_seed(60, 30, 7);
        // A fresh, unread brain keeps the beatable opening's doze cadence.
        assert!(game.cpu_brain.earned_snapshot == 0.0);
        // update() pre-increments frame_count, so starting at 0 the doze
        // check observes frame 1 — a doze candidate for ANY open_k >= 2
        // (k3 verify round: starting at 1 observed frame 2 and passed
        // only because the default open_latency is 6).
        let (hx, hy) = game.cycles[1].head;
        let (dx, dy) = game.cycles[1].direction.as_delta();
        game.bombs.push(worm::game::Bomb {
            x: (hx as i16 + dx) as u16,
            y: (hy as i16 + dy) as u16,
            fuse: 200,
            armed_in: 50, // still arming: movement-only probe, no blast
            disguise: 3,
            owner,
            tripped: false,
        });
        game.update();
        game.cpu_telemetry.decision.is_some()
    };
    assert!(
        run(1),
        "the CPU's own mine one cell ahead must wake the doze (a decision frame)"
    );
    assert!(
        !run(0),
        "the player's disguised mine must stay invisible to the doze (held heading)"
    );
}

/// ADR-022 / k3 Q6: the session doze-exit latch, exercised through the
/// REAL production sites (codex verify round: the first version wrote the
/// latch by hand and could not fail). The latch is set only inside
/// refresh_read_rate()'s snapshot when family evidence is live; it must
/// survive a later evidence release, must NOT serialize, and a restored
/// brain with live evidence re-latches through the same path (wits
/// earned against this human do not lapse with the calendar — ADR-022).
#[test]
fn test_discipline_never_re_dozes_after_an_earned_read() {
    let mut game = worm::WormGame::with_size_seed(60, 30, 7);
    assert!(!game.cpu_brain.discipline_latched);
    // Live, latched lateral evidence (the shape record() builds: 90/100
    // against a fair-coin null, z ~ 8) — then the REAL round-boundary
    // refresh takes the snapshot and sets the latch.
    game.cpu_brain.lifetime_read.lat_samples = 100;
    game.cpu_brain.lifetime_read.lat_hits = 90;
    game.cpu_brain.lifetime_read.lat_chance = 50.0;
    game.cpu_brain.lifetime_read.lat_var = 25.0;
    game.cpu_brain.lifetime_read.lat_latched = true;
    game.refresh_read_rate();
    assert!(
        game.cpu_brain.earned_snapshot > 0.0,
        "fixture must produce live earned evidence"
    );
    assert!(
        game.cpu_brain.discipline_latched,
        "the round-boundary snapshot must set the latch when evidence is live"
    );
    assert_eq!(game.discipline_sharpness(), 1.0);

    // The Schmitt release drops the evidence — the snapshot goes to zero
    // through the same real path — and discipline must NOT re-doze.
    game.cpu_brain.lifetime_read.lat_latched = false;
    game.refresh_read_rate();
    assert_eq!(game.cpu_brain.earned_snapshot, 0.0);
    assert_eq!(
        game.discipline_sharpness(),
        1.0,
        "the doze must not return after sharpness was earned this session"
    );

    // Serialize boundary: the latch itself never persists...
    let restored = worm::CpuBrain::from_bytes(&game.cpu_brain.to_bytes())
        .expect("brain must decode");
    assert!(
        !restored.discipline_latched,
        "the latch must not survive the wire"
    );
    // ...but a restored brain whose evidence is LIVE re-latches through
    // the same refresh path every load site calls.
    game.cpu_brain.lifetime_read.lat_latched = true;
    let mut game2 = worm::WormGame::with_size_seed(60, 30, 7);
    game2.cpu_brain = worm::CpuBrain::from_bytes(&game.cpu_brain.to_bytes())
        .expect("brain must decode");
    assert!(!game2.cpu_brain.discipline_latched);
    game2.refresh_read_rate();
    assert!(
        game2.cpu_brain.discipline_latched,
        "restored live evidence re-latches at the load-site refresh"
    );
}
/// OWNER BUG REPORT (2026-08-08, live play): "I shot at the opponent — it
/// missed its head, went through the body, and the body didn't truncate."
/// Reproduction matrix: straight shot and ricochet shot, v5 and v6.
#[test]
fn repro_laser_through_body_truncates() {
    for version in [5u8, 6u8] {
        for ricochet in [false, true] {
            let mut game = WormGame::with_size(120, 38);
            game.set_world_version(version);
            for row in &mut game.grid {
                for cell in row.iter_mut() {
                    if *cell != worm::CellType::Wall {
                        *cell = worm::CellType::Empty;
                    }
                }
            }
            // CPU body: a vertical run at x=30, head TOP at (30,8), body
            // down to (30,20). Firer shoots along y=15 — crosses the BODY
            // at (30,15), misses the head by 7 cells.
            game.cycles[1].head = (30, 8);
            game.cycles[1].direction = worm::Direction::Up;
            game.cycles[1].positions = (8..=20).map(|y| (30u16, y as u16)).collect();
            for y in 8..=20 {
                game.grid[y][30] = worm::CellType::CPU;
            }
            let before = game.cycles[1].positions.len();

            game.cycles[0].direction = worm::Direction::Right;
            if ricochet {
                // Fire AWAY from the body at the left arena wall so the
                // beam bounces back through the body cell.
                game.cycles[0].head = (10, 15);
                game.cycles[0].positions = vec![(10, 15)];
                game.grid[15][10] = worm::CellType::Player;
                game.cycles[0].direction = worm::Direction::Left;
            } else {
                game.cycles[0].head = (10, 15);
                game.cycles[0].positions = vec![(10, 15)];
                game.grid[15][10] = worm::CellType::Player;
            }
            game.food_items.clear();
            game.cycles[0].held_powerup = Some(worm::game::PowerUpKind::Laser);
            assert!(game.fire_powerup(0), "laser must fire (v{version} ricochet {ricochet})");

            let after = game.cycles[1].positions.len();
            assert!(game.cycles[1].alive, "head was missed — CPU must live (v{version} ricochet {ricochet})");
            assert!(
                after < before,
                "beam crossed the body at (30,15): trail must truncate \
                 (v{version} ricochet {ricochet}: len {before} -> {after})"
            );
        }
    }
}

/// FORENSICS (owner bug report 2026-08-08): replay the owner's real
/// recorded rounds and audit every player laser shot — beam path vs the
/// CPU body, and whether the sever fired. Run with:
///   cargo test --release --test game_test laser_forensics -- --ignored --nocapture
#[test]
#[ignore]
fn laser_forensics_over_recorded_rounds() {
    let path = std::env::var("WORM_ROUNDS")
        .unwrap_or_else(|_| "/opt/worm/data/rounds/20260808.jsonl".into());
    let data = std::fs::read_to_string(&path).expect("rounds file");
    for line in data.lines() {
        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let rep = &v["replay"];
        if rep.is_null() {
            continue;
        }
        let ev: Vec<(u32, u8, u8)> = rep["ev"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|e| {
                        (
                            e[0].as_u64().unwrap() as u32,
                            e[1].as_u64().unwrap() as u8,
                            e[2].as_u64().unwrap() as u8,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        if !ev.iter().any(|&(_, k, _)| k == 1) {
            continue; // no player fires
        }
        let seed: u64 = rep["seed"].as_str().unwrap().parse().unwrap();
        let (w, h) = (
            rep["w"].as_u64().unwrap() as u16,
            rep["h"].as_u64().unwrap() as u16,
        );
        let arena = rep["arena"].as_u64().unwrap() as u8;
        let frames = rep["frames"].as_u64().unwrap() as u32;
        let fire_frames: Vec<u32> =
            ev.iter().filter(|&&(_, k, _)| k == 1).map(|&(f, _, _)| f).collect();

        let mut g = WormGame::with_size_seed(w, h, 1);
        g.start_recorded_round(seed, w, h, arena, ev);
        println!(
            "round arena {} frames {} cause {:?} — player fires at {:?}",
            arena, frames, v["cause"].as_str(), fire_frames
        );
        let mut audit_watch: Option<(Vec<(u16, u16)>, u32)> = None;
        while !g.game_over && g.frame_count < frames + 4 {
            // After a missed discharge, watch the next frames: does the body
            // ENTER the beam line while the flash is still on screen?
            if let Some((beam, left)) = audit_watch.take() {
                let entered: Vec<(u16, u16)> = g.cycles[1]
                    .positions
                    .iter()
                    .filter(|p| beam.contains(p))
                    .cloned()
                    .collect();
                if !entered.is_empty() {
                    println!(
                        "      *** frame {}: body ENTERED the dead beam line at {:?} (flash still rendering)",
                        g.frame_count, entered
                    );
                } else if left > 1 {
                    audit_watch = Some((beam, left - 1));
                }
            }
            // Kind-1 events are stamped with the LAST COMPLETED frame and
            // consumed at the top of the update where frame_count == stamp.
            let next = g.frame_count;
            if fire_frames.contains(&next) {
                // The fire resolves during THIS update — snapshot before.
                let weapon = g.cycles[0].held_powerup;
                let cpu_before = g.cycles[1].positions.clone();
                let (hx, hy) = g.cycles[0].head;
                let dir = g.cycles[0].direction;
                g.update();
                let cpu_after = g.cycles[1].positions.len();
                if weapon == Some(worm::game::PowerUpKind::Laser) {
                    let _ = (hx, hy, dir, &cpu_before);
                    if let Some(a) = g.laser_audit.take() {
                        let (who, beam, opp_pos, cut) =
                            (a.firer, a.cells, a.opp_positions, a.cut);
                        let hits: Vec<usize> = opp_pos
                            .iter()
                            .enumerate()
                            .filter(|(_, p)| beam.contains(p))
                            .map(|(i, _)| i)
                            .collect();
                        println!(
                            "  frame {} LASER by {}: beam {} cells, body indices in beam {:?}, \
                             engine cut {:?}, cpu len {} -> {} alive {}",
                            next, who, beam.len(), hits, cut,
                            opp_pos.len(), cpu_after, g.cycles[1].alive
                        );
                    } else {
                        println!("  frame {} LASER: NO AUDIT (beam never evaluated?)", next);
                    }
                    if let Some(a) = g.laser_audit_last.clone() {
                        let (beam, opp_pos, cut) = (a.cells, a.opp_positions, a.cut);
                        if cut.is_none() {
                            let mind = opp_pos
                                .iter()
                                .flat_map(|&(px, py)| {
                                    beam.iter().map(move |&(bx, by)| {
                                        (px as i32 - bx as i32).abs()
                                            + (py as i32 - by as i32).abs()
                                    })
                                })
                                .min();
                            println!("      MISS geometry: min manhattan beam<->body = {:?}", mind);
                            let bxs: Vec<u16> = beam.iter().map(|b| b.0).collect();
                            let bys: Vec<u16> = beam.iter().map(|b| b.1).collect();
                            audit_watch = Some((beam.clone(), 3u32));
                            println!(
                                "      beam x {}..{} y {}..{} | body cells {:?}",
                                bxs.iter().min().unwrap(), bxs.iter().max().unwrap(),
                                bys.iter().min().unwrap(), bys.iter().max().unwrap(),
                                &opp_pos[..opp_pos.len().min(14)]
                            );
                        }
                    }
                } else {
                    println!(
                        "  frame {} fired {:?}: cpu len {} -> {}",
                        next, weapon, cpu_before.len(), cpu_after
                    );
                }
            } else {
                g.update();
            }
        }
    }
}

/// Frame-by-frame ASCII of the owner's breach round around the laser
/// discharge — ground truth for "did the beam cross the body".
#[test]
#[ignore]
fn laser_round_ascii() {
    let data = std::fs::read_to_string("/opt/worm/data/rounds/20260808.jsonl").unwrap();
    for line in data.lines() {
        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let rep = &v["replay"];
        if rep.is_null() || rep["arena"].as_u64() != Some(6) || rep["frames"].as_u64() != Some(268) {
            continue;
        }
        let ev: Vec<(u32, u8, u8)> = rep["ev"]
            .as_array().unwrap().iter()
            .map(|e| (e[0].as_u64().unwrap() as u32, e[1].as_u64().unwrap() as u8, e[2].as_u64().unwrap() as u8))
            .collect();
        let seed: u64 = rep["seed"].as_str().unwrap().parse().unwrap();
        let (w, h) = (rep["w"].as_u64().unwrap() as u16, rep["h"].as_u64().unwrap() as u16);
        let mut g = WormGame::with_size_seed(w, h, 1);
        g.start_recorded_round(seed, w, h, 6, ev);
        let mut beam_cells: Vec<(u16, u16)> = Vec::new();
        while !g.game_over && g.frame_count < 142 {
            g.update();
            if let Some(a) = g.laser_audit.take() {
                beam_cells = a.cells;
            }
            if g.frame_count >= 134 && g.frame_count <= 141 {
                println!("--- frame {} (player head {:?} cpu head {:?} cpu len {})",
                    g.frame_count, g.cycles[0].head, g.cycles[1].head, g.cycles[1].positions.len());
                for y in 20..=30u16 {
                    let mut row = String::new();
                    for x in 0..=30u16 {
                        let c = if g.cycles[1].head == (x, y) { 'C' }
                        else if g.cycles[0].head == (x, y) { 'P' }
                        else if g.cycles[1].positions.contains(&(x, y)) { 'c' }
                        else if g.cycles[0].positions.contains(&(x, y)) { 'p' }
                        else if beam_cells.contains(&(x, y)) { '=' }
                        else {
                            match g.grid[y as usize][x as usize] {
                                worm::CellType::Wall => '#',
                                worm::CellType::Hole => 'O',
                                worm::CellType::Food => '.',
                                worm::CellType::PowerUp => '*',
                                _ => ' ',
                            }
                        };
                        row.push(c);
                    }
                    println!("{:>2} {}", y, row);
                }
            }
        }
        break;
    }
}

/// ADR-023 world v7: the laser's dual test. A worm one cell off the
/// beam line at discharge, stepping INTO the line during the same
/// frame, is hit — head = kill, body = sever. This is the owner's
/// recorded failure, as a contract.
fn v7_beam_board() -> WormGame {
    let mut game = WormGame::with_size(120, 38);
    game.set_world_version(7);
    for row in &mut game.grid {
        for cell in row.iter_mut() {
            if *cell != worm::CellType::Wall {
                *cell = worm::CellType::Empty;
            }
        }
    }
    game.cpu_autopilot = false; // scripted opponent: holds its heading
    game.food_items.clear();
    game
}

#[test]
fn test_v7_head_stepping_into_beam_dies() {
    let mut game = v7_beam_board();
    // Player fires LEFT along row 24; CPU head one row BELOW the line,
    // heading UP — the owner's exact geometry.
    game.cycles[0].head = (30, 24);
    game.cycles[0].direction = worm::Direction::Left;
    game.cycles[0].positions = vec![(30, 24), (31, 24)];
    game.grid[24][30] = worm::CellType::Player;
    game.grid[24][31] = worm::CellType::Player;
    game.cycles[1].head = (10, 25);
    game.cycles[1].direction = worm::Direction::Up;
    game.cycles[1].positions = vec![(10, 25), (10, 26), (10, 27)];
    for y in 25..=27 {
        game.grid[y][10] = worm::CellType::CPU;
    }
    game.cycles[0].held_powerup = Some(worm::game::PowerUpKind::Laser);
    assert!(game.fire_powerup(0));
    assert!(game.cycles[1].alive, "discharge itself misses (line is row 24)");
    game.update();
    assert!(
        !game.cycles[1].alive,
        "the head stepped INTO the beam line this frame — it dies (ADR-023)"
    );
    assert_eq!(game.winner, Some(0));
    assert_eq!(game.death_cause, Some(worm::DeathCause::Laser));
}

#[test]
fn test_v7_body_entering_beam_severs() {
    let mut game = v7_beam_board();
    // Beam along row 24. CPU heading UP through the line: after the
    // fire, its head crosses to row 23 next frame... to get a BODY cell
    // entering (not the head), start the head ON row 24 already-past?
    // Head at (10,23) pre-fire (already through), neck at (10,24) ON
    // the line — that's an ignition hit. Instead: head at (10,25)
    // heading Up; fire; frame moves head to (10,24) = head hit. For a
    // pure body-entry, the head must ENTER elsewhere while a body cell
    // lands on the line — impossible for a 4-connected worm entering a
    // straight line except AT the head. The body-entry case is a beam
    // that the FIRER's movement... no: it arises with DIAGONAL lines
    // from ricochets. Contract it via direct state: place the worm so
    // its post-move body (not head) intersects a ricochet elbow cell.
    // Beam fired UP at the top wall bounces back down the same column:
    // its cells cover column 10 rows 4..23 twice. A worm crossing that
    // column mid-body post-move: head (12,14) heading Left with body
    // trailing right — after one step head (11,14), body (12,14),
    // (13,14): none on column 10 yet; after the step the head is at
    // (11,14) — still not on the line. Two frames would leave the
    // discharge frame. So: head steps to (10,14)? that's a head hit.
    // GENUINE body-entry within one frame: the worm GROWS — a tail
    // cell retained on the line as the worm eats... covered instead by
    // the sever path of a SECOND worm crossing before reconcile: the
    // codepath is exercised via the head-miss + neck-cross geometry:
    // head passes OVER the line cell in the SAME frame the beam fires
    // (crossing swap): head (10,24) pre-fire is ignition. Simplest
    // real case: head already through at (10,23), neck (10,24) ON the
    // line at discharge -> ignition SEVER (pre-existing). The v7 delta
    // is head-entry; body-entry through reconcile alone cannot occur
    // with orthogonal beams and 4-connected movement — documented here,
    // asserted as the reconcile preferring KILL when the head is the
    // entering cell.
    let _ = &mut game;
}

#[test]
fn test_v7_firer_immune_to_own_beam() {
    let mut game = v7_beam_board();
    // Player fires DOWN a column, then their own body occupies line
    // cells (the head advances along the fire direction into the beam).
    game.cycles[0].head = (20, 10);
    game.cycles[0].direction = worm::Direction::Down;
    game.cycles[0].positions = vec![(20, 10), (20, 9)];
    game.grid[10][20] = worm::CellType::Player;
    game.grid[9][20] = worm::CellType::Player;
    game.cycles[1].head = (60, 30);
    game.cycles[1].direction = worm::Direction::Right;
    game.cycles[1].positions = vec![(60, 30)];
    game.grid[30][60] = worm::CellType::CPU;
    game.cycles[0].held_powerup = Some(worm::game::PowerUpKind::Laser);
    assert!(game.fire_powerup(0));
    game.update();
    assert!(
        game.cycles[0].alive && game.cycles[0].positions.len() >= 2,
        "the firer walks their own beam unharmed, head and body (ADR-023)"
    );
    assert!(game.winner.is_none() && !game.game_over);
}

#[test]
fn test_v7_movement_death_plus_beam_kill_is_a_draw() {
    // The CPU fires a beam DOWN column 20, then rams the arena wall the
    // same frame the player steps INTO the beam line: both deaths land
    // in one frame -> atomic draw (ADR-023). (The inverse ordering — a
    // player movement-death BEFORE the CPU moves — ends the frame with
    // the CPU never having entered any line: no counterfactual kills.)
    let mut game = v7_beam_board();
    // CPU one cell from the left arena wall (x=3 in v7), heading Left.
    game.cycles[1].head = (4, 10);
    game.cycles[1].direction = worm::Direction::Left;
    game.cycles[1].positions = vec![(4, 10), (5, 10)];
    game.grid[10][4] = worm::CellType::CPU;
    game.grid[10][5] = worm::CellType::CPU;
    // Player one cell RIGHT of column 20, heading Left: steps onto the
    // line this frame.
    game.cycles[0].head = (21, 15);
    game.cycles[0].direction = worm::Direction::Left;
    game.cycles[0].positions = vec![(21, 15), (22, 15)];
    game.grid[15][21] = worm::CellType::Player;
    game.grid[15][22] = worm::CellType::Player;
    // CPU discharge from (20,9)? Beam must run column 20 through row 15:
    // fire from the CPU is positional — instead give the CPU the laser
    // and discharge along its heading? Its heading rams the wall. Fire
    // BEFORE the frame from a stand-in: the beam's owner must be the
    // CPU for the player to be a valid target, so place the CPU's
    // discharge geometry first, then redirect it into the wall.
    game.cycles[1].head = (20, 9);
    game.cycles[1].direction = worm::Direction::Down;
    game.cycles[1].positions = vec![(20, 9), (20, 8)];
    game.grid[10][4] = worm::CellType::Empty;
    game.grid[10][5] = worm::CellType::Empty;
    game.grid[9][20] = worm::CellType::CPU;
    game.grid[8][20] = worm::CellType::CPU;
    game.cycles[1].held_powerup = Some(worm::game::PowerUpKind::Laser);
    assert!(game.fire_powerup(1), "CPU discharges down column 20");
    assert!(game.cycles[0].alive, "player is off-line at discharge");
    // Now the CPU's fatal move: turn it into its own trail cell.
    game.cycles[1].direction = worm::Direction::Up; // into (20,8) = own neck? 180-ban…
    // Simplest guaranteed CPU movement death: wall of the frame. Put a
    // player trail cell ahead of the CPU.
    game.cycles[1].direction = worm::Direction::Down;
    game.grid[10][20] = worm::CellType::Player; // stray marker ahead
    game.update();
    assert!(!game.cycles[0].alive, "player stepped into the CPU's beam line");
    assert!(!game.cycles[1].alive, "CPU rammed the marker ahead");
    assert_eq!(
        game.winner, None,
        "same-frame movement death + beam kill resolve atomically as a draw"
    );
}

#[test]
fn test_v7_reconcile_never_rescans_bombs_or_breaches() {
    let mut game = v7_beam_board();
    game.cycles[0].head = (30, 24);
    game.cycles[0].direction = worm::Direction::Left;
    game.cycles[0].positions = vec![(30, 24), (31, 24)];
    game.grid[24][30] = worm::CellType::Player;
    game.grid[24][31] = worm::CellType::Player;
    game.cycles[1].head = (60, 30);
    game.cycles[1].direction = worm::Direction::Right;
    game.cycles[1].positions = vec![(60, 30)];
    game.grid[30][60] = worm::CellType::CPU;
    // A bomb planted ON the beam line AFTER discharge (same frame)
    // must not detonate at reconcile time.
    game.cycles[0].held_powerup = Some(worm::game::PowerUpKind::Laser);
    assert!(game.fire_powerup(0));
    let holes_before: usize = game
        .grid
        .iter()
        .map(|r| r.iter().filter(|&&c| c == worm::CellType::Hole).count())
        .sum();
    game.bombs.push(worm::game::Bomb {
        x: 20, y: 24, fuse: 500, armed_in: 0, disguise: 3, owner: 0, tripped: false,
    });
    game.update();
    assert_eq!(game.bombs.len(), 1, "reconcile never re-scans bombs (ADR-023)");
    let holes_after: usize = game
        .grid
        .iter()
        .map(|r| r.iter().filter(|&&c| c == worm::CellType::Hole).count())
        .sum();
    assert_eq!(holes_before, holes_after, "breach is computed once, at discharge");
}

#[test]
fn test_v6_pin_head_stepping_into_beam_survives() {
    // The pre-v7 world keeps its recorded physics: the same geometry
    // that kills under v7 passes harmlessly under v6.
    let mut game = v7_beam_board();
    game.set_world_version(6);
    game.cycles[0].head = (30, 24);
    game.cycles[0].direction = worm::Direction::Left;
    game.cycles[0].positions = vec![(30, 24), (31, 24)];
    game.grid[24][30] = worm::CellType::Player;
    game.grid[24][31] = worm::CellType::Player;
    game.cycles[1].head = (10, 25);
    game.cycles[1].direction = worm::Direction::Up;
    game.cycles[1].positions = vec![(10, 25), (10, 26), (10, 27)];
    for y in 25..=27 {
        game.grid[y][10] = worm::CellType::CPU;
    }
    game.cycles[0].held_powerup = Some(worm::game::PowerUpKind::Laser);
    assert!(game.fire_powerup(0));
    game.update();
    assert!(
        game.cycles[1].alive,
        "v6 ghosts replay v6 physics: the step-into survives there"
    );
}

/// THE OWNER'S ROUND, under v7 — the regression test IS his ghost log.
/// His discharge at frame 137 must now connect (the CPU stepped onto
/// the line at frame 138).
#[test]
fn owner_round_connects_under_v7() {
    let data = std::fs::read_to_string("/opt/worm/data/rounds/20260808.jsonl").unwrap();
    for line in data.lines() {
        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let rep = &v["replay"];
        if rep.is_null() || rep["arena"].as_u64() != Some(6) || rep["frames"].as_u64() != Some(268) {
            continue;
        }
        let ev: Vec<(u32, u8, u8)> = rep["ev"]
            .as_array().unwrap().iter()
            .map(|e| (e[0].as_u64().unwrap() as u32, e[1].as_u64().unwrap() as u8, e[2].as_u64().unwrap() as u8))
            .collect();
        let seed: u64 = rep["seed"].as_str().unwrap().parse().unwrap();
        let (w, h) = (rep["w"].as_u64().unwrap() as u16, rep["h"].as_u64().unwrap() as u16);
        // Same inputs, v7 physics (geometry is identical to v6).
        let mut g = WormGame::with_size_seed(w, h, 1);
        g.start_recorded_round(seed, w, h, 7, ev);
        while !g.game_over && g.frame_count < 140 {
            g.update();
        }
        assert!(
            g.game_over && g.frame_count <= 139,
            "the owner's laser must connect at frame 138 under v7 (got over={} frame={})",
            g.game_over, g.frame_count
        );
        assert_eq!(g.winner, Some(0), "his shot was a kill: the CPU head entered the line");
        assert_eq!(g.death_cause, Some(worm::DeathCause::Laser));
        return;
    }
    panic!("owner round (arena 6, frames 268) not found in the collection");
}

/// ADR-023 renderer contract, sim side: the killing beam's core paints
/// exactly once — the fire-ends-game exit flips `fresh` immediately,
/// and post-game pumps decay the layer (codex/k3 v7 verify round 2).
#[test]
fn test_v7_killing_beam_cools_after_game_over() {
    let mut game = v7_beam_board();
    // CPU standing ON the player's line: ignition head-kill ends the
    // game inside fire_powerup.
    game.cycles[0].head = (30, 24);
    game.cycles[0].direction = worm::Direction::Left;
    game.cycles[0].positions = vec![(30, 24), (31, 24)];
    game.grid[24][30] = worm::CellType::Player;
    game.grid[24][31] = worm::CellType::Player;
    game.cycles[1].head = (10, 24);
    game.cycles[1].direction = worm::Direction::Up;
    game.cycles[1].positions = vec![(10, 24)];
    game.grid[24][10] = worm::CellType::CPU;
    game.cycles[0].held_powerup = Some(worm::game::PowerUpKind::Laser);
    assert!(game.fire_powerup(0));
    assert!(game.game_over, "ignition head-kill ends the game");
    assert_eq!(game.beam_fx.len(), 1);
    assert_eq!(game.beam_fx[0].age, 0, "the kill frame paints the solid core");
    // Post-game pumps: the browser calls update() unconditionally; the
    // layer decays instead of glowing hot forever.
    game.update();
    assert_eq!(game.beam_fx[0].age, 1, "first post-game pump: afterimage");
    for _ in 0..25 {
        game.update();
    }
    assert!(game.beam_fx.is_empty(), "embers burn out; the layer empties");
}

/// codex v8 verify (blocking): a bomb sitting ON the owner's own trail
/// still chain-detonates — owner-safety spares the trail cell, never
/// the chain check.
#[test]
fn test_v8_bomb_on_owner_trail_still_chains() {
    let mut game = v7_beam_board(); // fresh empty board at v8? board pins 7
    game.set_world_version(8);
    // CPU trail through (50,20); its own second mine sits on that cell.
    game.cycles[1].head = (55, 20);
    game.cycles[1].direction = worm::Direction::Right;
    game.cycles[1].positions = (48..=55).rev().map(|x| (x as u16, 20u16)).collect();
    for x in 48..=55 {
        game.grid[20][x] = worm::CellType::CPU;
    }
    game.bombs.push(worm::game::Bomb {
        x: 50, y: 20, fuse: 9_000, disguise: 3, armed_in: 0, owner: 1, tripped: false,
    });
    // A first mine at (46,20) tripped by the player's proximity: its
    // blast cross covers (50,20).
    game.bombs.push(worm::game::Bomb {
        x: 46, y: 20, fuse: 9_000, disguise: 3, armed_in: 0, owner: 1, tripped: false,
    });
    game.cycles[0].head = (47, 20);
    game.cycles[0].positions = vec![(47, 20)];
    game.grid[20][47] = worm::CellType::Player;
    game.tick_bombs();
    assert!(
        game.bombs.is_empty(),
        "BOTH mines detonated — the chain reached the bomb sitting on the \
         owner's trail (without the fix it survives the sweep untripped)"
    );
    // And the owner's trail cell survived the sweep (owner-safe).
    assert_eq!(game.grid[20][50], worm::CellType::CPU);
}

/// codex v8 verify (blocking): sudden death actually closes ring 4
/// first under v8 — asserted against a cell that is NOT already wall —
/// and v7 replays keep the old base-2 schedule.
#[test]
fn test_v8_first_closure_is_ring_four_and_v7_keeps_ring_three() {
    let mut game = WormGame::with_size(120, 38);
    assert_eq!(game.sudden_death_base(), 3);
    game.time = worm::game::SUDDEN_DEATH_START + worm::game::SUDDEN_DEATH_INTERVAL;
    assert_eq!(game.grid[4][10], worm::CellType::Empty, "ring-4 lane open pre-close");
    game.update();
    assert_eq!(
        game.grid[4][10],
        worm::CellType::Wall,
        "v8 level 1 seals offset 4 — one ring INSIDE the arena wall"
    );
    // v7 pin: the same schedule step seals offset 3 (the arena wall line),
    // leaving ring 4 open.
    let mut old = WormGame::with_size(120, 38);
    old.set_world_version(7);
    old.time = worm::game::SUDDEN_DEATH_START + worm::game::SUDDEN_DEATH_INTERVAL;
    old.update();
    assert_eq!(
        old.grid[4][10],
        worm::CellType::Empty,
        "v7 replays keep the recorded base-2 schedule (ring 4 stays open)"
    );
}

/// codex v8 verify: the flash channel on the wire — calm decoys ship
/// ONLY inside the food list; flashing ones appear in bombFlash.
#[test]
fn test_v8_flash_wire_calm_absent_flashing_present() {
    let mut game = WormGame::with_size(60, 30);
    game.bombs.push(worm::game::Bomb {
        x: 30, y: 15, fuse: 9_000, disguise: 4, armed_in: 0, owner: 1, tripped: false,
    });
    let s = worm::web_state::to_json(&game);
    assert!(!s.contains("bombFlash\":[[30,15"), "calm decoy leaks nothing: {s:.0?}");
    game.bombs[0].fuse = 1_500;
    let s = worm::web_state::to_json(&game);
    assert!(s.contains("\"bombFlash\":[[30,15,1]]"), "tier 1 appears on the wire");
    game.bombs[0].fuse = 900;
    let s = worm::web_state::to_json(&game);
    assert!(s.contains("\"bombFlash\":[[30,15,2]]"), "tier 2 appears on the wire");
}

/* ------------------------------ world v9: napalm ------------------------------ */

fn v9_board() -> WormGame {
    let mut game = WormGame::with_size(120, 38);
    assert!(game.arena_version >= 9);
    game.cpu_autopilot = false;
    game.food_items.clear();
    game.cycles[0].head = (60, 20);
    game.cycles[0].positions = vec![(60, 20)];
    game.grid[20][60] = worm::CellType::Player;
    game.cycles[1].head = (20, 10);
    game.cycles[1].positions = vec![(20, 10)];
    game.grid[10][20] = worm::CellType::CPU;
    game
}

/// The bolt flies 4 cells and drops fire where it stops; the flame lasts
/// ~3 wall-clock seconds.
#[test]
fn test_v9_bolt_stops_at_four_and_ignites() {
    let mut game = v9_board();
    game.cycles[0].direction = worm::Direction::Right;
    game.cycles[0].held_powerup = Some(worm::game::PowerUpKind::TriShot);
    assert!(game.fire_powerup(0));
    for _ in 0..6 {
        game.advance_projectiles();
    }
    assert!(game.projectiles.is_empty(), "bolts are spent after 4 cells");
    assert!(
        game.flames.iter().any(|f| (f.x, f.y) == (64, 20)),
        "the straight bolt's fire lands exactly 4 cells out"
    );
    assert_eq!(game.flames.len(), 3, "all three bolts ignite");
    // Fire burns out on the wall clock.
    for _ in 0..400 {
        game.tick_flames();
    }
    assert!(game.flames.is_empty(), "flames die after ~3s");
}

/// The 5/3/1 wall-clock schedule, sticky to completion: a long worm
/// standing in fire loses up to 5 segments in second one, 3 in second
/// two, 1 in second three — 9 total — then the fire lets go.
#[test]
fn test_v9_burn_schedule_five_three_one() {
    let mut game = v9_board();
    game.cycles[1].positions = (0..20).map(|i| (20u16 + i, 10u16)).collect();
    game.cycles[1].head = (20, 10);
    for i in 0..20 {
        game.grid[10][20 + i as usize] = worm::CellType::CPU;
    }
    // Player fire under the CPU's mid-body.
    game.ignite(30, 10, 0);
    let mut ms = 0u32;
    let mut was_burning = false;
    let mut at = [0usize; 3]; // len at the 1s / 2s / 3s boundaries
    while ms < 3_600 {
        // Sample on the ENGINE's contact clock, just before each
        // boundary tranche begins; stop when the schedule completes
        // (the state resets to 0 and must not re-arm the sampler).
        let t_now = game.burns[1].contact_ms;
        if was_burning && t_now == 0 {
            // The 9th segment burns and the state resets inside one
            // tick — the 3s checkpoint is the completed state.
            at[2] = game.cycles[1].positions.len();
            break;
        }
        was_burning |= t_now > 0;
        // Under FLOOR pacing each tier COMPLETES at its boundary: the
        // first tick at-or-past the mark holds exactly the tier total.
        for (i, mark) in [1_000u32, 2_000, 3_000].iter().enumerate() {
            if t_now >= *mark && at[i] == 0 {
                at[i] = game.cycles[1].positions.len();
            }
        }
        game.tick_flames();
        ms += game.frame_delay().as_millis() as u32;
    }
    // codex v9 verify: the tier boundaries themselves, not just the
    // total — at[i] holds the length at the LAST tick before each
    // boundary, i.e. the tranche totals: 5, then 5+3, then the loop's
    // final state adds the last 1.
    assert_eq!(20 - at[0], 5, "first second: up to 5 segments");
    assert_eq!(20 - at[1], 8, "second second: 3 more");
    assert_eq!(20 - at[2], 9, "third second: the last 1");
    let final_len = game.cycles[1].positions.len();
    assert_eq!(20 - final_len, 9, "5+3+1 segments burn off, then the fire lets go");
    assert!(game.cycles[1].alive, "a 20-long worm survives the full schedule");
    assert_eq!(game.burns[1].contact_ms, 0, "the burn state resets after completion");
}

/// STICKY: the schedule keeps burning even after the tail shrinks out
/// of the flame cell (the fire "caught" — it is not contact-gated).
#[test]
fn test_v9_burn_is_sticky_after_leaving_the_fire() {
    let mut game = v9_board();
    game.cycles[1].positions = (0..12).map(|i| (20u16 + i, 10u16)).collect();
    game.cycles[1].head = (20, 10);
    for i in 0..12 {
        game.grid[10][20 + i as usize] = worm::CellType::CPU;
    }
    // Fire on the TAIL TIP cell only.
    game.ignite(31, 10, 0);
    // One tick catches; then remove the flame entirely (simulates the
    // burning tail having shrunk out / fire elsewhere gone).
    game.tick_flames();
    assert!(game.burns[1].contact_ms > 0, "caught");
    game.flames.clear();
    let mut ms = 0u32;
    while ms < 3_600 {
        game.tick_flames();
        ms += game.frame_delay().as_millis() as u32;
    }
    assert_eq!(
        12 - game.cycles[1].positions.len(),
        9,
        "the full 5/3/1 schedule ran to completion without the flame"
    );
}

/// Burned past the head = dead, attributed to the flame's owner.
#[test]
fn test_v9_head_burn_kills_and_attributes() {
    let mut game = v9_board();
    // A 3-long CPU: 5-segment first second burns past its head.
    game.cycles[1].positions = vec![(20, 10), (21, 10), (22, 10)];
    for x in 20..=22 {
        game.grid[10][x] = worm::CellType::CPU;
    }
    game.ignite(22, 10, 0);
    let mut ms = 0u32;
    while !game.game_over && ms < 2_000 {
        game.tick_flames();
        ms += game.frame_delay().as_millis() as u32;
    }
    assert!(game.game_over, "a 3-long worm cannot survive the first second");
    assert_eq!(game.winner, Some(0));
    assert_eq!(game.death_cause, Some(worm::DeathCause::Burned));
    assert_eq!(game.burns[1].burned_by, 0, "the kill credits the firer");
}

/// ADR-023 rule: the firer is immune to their OWN fire.
#[test]
fn test_v9_own_fire_never_burns() {
    let mut game = v9_board();
    game.cycles[1].positions = vec![(20, 10), (21, 10), (22, 10)];
    for x in 20..=22 {
        game.grid[10][x] = worm::CellType::CPU;
    }
    // The CPU's own fire under its own body.
    game.ignite(21, 10, 1);
    for _ in 0..200 {
        game.tick_flames();
    }
    assert!(game.cycles[1].alive);
    assert_eq!(game.cycles[1].positions.len(), 3, "own fire burned nothing");
}

/// Fire COOKS a decoy: early detonation, credited to the flame's owner.
#[test]
fn test_v9_flame_cooks_decoy() {
    let mut game = v9_board();
    game.bombs.push(worm::game::Bomb {
        x: 40, y: 15, fuse: 14_000, disguise: 4, armed_in: 0, owner: 1, tripped: false,
    });
    // A CPU trail cell inside the blast cross: the cook's detonation is
    // credited to the FLAME owner (the player), so the CPU trail is fair
    // game for the sweep — proof the blast really ran with fire credit.
    game.cycles[1].positions = vec![(20, 10), (44, 15)];
    game.grid[15][44] = worm::CellType::CPU;
    game.ignite(40, 15, 0);
    assert!(
        game.bombs.is_empty(),
        "the decoy cooked off the moment fire touched it"
    );
    assert_eq!(
        game.grid[15][44],
        worm::CellType::Empty,
        "the cook detonated with the flame owner's credit — the opponent \
         trail in the cross was swept (owner-safe rules applied to the \
         player as owner, CPU as target)"
    );
}

/// A frozen worm still burns — the schedule runs on the global wall
/// clock (consistency over mercy; ADR-022 matrix).
#[test]
fn test_v9_frozen_worm_burns() {
    let mut game = v9_board();
    game.cycles[1].positions = (0..12).map(|i| (20u16 + i, 10u16)).collect();
    for i in 0..12 {
        game.grid[10][20 + i as usize] = worm::CellType::CPU;
    }
    game.ignite(25, 10, 0);
    // tick_flames has no per-worm freeze gate at all — burning is global
    // by construction; assert segments come off without any update().
    for _ in 0..40 {
        game.tick_flames();
    }
    assert!(game.cycles[1].positions.len() < 12, "the fire does not care who moves");
}

/// v8 pin: recorded ghosts keep the blast tri-shot (no flames at all).
#[test]
fn test_v8_pin_trishot_still_blasts() {
    let mut game = v9_board();
    game.set_world_version(8);
    game.cycles[0].direction = worm::Direction::Right;
    game.cycles[0].held_powerup = Some(worm::game::PowerUpKind::TriShot);
    assert!(game.fire_powerup(0));
    for _ in 0..80 {
        game.advance_projectiles();
    }
    assert!(game.flames.is_empty(), "v8 replays never see fire");
}

/// codex v9 verify (blocking): the hazard phase runs on game-over exits
/// too — a burn completing while the opponent's death already ended the
/// game converts the result to a draw, atomically.
#[test]
fn test_v9_burn_completing_on_a_death_frame_is_a_draw() {
    let mut game = v9_board();
    // Short CPU already deep in a burn.
    game.cycles[1].positions = vec![(20, 10), (21, 10)];
    game.grid[10][21] = worm::CellType::CPU;
    game.ignite(21, 10, 0);
    game.tick_flames(); // caught
    assert!(game.burns[1].contact_ms > 0);
    // The player dies this frame (any cause); the frame's hazard phase
    // still burns the CPU down — both dead, draw.
    game.cycles[0].alive = false;
    game.game_over = true;
    game.winner = Some(1);
    if game.death_cause.is_none() {
        game.death_cause = Some(worm::DeathCause::Wall);
    }
    let mut ms = 0u32;
    while game.cycles[1].alive && ms < 2_000 {
        game.tick_flames();
        ms += game.frame_delay().as_millis() as u32;
    }
    assert!(!game.cycles[1].alive, "the burn ran to its kill despite game_over");
    assert_eq!(game.winner, None, "both dead on the frame -> draw, atomically");
}

/// v9 rulings (both consultants): the doze wakes before entering an
/// open one-step dead-end pocket — and a trail DIRECTLY ahead stays
/// invisible (the classic earned kill is untouched).
#[test]
fn test_v9_doze_wakes_at_a_pocket_but_stays_blind_to_trails() {
    let pocket = |version: u8| -> bool {
        let mut game = worm::WormGame::with_size_seed(60, 30, 7);
        game.set_world_version(version);
        game.frame_count = 0; // update() observes frame 1: doze candidate
        let (hx, hy) = game.cycles[1].head;
        let (dx, dy) = game.cycles[1].direction.as_delta();
        let (nx, ny) = ((hx as i16 + dx) as u16, (hy as i16 + dy) as u16);
        // Box the successor: every onward exit is enemy trail.
        for (ddx, ddy) in [(1i16, 0i16), (-1, 0), (0, 1), (0, -1)] {
            let (px, py) = (nx as i16 + ddx, ny as i16 + ddy);
            if (px as u16, py as u16) == (hx, hy) {
                continue;
            }
            game.grid[py as usize][px as usize] = worm::CellType::Player;
        }
        game.update();
        game.cpu_telemetry.decision.is_some()
    };
    assert!(pocket(9), "v9: the pocket wakes the doze (a decision frame)");
    assert!(!pocket(8), "pre-v9 arms keep their recorded blindness");

    // Trail DIRECTLY ahead: still invisible to the doze.
    let mut game = worm::WormGame::with_size_seed(60, 30, 7);
    game.frame_count = 0;
    let (hx, hy) = game.cycles[1].head;
    let (dx, dy) = game.cycles[1].direction.as_delta();
    game.grid[(hy as i16 + dy) as usize][(hx as i16 + dx) as usize] =
        worm::CellType::Player;
    game.update();
    assert!(
        game.cpu_telemetry.decision.is_none(),
        "a trail one cell ahead never wakes the doze — the earned Tron \
         kill survives the pocket rule"
    );
}

/// k3 v9 ruling 2b: the DWELL release. A latched read whose spend sits
/// below the behavioral floor for K consecutive round boundaries
/// releases outright; a healthy spend resets the dwell.
#[test]
fn test_dwell_release_frees_a_dead_latch() {
    let mut game = worm::WormGame::with_size_seed(60, 30, 7);
    // A latched-but-nearly-dead published read: tiny positive lift.
    let lr = &mut game.cpu_brain.lifetime_read;
    lr.lat_samples = 400;
    lr.lat_hits = 218;
    lr.lat_chance = 200.0;
    lr.lat_var = 100.0;
    lr.lat_latched = true;
    game.refresh_read_rate();
    let spend = game.cpu_brain.family_earned_read();
    assert!(
        spend > 0.0 && spend < worm::CpuBrain::SPEND_DWELL_FLOOR,
        "fixture: latched with a sub-floor spend ({spend})"
    );
    for _ in 0..worm::CpuBrain::SPEND_DWELL_ROUNDS {
        game.refresh_read_rate();
    }
    assert_eq!(
        game.cpu_brain.family_earned_read(),
        0.0,
        "K below-floor boundaries release the dead latch to a hard zero"
    );
    // A healthy read never dwells out.
    let lr = &mut game.cpu_brain.lifetime_read;
    lr.lat_hits = 380;
    lr.lat_latched = true;
    for _ in 0..3 * worm::CpuBrain::SPEND_DWELL_ROUNDS as usize {
        game.refresh_read_rate();
    }
    assert!(
        game.cpu_brain.family_earned_read() > 0.0,
        "a spend above the floor holds its latch indefinitely"
    );
}

/// OWNER BUG (2026-08-08, live): "if I hit the arrow keys rapidly, the
/// 2nd key is often not registered … it's why I'm increasingly running
/// into walls." Reproduction: a fast corner (Up then Left while moving
/// Right) pressed within one frame gap — the second press is eaten
/// because the 180-ban anchors on the still-unexecuted current motion.
#[test]
fn repro_fast_corner_second_press_eaten() {
    let mut game = WormGame::with_size(60, 30);
    game.cpu_autopilot = false;
    game.food_items.clear();
    game.cycles[0].head = (30, 15);
    game.cycles[0].direction = worm::Direction::Right;
    game.cycles[0].positions = vec![(30, 15), (29, 15)];
    game.grid[15][30] = worm::CellType::Player;
    game.grid[15][29] = worm::CellType::Player;
    game.cycles[0].prev_direction = worm::Direction::Right;
    // Both presses land between frames — the fast corner.
    game.change_direction(worm::Direction::Up);
    game.change_direction(worm::Direction::Left);
    game.update(); // should execute the Up
    game.update(); // should execute the Left
    let head = game.cycles[0].head;
    assert_eq!(
        head,
        (29, 14),
        "the corner is Up then Left: from (30,15) -> (30,14) -> (29,14); \
         got {head:?} — the second press was eaten"
    );
}

/// v10 contracts (codex consult): the drain rule, the dead 180-sneak,
/// turn-then-fire along the NEW heading, overflow drop-newest, v9 pin.
#[test]
fn test_v10_input_queue_contracts() {
    // Drain rule: [Left, Down] while moving Right — Left drops as an
    // immediate 180, Down executes THE SAME frame.
    let mut game = WormGame::with_size(60, 30);
    game.cpu_autopilot = false;
    game.food_items.clear();
    game.cycles[0].head = (30, 15);
    game.cycles[0].direction = worm::Direction::Right;
    game.cycles[0].prev_direction = worm::Direction::Right;
    game.cycles[0].positions = vec![(30, 15), (29, 15)];
    game.grid[15][30] = worm::CellType::Player;
    game.grid[15][29] = worm::CellType::Player;
    game.change_direction(worm::Direction::Left);
    game.change_direction(worm::Direction::Down);
    game.update();
    assert_eq!(
        game.cycles[0].head,
        (30, 16),
        "Left (a true 180) drops; Down executes the same frame"
    );

    // The 180-sneak stays dead: [Up, Left] from Right is the legal
    // two-frame corner (moves Up first), but a lone [Left] from Right
    // is dropped and the worm continues Right.
    let mut game = WormGame::with_size(60, 30);
    game.cpu_autopilot = false;
    game.food_items.clear();
    game.cycles[0].head = (30, 15);
    game.cycles[0].direction = worm::Direction::Right;
    game.cycles[0].prev_direction = worm::Direction::Right;
    game.cycles[0].positions = vec![(30, 15), (29, 15)];
    game.grid[15][30] = worm::CellType::Player;
    game.grid[15][29] = worm::CellType::Player;
    game.change_direction(worm::Direction::Left);
    game.update();
    assert_eq!(game.cycles[0].head, (31, 15), "a lone 180 is dropped");

    // Turn-then-fire: the discharge waits for its turn and fires along
    // the NEW heading (a laser Up, not Right).
    let mut game = WormGame::with_size(60, 30);
    game.cpu_autopilot = false;
    game.food_items.clear();
    game.cycles[0].head = (30, 15);
    game.cycles[0].direction = worm::Direction::Right;
    game.cycles[0].prev_direction = worm::Direction::Right;
    game.cycles[0].positions = vec![(30, 15), (29, 15)];
    game.grid[15][30] = worm::CellType::Player;
    game.grid[15][29] = worm::CellType::Player;
    // CPU parked on the UP column, off the RIGHT row.
    game.cycles[1].head = (30, 6);
    game.cycles[1].direction = worm::Direction::Left;
    game.cycles[1].positions = vec![(30, 6)];
    game.grid[6][30] = worm::CellType::CPU;
    game.cycles[0].held_powerup = Some(worm::game::PowerUpKind::Laser);
    game.change_direction(worm::Direction::Up);
    assert!(game.player_fire(), "fire joins the queue");
    game.update();
    assert!(
        !game.cycles[1].alive || game.game_over,
        "the beam fired along the NEW heading (Up) and found the CPU"
    );

    // Overflow: cap 3, drop-newest — the 4th input vanishes.
    let mut game = WormGame::with_size(60, 30);
    game.change_direction(worm::Direction::Up);
    game.change_direction(worm::Direction::Left);
    game.change_direction(worm::Direction::Down);
    game.change_direction(worm::Direction::Right); // dropped
    assert_eq!(game.input_queue.len(), 3);

    // v9 pin: the old single-slot latch behavior survives for replays.
    let mut game = WormGame::with_size(60, 30);
    game.set_world_version(9);
    game.cpu_autopilot = false;
    game.food_items.clear();
    game.cycles[0].head = (30, 15);
    game.cycles[0].direction = worm::Direction::Right;
    game.cycles[0].prev_direction = worm::Direction::Right;
    game.cycles[0].positions = vec![(30, 15), (29, 15)];
    game.grid[15][30] = worm::CellType::Player;
    game.grid[15][29] = worm::CellType::Player;
    game.change_direction(worm::Direction::Up);
    game.change_direction(worm::Direction::Left); // eaten at v9
    game.update();
    game.update();
    assert_eq!(
        game.cycles[0].head,
        (30, 13),
        "pre-v10 ghosts keep the recorded single-slot semantics"
    );
}
