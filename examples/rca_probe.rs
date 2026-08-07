//! RCA probe (ADR-020 spike): per-model, per-frame-class performance on a
//! real player's ghost corpus.
//!
//!     cargo run --release --example rca_probe -- data/players/<id>.json
//!
//! Replays every ghost-v2 round with shadow learning and, for every frame,
//! grades EACH ensemble model's masked prediction (made the frame before)
//! against the move the player actually executed — split by frame class:
//! straight-available-and-taken, voluntary turn, forced turn. Also grades
//! the base predictor (their commonest move so far) for the honest
//! comparison. This is the microscope the lift number cannot be.

use worm::cpu_ai::{Turn, ENSEMBLE_MODELS, MODEL_NAMES};
use worm::{Direction, WormGame};

/// The PRODUCTION baseline, mirrored exactly (ADR-020, codex finding: the
/// probe's original base was modal-ABSOLUTE-direction, which is not the
/// rival the shipped McNemar scores against). Modal RELATIVE turn, chosen
/// among the frame's legal set, class-conditioned on the published
/// forecast — same tie-breaks as ReadRate.
#[derive(Default)]
struct BaseMirror {
    taken: [u32; 3],
}
impl BaseMirror {
    fn modal_among(&self, legal: [bool; 3]) -> Option<usize> {
        // EXACT production parity (codex round 3): at zero history every
        // legal candidate ties and the lowest index wins — the base
        // always answers when something is legal, same as ReadRate.
        let max = (0..3).filter(|&i| legal[i]).map(|i| self.taken[i]).max()?;
        (0..3).find(|&i| legal[i] && self.taken[i] == max)
    }
    fn predict(&self, predicted_class_turn: bool, legal: [bool; 3]) -> Option<usize> {
        if predicted_class_turn {
            let lateral = [false, legal[1], legal[2]];
            if lateral.iter().any(|&b| b) {
                return self.modal_among(lateral);
            }
        }
        self.modal_among(legal)
    }
    fn observe(&mut self, turn: usize) {
        self.taken[turn] += 1;
    }
}

fn turn_of(heading: Direction, d: Direction) -> Option<usize> {
    Turn::from_dirs(heading, d).map(|t| t as usize)
}

struct Round {
    ended_at: u64,
    frames: u32,
    seed: u64,
    w: u16,
    h: u16,
    events: Vec<(u32, u8, u8)>,
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: rca_probe <player.json>");
    let text = std::fs::read_to_string(&path).expect("read");
    let v: serde_json::Value = serde_json::from_str(&text).expect("json");
    let mut rounds = Vec::new();
    for rec in v.get("rounds").and_then(|r| r.as_array()).expect("rounds") {
        let Some(replay) = rec.get("replay") else { continue };
        if replay.get("v").and_then(|x| x.as_u64()) != Some(2) {
            continue;
        }
        let seed: u64 = replay["seed"].as_str().unwrap().parse().unwrap();
        rounds.push(Round {
            ended_at: rec.get("endedAt").and_then(|x| x.as_u64()).unwrap_or(0),
            frames: replay["frames"].as_u64().unwrap() as u32,
            seed,
            w: replay["w"].as_u64().unwrap() as u16,
            h: replay["h"].as_u64().unwrap() as u16,
            events: replay["ev"]
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
                .collect(),
        });
    }
    rounds.sort_by_key(|r| r.ended_at);
    eprintln!("{} ghost rounds", rounds.len());

    let mut game = WormGame::with_size_seed(55, 40, 1);
    game.cpu_autopilot = false;
    game.shadow_learning = true;

    // [class][model] hits/total. class: 0 straight-taken, 1 voluntary turn,
    // 2 forced turn. Model index ENSEMBLE_MODELS = the base predictor,
    // +1 = the PUBLISHED forecast (active model).
    const CLASSES: usize = 3;
    let m = ENSEMBLE_MODELS + 2;
    let mut hits = vec![vec![0u32; m]; CLASSES];
    let mut total = vec![vec![0u32; m]; CLASSES];
    let mut base = BaseMirror::default();

