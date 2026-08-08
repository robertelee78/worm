//! THE BETWEEN-ROUND RATCHET (owner intent; task #13 sense-A, done right):
//! when the player's style drifts, name WHICH REGION of situation-space
//! moved — as a minimum cut over the context graph, computed offline or
//! at a round boundary where a few milliseconds are free.
//!
//! Graph: nodes = the 96 hazard context cells (both eras populated).
//! Edges: context-space neighbors (gap±1, same otherwise; and same gap
//! across one factor flip), weighted by CO-MOVEMENT: cells whose
//! era-over-era hazard deltas agree bind tightly; cells that moved
//! differently bind loosely. The exact min cut then separates "the part
//! of your game that changed" from "the part that held" — with the
//! partition NAMED in words.
//!
//! Exact Stoer–Wagner (no dependency): validated below on a planted
//! two-regime instance the heuristic-partition crate fails
//! (`cargo run --release --example drift_partition -- --selftest`),
//! then applied to a real corpus:
//!     cargo run --release --example drift_partition -- data/players/<id>.json
use worm::WormGame;

/// Exact global minimum cut (Stoer–Wagner) on a small dense-matrix graph.
/// O(n^3): at n = 96 that is ~1M ops — microseconds to milliseconds.
fn stoer_wagner(mut w: Vec<Vec<f64>>) -> (f64, Vec<usize>) {
    let n = w.len();
    let mut vertices: Vec<Vec<usize>> = (0..n).map(|i| vec![i]).collect();
    let mut best = (f64::INFINITY, Vec::new());
    let mut active: Vec<usize> = (0..n).collect();
    while active.len() > 1 {
        // Maximum adjacency (minimum cut phase).
        let mut weights = vec![0.0f64; n];
        let mut in_a = vec![false; n];
        let mut prev = active[0];
        let mut last = active[0];
        for _ in 0..active.len() {
            let mut sel = usize::MAX;
            for &v in &active {
                if !in_a[v] && (sel == usize::MAX || weights[v] > weights[sel]) {
                    sel = v;
                }
            }
            in_a[sel] = true;
            prev = last;
            last = sel;
            for &v in &active {
                if !in_a[v] {
                    weights[v] += w[sel][v];
                }
            }
        }
        // Cut-of-the-phase: `last` alone vs the rest.
        if weights[last] < best.0 {
            best = (weights[last], vertices[last].clone());
        }
        // Merge last into prev.
        let moved = std::mem::take(&mut vertices[last]);
        vertices[prev].extend(moved);
        for v in 0..n {
            w[prev][v] += w[last][v];
            w[v][prev] += w[v][last];
        }
        active.retain(|&v| v != last);
    }
    best
}

fn selftest() {
    // The planted instance the crate's heuristic partition failed:
    // two 48-node ring+chord clusters, one weight-1 bridge (10,58).
    let n = 96;
    let mut w = vec![vec![0.0; n]; n];
    let mut add = |a: usize, b: usize, wt: f64, w: &mut Vec<Vec<f64>>| {
        w[a][b] += wt;
        w[b][a] += wt;
    };
    for block in [0usize, 48] {
        for i in 0..48 {
            add(block + i, block + (i + 1) % 48, 5.0, &mut w);
            add(block + i, block + (i + 13) % 48, 5.0, &mut w);
        }
    }
    add(10, 58, 1.0, &mut w);
    let t = std::time::Instant::now();
    let (value, side) = stoer_wagner(w);
    println!(
        "selftest: cut={value} (truth 1) · side={} nodes (truth 48) · pure={} · {:?}",
        side.len(),
        side.iter().all(|&v| v >= 48) || side.iter().all(|&v| v < 48),
        t.elapsed()
    );
    assert_eq!(value, 1.0);
    assert_eq!(side.len(), 48);
}

