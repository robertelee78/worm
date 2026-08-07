//! ADR-021 surface #9's harness: the PLATEAU DETECTOR.
//!
//! SONA (or any learned challenger) earns evaluation only when the
//! counting stack measurably plateaus. This makes that gate a number:
//! per-era prequential log-loss of the production hazard and per-era
//! side accuracy of the turn book, over a player corpus. "Plateau" =
//! consecutive-era improvement below 1% relative, twice running.
//!
//!     cargo run --release --example sona_probe -- data/players/<id>.json
use worm::WormGame;

fn main() {
    let path = std::env::args().nth(1).expect("usage: sona_probe <player.json>");
    let text = std::fs::read_to_string(&path).expect("read");
    let v: serde_json::Value = serde_json::from_str(&text).expect("json");
    let mut rounds: Vec<&serde_json::Value> =
        v["rounds"].as_array().expect("rounds").iter().collect();
    rounds.sort_by_key(|r| r["endedAt"].as_u64().unwrap_or(0));

    let mut game = WormGame::with_size_seed(55, 40, 1);
    game.cpu_autopilot = false;
    game.shadow_learning = true;

    const ERA: usize = 15;
    let mut era_ll = Vec::new(); // (hazard log-loss sum, frames)
    let mut era_side = Vec::new(); // (side hits, side events)
    let mut cur_ll = 0f64;
    let mut cur_n = 0u64;
    let mut side_before = (0u32, 0u32);

    for (idx, rec) in rounds.iter().enumerate() {
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
        game.start_recorded_round(seed, w, h, arena, events);
        let mut gap = 0u32;
        while !game.game_over && game.frame_count <= frames {
            let heading = game.cycles[0].prev_direction;
            let straight_legal =
                worm::legal_options_from(&game, 0, heading).contains(&heading);
            // Production hazard estimate BEFORE the frame lands.
            let (px, py) = game.cycles[0].head;
            let nearest = game
                .food_items
                .iter()
                .min_by_key(|&&(fx, fy, _)| {
                    (fx as i32 - px as i32).abs() + (fy as i32 - py as i32).abs()
                })
                .map(|&(fx, fy, _)| (fx, fy));
            let fside = worm::cpu_ai::food_side(px, py, heading, nearest);
            let cell = worm::cpu_ai::hazard_cell(
                game.cpu_brain.gap_since_voluntary,
                fside,
                game.cpu_brain.frames_since_food <= 3,
                false,
            );
            let hz = game.cpu_brain.class_books.hazard(cell).clamp(1e-4, 1.0 - 1e-4);
            game.update();
            let dir = game.cycles[0].direction;
            if let Some(t) = worm::cpu_ai::Turn::from_dirs(heading, dir) {
                let lateral = t != worm::cpu_ai::Turn::Straight;
                if straight_legal {
                    cur_ll -= if lateral {
                        (hz as f64).ln()
                    } else {
                        (1.0 - hz as f64).ln()
                    };
                    cur_n += 1;
                }
                if lateral && straight_legal {
                    gap = 0;
                } else {
                    gap = gap.saturating_add(1);
                }
            }
            let _ = gap;
        }
        if (idx + 1) % ERA == 0 {
            era_ll.push((cur_ll, cur_n));
            cur_ll = 0.0;
            cur_n = 0;
            // Non-decayed counters — the decayed aT tallies cannot be
            // era-differenced (codex: the first cut printed nonsense).
            let br = &game.cpu_brain.class_books.book_read;
            let hits = br.hits;
            let tot = br.samples;
            era_side.push((hits.saturating_sub(side_before.0), tot.saturating_sub(side_before.1)));
            side_before = (hits, tot);
        }
    }

    println!("== SONA ENTRY GATE: is the counting stack still improving? ==");
    println!(
        "  corpus: {} · {} rounds · {} eras of {} (scope: THIS corpus only)",
        path,
        rounds.len(),
        era_ll.len(),
        ERA
    );
    let mut prev: Option<f64> = None;
    let mut flat_streak = 0;
    let mut worst_recent = 0.0f64;
    let n_eras = era_ll.len();
    for (i, &(ll, n)) in era_ll.iter().enumerate() {
        let per = ll / n.max(1) as f64;
        let delta = prev.map(|p| (p - per) / p * 100.0);
        let side = era_side
            .get(i)
            .map(|&(h, t)| format!("  side-acc {:.2} ({} ev)", h as f64 / t.max(1) as f64, t))
            .unwrap_or_default();
        println!(
            "  era {:>2} ({} rounds): hazard log-loss {:.4}/frame{}{}",
            i + 1,
            ERA,
            per,
            delta
                .map(|d| format!("  ({:+.1}% vs prev era)", d))
                .unwrap_or_default(),
            side
        );
        if let Some(d) = delta {
            // FLAT means WITHIN the band — deterioration is not flatness
            // (codex verification: 'large deterioration then +0.5%' must
            // not open the gate).
            if d.abs() < 1.0 {
                flat_streak += 1;
            } else {
                flat_streak = 0;
            }
            // Any recent era materially WORSE = the target moved.
            if i + 3 >= n_eras && d < -1.0 {
                worst_recent = worst_recent.min(d);
            }
        }
        prev = Some(per);
    }
    let drift_latched = game.cpu_brain.ledgers.drift_latched;
    let plateaued = flat_streak >= 2;
    // The gate opens ONLY on: two consecutive within-band eras, AND no
    // recent era materially worse (the target moved), AND the drift alarm
    // itself quiet — checked here directly, not left to the reader
    // (codex verification, blocking finding 5).
    let moving = worst_recent < -1.0 || drift_latched;
    println!(
        "\n  drift alarm latched: {drift_latched} · worst recent era delta: {worst_recent:+.1}%"
    );
    println!(
        "  VERDICT: {}",
        if moving {
            "TARGET MOVING (recent deterioration or drift latched) — not a \
             plateau; the challenger gate does NOT open on a moving target"
        } else if plateaued {
            "PLATEAU — a challenger (SONA or otherwise) has earned an offline evaluation"
        } else {
            "still improving — no challenger earns entry (this is the gate holding, not a failure)"
        }
    );
}