    // Projection grading (codex D8): every 5 frames, freeze both
    // projections from the same state and score them against the5
    // realized positions; windows cut short by round end are censored
    // (dropped) rather than zero-padded.
    let mut proj_loss_straight = 0f64;
    let mut proj_loss_medoid = 0f64;
    let mut proj_windows = 0u32;
    let mut proj_loss_straight_auth = 0f64;
    let mut proj_loss_medoid_auth = 0f64;
    let mut proj_windows_auth = 0u32;
    for r in &rounds {
        game.start_recorded_round(r.seed, r.w, r.h, r.events.clone());
        let mut pend: Option<([Option<Direction>; ENSEMBLE_MODELS], Option<Direction>)> = None;
        let mut prev_heading = game.cycles[0].direction;
        let mut prev_legal = worm::legal_options_from(&game, 0, prev_heading);
        let mut proj_pending: Option<(Vec<(u16, u16)>, Vec<(u16, u16)>, usize)> = None;
        let mut realized: Vec<(u16, u16)> = Vec::new();
        while !game.game_over && game.frame_count <= r.frames {
            // Freeze both projections from the CURRENT state every 5th frame.
            if proj_pending.is_none() {
                let base = worm::cpu_ai::project_player_straight(&game, 5);
                let bent = worm::cpu_ai::project_player_book(&game, 5);
                proj_pending = Some((base, bent, 0));
                realized.clear();
            }
            game.update();
            realized.push(game.cycles[0].head);
            if let Some((base, bent, _)) = &proj_pending {
                if realized.len() == 5 {
                    let loss = |p: &Vec<(u16, u16)>| -> f64 {
                        p.iter()
                            .zip(realized.iter())
                            .map(|(&(ax, ay), &(bx, by))| {
                                ((ax as i32 - bx as i32).abs()
                                    + (ay as i32 - by as i32).abs())
                                    as f64
                            })
                            .sum()
                    };
                    proj_loss_straight += loss(base);
                    proj_loss_medoid += loss(bent);
                    proj_windows += 1;
                    if game.cpu_brain.book_authority_snapshot {
                        proj_loss_straight_auth += loss(base);
                        proj_loss_medoid_auth += loss(bent);
                        proj_windows_auth += 1;
                    }
                    proj_pending = None;
                }
            }
            let actual = game.cycles[0].direction;
            // Frame class from the PRE-move state.
            let straight_ok = prev_legal.contains(&prev_heading);
            let class = if !straight_ok {
                2
            } else if actual != prev_heading {
                1
            } else {
                0
            };
            // Production baseline, at production information parity.
            let legal3 = {
                let mut l = [false; 3];
                for &d in &prev_legal {
                    if let Some(t) = turn_of(prev_heading, d) {
                        l[t] = true;
                    }
                }
                l
            };
            let actual_turn = turn_of(prev_heading, actual);
            if let Some((models, published)) = pend {
                for (i, p) in models.iter().enumerate() {
                    if let Some(p) = p {
                        total[class][i] += 1;
                        if *p == actual {
                            hits[class][i] += 1;
                        }
                    }
                }
                let predicted_class_turn = published
                    .and_then(|p| turn_of(prev_heading, p))
                    .map(|t| t != 0)
                    .unwrap_or(false);
                total[class][ENSEMBLE_MODELS] += 1;
                if base.predict(predicted_class_turn, legal3) == actual_turn
                    && actual_turn.is_some()
                {
                    hits[class][ENSEMBLE_MODELS] += 1;
                }
                if let Some(p) = published {
                    total[class][ENSEMBLE_MODELS + 1] += 1;
                    if p == actual {
                        hits[class][ENSEMBLE_MODELS + 1] += 1;
                    }
                }
            }
            if let Some(t) = actual_turn {
                base.observe(t);
            }
            // Snapshot this frame's pendings (they target the NEXT frame).
            pend = Some((game.cpu_brain.ensemble.pending, game.cpu_brain.ensemble.predicted_dir));
            prev_heading = actual;
            prev_legal = worm::legal_options_from(&game, 0, actual);
        }
    }

    let books_after_pass1 = game.cpu_brain.class_books.clone();