fn main() {
    let arg = std::env::args().nth(1).expect("usage: drift_partition <player.json>|--selftest");
    if arg == "--selftest" {
        selftest();
        return;
    }

    // Replay the corpus in two halves; capture per-cell hazard deltas.
    let text = std::fs::read_to_string(&arg).expect("read");
    let v: serde_json::Value = serde_json::from_str(&text).expect("json");
    let mut rounds: Vec<&serde_json::Value> =
        v["rounds"].as_array().expect("rounds").iter().collect();
    rounds.sort_by_key(|r| r["endedAt"].as_u64().unwrap_or(0));
    let half = rounds.len() / 2;

    let mut game = WormGame::with_size_seed(55, 40, 1);
    game.cpu_autopilot = false;
    game.shadow_learning = true;
    // RAW per-era tallies (turned, eligible) per cell — the persisted
    // hazard counters DECAY, and differencing decayed counters produces
    // impossible proportions (the sona_probe side-series bug class; a
    // first cut of this probe printed a +102% hazard delta).
    const CELLS: usize = worm::cpu_ai::HAZARD_CELLS;
    let mut era_turn = [[0.0f64; CELLS]; 2];
    let mut era_total = [[0.0f64; CELLS]; 2];
    for (idx, rec) in rounds.iter().enumerate() {
        let Some(replay) = rec.get("replay") else { continue };
        if replay["v"].as_u64() != Some(2) {
            continue;
        }
        let seed: u64 = replay["seed"].as_str().unwrap().parse().unwrap();
        let w0 = replay["w"].as_u64().unwrap() as u16;
        let h0 = replay["h"].as_u64().unwrap() as u16;
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
        game.start_recorded_round(seed, w0, h0, arena, events);
        let era = (idx >= half) as usize;
        let mut prev_dist = 0u32;
        while !game.game_over && game.frame_count <= frames {
            let heading = game.cycles[0].prev_direction;
            let straight_legal =
                worm::legal_options_from(&game, 0, heading).contains(&heading);
            let (px, py) = game.cycles[0].head;
            let nearest = game
                .food_items
                .iter()
                .min_by_key(|&&(fx, fy, _)| {
                    (fx as i32 - px as i32).abs() + (fy as i32 - py as i32).abs()
                })
                .map(|&(fx, fy, _)| (fx, fy));
            let fside = worm::cpu_ai::food_side(px, py, heading, nearest);
            let (chx, chy) = game.cycles[1].head;
            let dist =
                ((px as i32 - chx as i32).abs() + (py as i32 - chy as i32).abs()) as u32;
            let cpu_close = dist <= 12 && dist < prev_dist.max(1);
            prev_dist = dist;
            let cell = worm::cpu_ai::hazard_cell(
                game.cpu_brain.gap_since_voluntary,
                fside,
                game.cpu_brain.frames_since_food <= 3,
                cpu_close,
            );
            game.update();
            let dir = game.cycles[0].direction;
            if straight_legal {
                if let Some(t) = worm::cpu_ai::Turn::from_dirs(heading, dir) {
                    era_total[era][cell] += 1.0;
                    if t != worm::cpu_ai::Turn::Straight {
                        era_turn[era][cell] += 1.0;
                    }
                }
            }
        }
    }
    game.finalize_round_ledgers();

    // Per-cell RAW KT deltas where both eras have mass.
    let mut delta = vec![f64::NAN; CELLS];
    for i in 0..CELLS {
        if era_total[0][i] >= 5.0 && era_total[1][i] >= 5.0 {
            let h1 = (era_turn[0][i] + 0.5) / (era_total[0][i] + 1.0);
            let h2 = (era_turn[1][i] + 0.5) / (era_total[1][i] + 1.0);
            delta[i] = h2 - h1;
        }
    }
    let live: Vec<usize> = (0..CELLS).filter(|&i| delta[i].is_finite()).collect();
    if live.len() < 8 {
        println!("not enough co-populated cells ({}) — no boundary to name", live.len());
        return;
    }

    // Co-movement graph over live cells: context-space neighbors bind by
    // delta agreement.
    let idx_of: std::collections::HashMap<usize, usize> =
        live.iter().enumerate().map(|(k, &c)| (c, k)).collect();
    let m = live.len();
    let mut w = vec![vec![0.0f64; m]; m];
    let neighbors = |cell: usize| -> Vec<usize> {
        let gap = cell % 8;
        let rest = cell / 8;
        let mut out = Vec::new();
        if gap > 0 {
            out.push(cell - 1);
        }
        if gap < 7 {
            out.push(cell + 1);
        }
        // One factor flip at same gap: side (3 values), ate, close.
        let side = rest % 3;
        let ate = (rest / 3) % 2;
        let close = (rest / 6) % 2;
        for s2 in 0..3 {
            if s2 != side {
                out.push(gap + 8 * (s2 + 3 * (ate + 2 * close)));
            }
        }
        out.push(gap + 8 * (side + 3 * ((1 - ate) + 2 * close)));
        out.push(gap + 8 * (side + 3 * (ate + 2 * (1 - close))));
        out
    };
    for &c in &live {
        for nb in neighbors(c) {
            if let (Some(&a), Some(&bidx)) = (idx_of.get(&c), idx_of.get(&nb)) {
                if a < bidx {
                    let agree = 1.0 / (0.05 + (delta[c] - delta[nb]).abs());
                    w[a][bidx] = agree;
                    w[bidx][a] = agree;
                }
            }
        }
    }

    let t = std::time::Instant::now();
    let (value, side_idx) = stoer_wagner(w);
    let side_cells: Vec<usize> = side_idx.iter().map(|&k| live[k]).collect();
    let name = |cell: usize| -> String {
        let gap = cell % 8;
        let rest = cell / 8;
        let side = ["food-ahead", "food-left", "food-right"][rest % 3];
        let ate = if (rest / 3) % 2 == 1 { "+just-ate" } else { "" };
        let close = if (rest / 6) % 2 == 1 { "+chased" } else { "" };
        format!("gap{gap} {side}{ate}{close}")
    };
    let mean = |cells: &[usize]| -> f64 {
        cells.iter().map(|&c| delta[c]).sum::<f64>() / cells.len().max(1) as f64
    };
    let other: Vec<usize> = live.iter().copied().filter(|c| !side_cells.contains(c)).collect();
    println!(
        "== DRIFT BOUNDARY (exact cut {value:.2}, {} live cells, {:?}) ==",
        live.len(),
        t.elapsed()
    );
    println!(
        "  the region that MOVED ({} cells, mean Δhazard {:+.2}):",
        side_cells.len().min(other.len()),
        if side_cells.len() <= other.len() { mean(&side_cells) } else { mean(&other) }
    );
    let (moved, held) = if side_cells.len() <= other.len() {
        (&side_cells, &other)
    } else {
        (&other, &side_cells)
    };
    for &c in moved.iter().take(6) {
        println!("    {} ({:+.0}%)", name(c), delta[c] * 100.0);
    }
    println!(
        "  vs the region that HELD ({} cells, mean Δhazard {:+.2})",
        held.len(),
        mean(held)
    );
}
