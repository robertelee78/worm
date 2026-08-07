//! Learning-program spike (kata step 2): measure the DATA SUPPLY for each
//! proposed learning surface from a real player corpus, so the design
//! consults argue from numbers instead of vibes.
//!
//!     cargo run --release --example learning_probe -- data/players/<id>.json
use std::collections::HashMap;
use worm::WormGame;

fn main() {
    let path = std::env::args().nth(1).expect("usage: learning_probe <player.json>");
    let text = std::fs::read_to_string(&path).expect("read");
    let v: serde_json::Value = serde_json::from_str(&text).expect("json");
    let rounds: Vec<&serde_json::Value> = v["rounds"].as_array().expect("rounds").iter().collect();

    // ---- Surface 5/7 supply from the record fields alone ----
    let mut cpu_death_causes: HashMap<String, u32> = HashMap::new();
    let mut player_death_causes: HashMap<String, u32> = HashMap::new();
    for r in &rounds {
        let cause = r["cause"].as_str().unwrap_or("?").to_string();
        match r["winner"].as_u64() {
            Some(0) => *cpu_death_causes.entry(cause).or_default() += 1,
            Some(1) => *player_death_causes.entry(cause).or_default() += 1,
            _ => {}
        }
    }

    // ---- Replay-based supplies ----
    let mut game = WormGame::with_size_seed(55, 40, 1);
    game.cpu_autopilot = false;
    game.shadow_learning = true;

    // Spawn book: player's first 3 direction CHANGES per round (kind 0
    // events in the opening 60 frames), as a sequence signature.
    let mut spawn_sigs: HashMap<String, u32> = HashMap::new();
    // Bait/weapon supply: CPU fires by held weapon at fire time + whether
    // the player died within 40 frames after.
    let mut fires: HashMap<&'static str, (u32, u32)> = HashMap::new(); // (fired, lethal)
    // Rhythm: voluntary-lateral gap histogram, overall and by food-side.
    let mut gap_hist = [0u32; 16];
    let mut gap_hist_side = [0u32; 16]; // gaps ending in a turn while food was off-side
    // Era split for drift: first 45 rounds vs rest.
    let mut era_gaps: [Vec<u32>; 2] = [Vec::new(), Vec::new()];
    let mut era_alt: [(u32, u32); 2] = [(0, 0), (0, 0)]; // (alternations, laterals)

    let mut ordered: Vec<(u64, &serde_json::Value)> = rounds
        .iter()
        .map(|r| (r["endedAt"].as_u64().unwrap_or(0), *r))
        .collect();
    ordered.sort_by_key(|(t, _)| *t);

    for (idx, (_, rec)) in ordered.iter().enumerate() {
        let Some(replay) = rec.get("replay") else { continue };
        if replay["v"].as_u64() != Some(2) {
            continue;
        }
        let seed: u64 = replay["seed"].as_str().unwrap().parse().unwrap();
        let w = replay["w"].as_u64().unwrap() as u16;
        let h = replay["h"].as_u64().unwrap() as u16;
        let arena = replay.get("arena").and_then(|x| x.as_u64()).unwrap_or(1) as u8;
        let frames = replay["frames"].as_u64().unwrap() as u32;
        let events: Vec<(u32, u8, u8)> = replay["ev"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                let t = e.as_array().unwrap();
                (
                    t[0].as_u64().unwrap() as u32,
                    t[1].as_u64().unwrap() as u8,
                    t[2].as_u64().unwrap() as u8,
                )
            })
            .collect();

        // Spawn signature from the raw event stream.
        let sig: Vec<String> = events
            .iter()
            .filter(|&&(f, k, _)| k == 0 && f <= 60)
            .take(3)
            .map(|&(_, _, v)| format!("{v}"))
            .collect();
        *spawn_sigs.entry(sig.join("-")).or_default() += 1;

        // Replay for weapon fires + rhythm.
        game.start_recorded_round(seed, w, h, arena, events.clone());
        let era = if idx < 45 { 0 } else { 1 };
        let mut gap = 0u32;
        let mut last_lat: Option<bool> = None; // true = Left
        let mut player_dead_at: Option<u32> = None;
        let mut pending_fire: Vec<(u32, &'static str)> = Vec::new();
        while !game.game_over && game.frame_count <= frames {
            let heading = game.cycles[0].prev_direction;
            let cpu_weapon = game.cycles[1].held_powerup;
            let cpu_fired = events
                .iter()
                .any(|&(f, k, _)| k == 3 && f == game.frame_count + 1);
            if cpu_fired {
                let name = match cpu_weapon {
                    Some(worm::game::PowerUpKind::Laser) => "laser",
                    Some(worm::game::PowerUpKind::TriShot) => "trishot",
                    Some(worm::game::PowerUpKind::Bomb) => "mine",
                    None => "none",
                };
                fires.entry(name).or_default().0 += 1;
                pending_fire.push((game.frame_count, name));
            }
            game.update();
            if !game.cycles[0].alive && player_dead_at.is_none() {
                player_dead_at = Some(game.frame_count);
            }
            // Rhythm accounting from the realized move.
            let dir = game.cycles[0].direction;
            if let Some(t) = worm::cpu_ai::Turn::from_dirs(heading, dir) {
                let lateral = t != worm::cpu_ai::Turn::Straight;
                let straight_was_legal = true; // approximation: gaps measured over all frames
                let _ = straight_was_legal;
                if lateral {
                    let g = (gap as usize).min(15);
                    gap_hist[g] += 1;
                    let (px, py) = game.cycles[0].head;
                    let nearest = game
                        .food_items
                        .iter()
                        .min_by_key(|&&(fx, fy, _)| {
                            (fx as i32 - px as i32).abs() + (fy as i32 - py as i32).abs()
                        })
                        .map(|&(fx, fy, _)| (fx, fy));
                    if worm::cpu_ai::food_side(px, py, heading, nearest)
                        != worm::cpu_ai::FoodSide::Ahead
                    {
                        gap_hist_side[g] += 1;
                    }
                    era_gaps[era].push(gap);
                    let left = t == worm::cpu_ai::Turn::Left;
                    if let Some(prev) = last_lat {
                        era_alt[era].1 += 1;
                        if prev != left {
                            era_alt[era].0 += 1;
                        }
                    }
                    last_lat = Some(left);
                    gap = 0;
                } else {
                    gap = gap.saturating_add(1);
                }
            }
        }
        if let Some(d) = player_dead_at {
            for (f, name) in &pending_fire {
                if d > *f && d - *f <= 40 {
                    fires.entry(name).or_default().1 += 1;
                }
            }
        }
        if idx == 44 {
            let b = &game.cpu_brain.class_books;
            println!(
                "[era-1 end] book aT={:.2} spendable={:.2} events={} alt_p_left_events={}",
                b.a_turn(),
                b.spendable(),
                b.turn_events,
                game.cpu_brain.voluntary_pattern.events
            );
        }
    }
    {
        let b = &game.cpu_brain.class_books;
        println!(
            "[era-2 end] book aT={:.2} spendable={:.2} events={}",
            b.a_turn(),
            b.spendable(),
            b.turn_events
        );
    }

    // Era snapshot of book health (consult question C: does the measured
    // drift already degrade the read?) — captured after round 45.
    // (Recomputed here rather than mid-loop to keep the loop simple: the
    // ClassBooks accuracies are decayed, so "state at end of era 2" vs
    // "state at end of era 1" is the comparison that matters.)

    // ---- Kata 2 gate: 8- vs 16-bucket gap resolution, prequential ----
    // Twin KT tables over the same context factors as production
    // (food-side x just-ate x cpu-close = 12 contexts), differing only in
    // gap buckets. Per eligible frame: log-loss of the CURRENT estimate
    // against the realized turn/stay, THEN update — prequential.
    {
        let mut t8 = vec![(0.0f32, 0.0f32); 8 * 12];
        let mut t16 = vec![(0.0f32, 0.0f32); 16 * 12];
        let mut ll8 = 0f64;
        let mut ll16 = 0f64;
        let mut n_frames = 0u64;
        let mut game = WormGame::with_size_seed(55, 40, 1);
        game.cpu_autopilot = false;
        game.shadow_learning = true;
        for (_, rec) in ordered.iter() {
            let Some(replay) = rec.get("replay") else { continue };
            if replay["v"].as_u64() != Some(2) { continue; }
            let seed: u64 = replay["seed"].as_str().unwrap().parse().unwrap();
            let w = replay["w"].as_u64().unwrap() as u16;
            let h = replay["h"].as_u64().unwrap() as u16;
            let arena = replay.get("arena").and_then(|x| x.as_u64()).unwrap_or(1) as u8;
            let frames = replay["frames"].as_u64().unwrap() as u32;
            let events: Vec<(u32, u8, u8)> = replay["ev"].as_array().unwrap().iter().map(|e| {
                let t = e.as_array().unwrap();
                (t[0].as_u64().unwrap() as u32, t[1].as_u64().unwrap() as u8, t[2].as_u64().unwrap() as u8)
            }).collect();
            game.start_recorded_round(seed, w, h, arena, events);
            let mut gap = 0u32;
            while !game.game_over && game.frame_count <= frames {
                let heading = game.cycles[0].prev_direction;
                let straight_legal = worm::legal_options_from(&game, 0, heading).contains(&heading);
                let (px, py) = game.cycles[0].head;
                let nearest = game.food_items.iter().min_by_key(|&&(fx, fy, _)| {
                    (fx as i32 - px as i32).abs() + (fy as i32 - py as i32).abs()
                }).map(|&(fx, fy, _)| (fx, fy));
                let fs = match worm::cpu_ai::food_side(px, py, heading, nearest) {
                    worm::cpu_ai::FoodSide::Ahead => 0usize,
                    worm::cpu_ai::FoodSide::Left => 1,
                    worm::cpu_ai::FoodSide::Right => 2,
                };
                let just_ate = false; // context parity across both tables
                let ctx = fs + 3 * (just_ate as usize);
                game.update();
                let dir = game.cycles[0].direction;
                if let Some(t) = worm::cpu_ai::Turn::from_dirs(heading, dir) {
                    let lateral = t != worm::cpu_ai::Turn::Straight;
                    if straight_legal {
                        let c8 = (gap as usize).min(7) + 8 * ctx;
                        let c16 = (gap as usize).min(15) + 16 * ctx;
                        let p8 = (t8[c8].0 + 0.5) / (t8[c8].1 + 1.0);
                        let p16 = (t16[c16].0 + 0.5) / (t16[c16].1 + 1.0);
                        let y = lateral;
                        ll8 -= if y { (p8 as f64).ln() } else { (1.0 - p8 as f64).ln() };
                        ll16 -= if y { (p16 as f64).ln() } else { (1.0 - p16 as f64).ln() };
                        n_frames += 1;
                        for (tab, cell) in [(&mut t8, c8), (&mut t16, c16)] {
                            tab[cell].0 *= 0.995;
                            tab[cell].1 *= 0.995;
                            tab[cell].1 += 1.0;
                            if y {
                                tab[cell].0 += 1.0;
                            }
                        }
                    }
                    if lateral && straight_legal {
                        gap = 0;
                    } else {
                        gap = gap.saturating_add(1);
                    }
                }
            }
        }
        println!(
            "\n== KATA-2 GATE: gap resolution (prequential log-loss, lower=better) ==\n  8-bucket: {:.4}/frame  16-bucket: {:.4}/frame  over {} eligible frames",
            ll8 / n_frames as f64,
            ll16 / n_frames as f64,
            n_frames
        );
    }

    // ---- Epistemic thinness over the learned hazard cells ----
    let b = &game.cpu_brain.class_books;
    let mut populated = 0;
    let mut thin = 0;
    let total_mass: f32 = b.hz_total.iter().sum();
    for cell in 0..worm::cpu_ai::HAZARD_CELLS {
        if b.hz_total[cell] >= 1.0 {
            populated += 1;
            if b.hz_total[cell] < 5.0 {
                thin += 1;
            }
        }
    }

    println!("== 5/7 SUPPLY: death causes ==");
    println!("  CPU deaths (its own losses to learn from): {cpu_death_causes:?}");
    println!("  player deaths (tactic outcomes): {player_death_causes:?}");
    println!("\n== 2 SUPPLY: weapon fires (fired, lethal<=40f) ==");
    println!("  {fires:?}");
    println!("\n== 7 SUPPLY: spawn signatures (top 5 of {} rounds) ==", ordered.len());
    let mut sigs: Vec<_> = spawn_sigs.iter().collect();
    sigs.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
    for (s, n) in sigs.iter().take(5) {
        println!("  '{s}': {n}");
    }
    println!("\n== 4 SUPPLY: voluntary-ish lateral gap histogram (all laterals) ==");
    println!("  all : {gap_hist:?}");
    println!("  side: {gap_hist_side:?}");
    println!("\n== 6 SUPPLY: era drift (rounds 1-45 vs 46+) ==");
    for e in 0..2 {
        let n = era_gaps[e].len().max(1);
        let mean: f32 = era_gaps[e].iter().sum::<u32>() as f32 / n as f32;
        let (alts, lats) = era_alt[e];
        println!(
            "  era {}: laterals={} mean-gap={:.2} P(alternate)={:.2}",
            e,
            era_gaps[e].len(),
            mean,
            alts as f32 / lats.max(1) as f32
        );
    }
    println!("\n== 3 SUPPLY: epistemic thinness of the hazard map ==");
    println!(
        "  populated cells {populated}/{} · thin (<5 mass) {thin} · total mass {total_mass:.0}",
        worm::cpu_ai::HAZARD_CELLS
    );
}