    // ---- WHY-HAZARD spike: does target misalignment predict his turns? ----
    // A FRESH game+brain: replaying the corpus into the already-trained
    // brain would double-count every event (codex round 2 — the 1,983-
    // event figure from the first probe run was exactly this), so pass 1
    // above stays the single prequential pass all statistics come from.
    let mut game = WormGame::with_size_seed(55, 40, 1);
    game.cpu_autopilot = false;
    game.shadow_learning = true;
    let mut mis = [[0u32; 2]; 2]; // [aligned?][turned?] counts
    let mut toward_hits = 0u32;
    let mut toward_total = 0u32;
    for r in &rounds {
        game.start_recorded_round(r.seed, r.w, r.h, r.events.clone());
        let mut prev_heading = game.cycles[0].direction;
        let mut prev_aligned: Option<bool> = None;
        let mut prev_food_side: Option<worm::Direction> = None;
        while !game.game_over && game.frame_count <= r.frames {
            // Pre-move state: alignment + which perpendicular side food is on.
            let (px, py) = game.cycles[0].head;
            let nearest = game
                .food_items
                .iter()
                .min_by_key(|&&(x, y, _)| (x as i32 - px as i32).abs() + (y as i32 - py as i32).abs())
                .copied();
            let (aligned, food_side) = if let Some((fx, fy, _)) = nearest {
                let (dx, dy) = prev_heading.as_delta();
                let closing = (fx as i32 - px as i32) * dx as i32
                    + (fy as i32 - py as i32) * dy as i32
                    > 0;
                // Perpendicular side of the target relative to heading.
                let cross = dx as i32 * (fy as i32 - py as i32)
                    - dy as i32 * (fx as i32 - px as i32);
                let left_of = match prev_heading {
                    worm::Direction::Up => worm::Direction::Left,
                    worm::Direction::Left => worm::Direction::Down,
                    worm::Direction::Down => worm::Direction::Right,
                    worm::Direction::Right => worm::Direction::Up,
                };
                let right_of = match prev_heading {
                    worm::Direction::Up => worm::Direction::Right,
                    worm::Direction::Right => worm::Direction::Down,
                    worm::Direction::Down => worm::Direction::Left,
                    worm::Direction::Left => worm::Direction::Up,
                };
                let side = if cross > 0 { Some(right_of) } else if cross < 0 { Some(left_of) } else { None };
                (closing, side)
            } else {
                (true, None)
            };
            game.update();
            let actual = game.cycles[0].direction;
            let turned = actual != prev_heading;
            if let Some(a) = prev_aligned {
                mis[a as usize][turned as usize] += 1;
                if turned && !a {
                    toward_total += 1;
                    if Some(actual) == prev_food_side {
                        toward_hits += 1;
                    }
                }
            }
            prev_aligned = Some(aligned);
            prev_food_side = food_side;
            prev_heading = actual;
        }
    }
    println!("
== WHY-HAZARD (target misalignment) ==");
    let p_t_mis = mis[0][1] as f32 / (mis[0][0] + mis[0][1]).max(1) as f32;
    let p_t_al = mis[1][1] as f32 / (mis[1][0] + mis[1][1]).max(1) as f32;
    println!(
        "  P(turn | MISALIGNED to nearest food) = {:.1}%  (n={})",
        100.0 * p_t_mis,
        mis[0][0] + mis[0][1]
    );
    println!(
        "  P(turn | aligned)                    = {:.1}%  (n={})",
        100.0 * p_t_al,
        mis[1][0] + mis[1][1]
    );
    println!(
        "  when misaligned+turning, turn is TOWARD food side: {:.1}% ({}/{})",
        100.0 * toward_hits as f32 / toward_total.max(1) as f32,
        toward_hits,
        toward_total
    );

    // Turn-book diagnostics after the full corpus (single prequential pass).
    {
        let b = &books_after_pass1;
        let mut max_h = 0.0f32;
        let mut hot = 0usize;
        for cell in 0..worm::cpu_ai::HAZARD_CELLS {
            if b.hz_total[cell] >= 5.0 {
                let h = b.hazard(cell);
                if h > max_h {
                    max_h = h;
                    hot = cell;
                }
            }
        }
        println!(
            "\n== TURN BOOK (prequential, pass 1) == events={} aT={:.2} aS={:.2} coverage={:.2} gate_open={} max_h={:.2} (cell {:#08b}, n={:.0})",
            b.turn_events,
            b.a_turn(),
            b.a_straight(),
            b.coverage(),
            b.gate_open,
            max_h,
            hot,
            b.hz_total[hot]
        );
        println!(
            "   book_read: side_opps={} decls={} earned={:.2} spendable={:.2} authority={}",
            b.side_opportunities,
            b.side_declarations,
            b.book_read.earned_read(),
            b.spendable(),
            b.projection_authority(),
        );
        println!(
            "   projection paired loss (sum manhattan over 5f, lower=better): straight={:.0} medoid={:.0} on {} scored windows",
            proj_loss_straight, proj_loss_medoid, proj_windows
        );
        println!(
            "   authority-active subset: straight={:.0} medoid={:.0} on {} windows",
            proj_loss_straight_auth, proj_loss_medoid_auth, proj_windows_auth
        );
    }

    let class_names = ["straight", "VOLUNTARY-TURN", "forced-turn"];
    for c in 0..CLASSES {
        println!("\n== {} frames ==", class_names[c]);
        let mut rows: Vec<(String, u32, u32)> = (0..ENSEMBLE_MODELS)
            .map(|i| (MODEL_NAMES[i].to_string(), hits[c][i], total[c][i]))
            .collect();
        rows.push(("BASE".into(), hits[c][ENSEMBLE_MODELS], total[c][ENSEMBLE_MODELS]));
        rows.push(("PUBLISHED".into(), hits[c][ENSEMBLE_MODELS + 1], total[c][ENSEMBLE_MODELS + 1]));
        for (name, h, t) in rows {
            if t > 0 {
                println!("  {:<10} {:>5.1}%  ({}/{})", name, 100.0 * h as f32 / t as f32, h, t);
            }
        }
    }
}
