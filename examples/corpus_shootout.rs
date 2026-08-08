//! THE FIRST CORPUS SHOOTOUT (architecture.md §7; consult-specified).
//!
//! A tiny learned head vs the counting ensemble, prequentially, on the
//! frozen recorded-round corpus. Pre-registered protocol (codex + k3):
//!
//! - Replay-vs-replay: both learners run the SAME shadow-learning
//!   replay stream in (endedAt, id) order, deduplicated; the incumbent
//!   is today's ensemble, warm across a player's rounds.
//! - Challenger predicts BEFORE each update from the same 32-dim
//!   heading-relative context the k-NN sees, as a 3-way relative
//!   softmax {straight,left,right}, legality-masked; trains online
//!   (predict-then-update, AdaGrad eta0=0.1, L2=1e-4, zero init,
//!   deterministic ties). An MLP (16 tanh) runs in the same pass, and
//!   a separate 2-way lateral head trains on decisive frames only.
//! - PRIMARY ENDPOINT (pre-registered): paired hit difference on the
//!   DECISIVE class (voluntary lateral, both laterals legal) over an
//!   INDEPENDENTLY defined frame set (player had >=2 legal moves and
//!   moved), with round-cluster bootstrap 95% CI; exact McNemar as a
//!   receipt. delta = 2pp. Splits (secondary): owner vs others,
//!   corpus deciles, arena (descriptive).
//! - A corpus win earns the LADDER, never the seat (off-policy caveat:
//!   the human's moves were elicited by the old CPU's steering).
//!
//! Usage: cargo run --release --example corpus_shootout -- /opt/worm/data/rounds
// Numeric kernels below index several parallel arrays in lockstep
// (weights, squared-gradient accumulators, activations); explicit
// indices are clearer than 3-way zips there.
#![allow(clippy::needless_range_loop)]

use worm::{Direction, WormGame};

const DIM: usize = worm::cpu_ai::PLAYER_FEATURE_DIM;

fn rel(heading: Direction, next: Direction) -> Option<usize> {
    // 0 straight, 1 left, 2 right; None = reversal (excluded).
    use Direction::*;
    if heading == next {
        return Some(0);
    }
    let left = match heading {
        Up => Left,
        Left => Down,
        Down => Right,
        Right => Up,
    };
    let right = match heading {
        Up => Right,
        Right => Down,
        Down => Left,
        Left => Up,
    };
    if next == left {
        Some(1)
    } else if next == right {
        Some(2)
    } else {
        None
    }
}
fn abs_dir(heading: Direction, r: usize) -> Direction {
    use Direction::*;
    match r {
        0 => heading,
        1 => match heading {
            Up => Left,
            Left => Down,
            Down => Right,
            Right => Up,
        },
        _ => match heading {
            Up => Right,
            Right => Down,
            Down => Left,
            Left => Up,
        },
    }
}

/// Online AdaGrad softmax head, K classes over DIM+1 inputs.
struct Head<const K: usize> {
    w: Vec<[f32; K]>,
    g2: Vec<[f32; K]>,
    l2: f32,
    eta: f32,
}
impl<const K: usize> Head<K> {
    fn new(eta: f32, l2: f32) -> Self {
        Head {
            w: vec![[0.0; K]; DIM + 1],
            g2: vec![[1e-8; K]; DIM + 1],
            l2,
            eta,
        }
    }
    fn logits(&self, x: &[f32; DIM]) -> [f32; K] {
        let mut z = [0.0f32; K];
        for k in 0..K {
            z[k] = self.w[DIM][k]; // bias
            for i in 0..DIM {
                z[k] += self.w[i][k] * x[i];
            }
        }
        z
    }
    fn predict_masked(&self, x: &[f32; DIM], legal: &[bool; K]) -> Option<usize> {
        let z = self.logits(x);
        let mut best = None;
        for k in 0..K {
            if !legal[k] {
                continue;
            }
            // Deterministic tie-break: lowest index wins on exact ties.
            if best.is_none_or(|b: usize| z[k] > z[b]) {
                best = Some(k);
            }
        }
        best
    }
    fn train(&mut self, x: &[f32; DIM], y: usize) {
        let z = self.logits(x);
        let m = z.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let mut p = [0.0f32; K];
        let mut s = 0.0;
        for k in 0..K {
            p[k] = (z[k] - m).exp();
            s += p[k];
        }
        for k in 0..K {
            p[k] /= s;
        }
        for k in 0..K {
            let err = p[k] - if k == y { 1.0 } else { 0.0 };
            for i in 0..=DIM {
                let xi = if i == DIM { 1.0 } else { x[i] };
                if xi == 0.0 {
                    continue;
                }
                let g = err * xi + if i < DIM { self.l2 * self.w[i][k] } else { 0.0 };
                self.g2[i][k] += g * g;
                self.w[i][k] -= self.eta * g / self.g2[i][k].sqrt();
            }
        }
    }
}

/// Tiny MLP: DIM -> H tanh -> K, AdaGrad. Same interface.
struct Mlp {
    w1: Vec<[f32; 16]>,
    g1: Vec<[f32; 16]>,
    w2: [[f32; 3]; 17],
    g2: [[f32; 3]; 17],
    eta: f32,
}
impl Mlp {
    fn new(eta: f32) -> Self {
        // Deterministic tiny init (hash-ish spread, no RNG dependency).
        let mut w1 = vec![[0.0f32; 16]; DIM + 1];
        for (i, row) in w1.iter_mut().enumerate() {
            for (j, v) in row.iter_mut().enumerate() {
                *v = (((i * 31 + j * 17 + 7) % 13) as f32 - 6.0) * 0.01;
            }
        }
        Mlp {
            w1,
            g1: vec![[1e-8; 16]; DIM + 1],
            w2: [[0.0; 3]; 17],
            g2: [[1e-8; 3]; 17],
            eta,
        }
    }
    fn forward(&self, x: &[f32; DIM]) -> ([f32; 16], [f32; 3]) {
        let mut h = [0.0f32; 16];
        for j in 0..16 {
            let mut a = self.w1[DIM][j];
            for i in 0..DIM {
                a += self.w1[i][j] * x[i];
            }
            h[j] = a.tanh();
        }
        let mut z = [0.0f32; 3];
        for k in 0..3 {
            z[k] = self.w2[16][k];
            for j in 0..16 {
                z[k] += self.w2[j][k] * h[j];
            }
        }
        (h, z)
    }
    fn predict_masked(&self, x: &[f32; DIM], legal: &[bool; 3]) -> Option<usize> {
        let (_, z) = self.forward(x);
        let mut best = None;
        for k in 0..3 {
            if legal[k] && best.is_none_or(|b: usize| z[k] > z[b]) {
                best = Some(k);
            }
        }
        best
    }
    fn train(&mut self, x: &[f32; DIM], y: usize) {
        let (h, z) = self.forward(x);
        let m = z.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let mut p = [0.0f32; 3];
        let mut s = 0.0;
        for k in 0..3 {
            p[k] = (z[k] - m).exp();
            s += p[k];
        }
        for k in 0..3 {
            p[k] /= s;
        }
        let mut dh = [0.0f32; 16];
        for k in 0..3 {
            let err = p[k] - if k == y { 1.0 } else { 0.0 };
            for j in 0..16 {
                dh[j] += err * self.w2[j][k];
                let g = err * h[j];
                self.g2[j][k] += g * g;
                self.w2[j][k] -= self.eta * g / self.g2[j][k].sqrt();
            }
            let g = err;
            self.g2[16][k] += g * g;
            self.w2[16][k] -= self.eta * g / self.g2[16][k].sqrt();
        }
        for j in 0..16 {
            let da = dh[j] * (1.0 - h[j] * h[j]);
            if da == 0.0 {
                continue;
            }
            for i in 0..=DIM {
                let xi = if i == DIM { 1.0 } else { x[i] };
                if xi == 0.0 {
                    continue;
                }
                let g = da * xi;
                self.g1[i][j] += g * g;
                self.w1[i][j] -= self.eta * g / self.g1[i][j].sqrt();
            }
        }
    }
}

#[derive(Clone, Default)]
struct Frame {
    round: usize,
    player: u64,
    decisive: bool,
    inc_hit: bool,
    lin_hit: bool,
    mlp_hit: bool,
    lat_hit: Option<(bool, bool)>, // (incumbent, lateral-head) on decisive
}

fn main() {
    let dir = std::env::args().nth(1).unwrap_or("/opt/worm/data/rounds".into());
    let mut rows: Vec<(u64, String, serde_json::Value)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for entry in std::fs::read_dir(&dir).expect("rounds dir") {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        for line in std::fs::read_to_string(&path).unwrap().lines() {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            let Some(rep) = v.get("replay") else { continue };
            let Some(ev) = rep.get("ev").and_then(|e| e.as_array()) else {
                continue;
            };
            if ev.is_empty() {
                continue;
            }
            // Dedup on the replay itself (identical uploads exist).
            let key = format!(
                "{}:{}",
                rep.get("seed").and_then(|s| s.as_str()).unwrap_or(""),
                ev.len()
            );
            if !seen.insert(key) {
                continue;
            }
            let ended = v.get("endedAt").and_then(|e| e.as_u64()).unwrap_or(0);
            let id = v.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
            rows.push((ended, id, v));
        }
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    eprintln!("corpus: {} deduplicated replayable rounds", rows.len());

    // Player key: the id's device prefix.
    let pkey = |id: &str| -> u64 {
        let dev = id.split(':').next().unwrap_or(id);
        let mut h = 1469598103934665603u64;
        for b in dev.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(1099511628211);
        }
        h
    };
    // Owner = most frequent device.
    let mut counts = std::collections::HashMap::new();
    for (_, id, _) in &rows {
        *counts.entry(pkey(id)).or_insert(0u32) += 1;
    }
    let owner = counts.iter().max_by_key(|(_, c)| **c).map(|(k, _)| *k).unwrap_or(0);

    let mut lin = Head::<3>::new(0.1, 1e-4);
    let mut mlp = Mlp::new(0.1);
    let mut lat = Head::<2>::new(0.1, 1e-4); // decisive-frames-only lateral head
    let mut frames: Vec<Frame> = Vec::new();
    // Warm incumbent per player: one brain per device, carried across
    // that device's rounds in corpus order.
    let mut brains: std::collections::HashMap<u64, worm::CpuBrain> =
        std::collections::HashMap::new();

    for (ri, (_, id, v)) in rows.iter().enumerate() {
        let rep = &v["replay"];
        let (Some(seed), Some(w), Some(h), Some(fr)) = (
            rep.get("seed").and_then(|s| s.as_str()).and_then(|s| s.parse::<u64>().ok()),
            rep.get("w").and_then(|x| x.as_u64()),
            rep.get("h").and_then(|x| x.as_u64()),
            rep.get("frames").and_then(|x| x.as_u64()),
        ) else {
            continue;
        };
        if !(10..=400).contains(&w) || !(10..=400).contains(&h) || fr > 100_000 {
            continue;
        }
        let arena = rep.get("arena").and_then(|a| a.as_u64()).unwrap_or(1) as u8;
        if arena == 0 || arena > worm::ARENA_VERSION {
            continue;
        }
        let events: Vec<(u32, u8, u8)> = rep["ev"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|e| {
                Some((
                    e.get(0)?.as_u64()? as u32,
                    e.get(1)?.as_u64()? as u8,
                    e.get(2)?.as_u64()? as u8,
                ))
            })
            .collect();
        let player = pkey(id);
        let mut game = WormGame::with_size_seed(w as u16, h as u16, 1);
        game.start_recorded_round(seed, w as u16, h as u16, arena, events);
        game.shadow_learning = true;
        game.cpu_brain = brains.remove(&player).unwrap_or_default();
        game.refresh_read_rate();

        let mut steps = 0u32;
        while !game.game_over && game.frame_count < fr as u32 + 4 && steps < 110_000 {
            steps += 1;
            // Predict BEFORE the world moves, from pre-move state — the
            // same information timing as the incumbent's pended forecast.
            let heading = game.cycles[0].prev_direction;
            let ctx = worm::cpu_ai::encode_player_context(&game, &game.cpu_brain.player_tail);
            let legal_dirs = worm::legal_options_from(&game, 0, heading);
            let mut legal3 = [false; 3];
            for d in &legal_dirs {
                if let Some(r) = rel(heading, *d) {
                    legal3[r] = true;
                }
            }
            let lin_pred = lin.predict_masked(&ctx, &legal3);
            let mlp_pred = mlp.predict_masked(&ctx, &legal3);
            let lat_pred = lat.predict_masked(&ctx, &[true, true]);
            let book_side = game.book_audit.take();
            game.update();
            // Score against what actually happened, on the incumbent's
            // scored frame (paired set) — and train prequentially.
            if let Some(s) = game.cpu_telemetry.scored {
                let actual = s.actual;
                let Some(y) = rel(heading, actual) else {
                    continue;
                };
                lin.train(&ctx, y);
                mlp.train(&ctx, y);
                let both_lats = legal3[1] && legal3[2];
                let straight_legal = legal3[0];
                let decisive = y != 0 && both_lats && straight_legal;
                if decisive {
                    lat.train(&ctx, y - 1);
                }
                let inc_hit = s.hit;
                let lin_hit = lin_pred.map(|p| abs_dir(heading, p) == actual).unwrap_or(false);
                let mlp_hit = mlp_pred.map(|p| abs_dir(heading, p) == actual).unwrap_or(false);
                frames.push(Frame {
                    round: ri,
                    player,
                    decisive,
                    inc_hit,
                    lin_hit,
                    mlp_hit,
                    lat_hit: if decisive {
                        // FAIR PAIR: the book's own precommitted side call
                        // (the incumbent's real side specialist) vs the
                        // learned lateral head, same frames.
                        let book_hit = book_side.map(|d| d == actual).unwrap_or(false);
                        let head_hit = lat_pred.map(|p| p + 1 == y).unwrap_or(false);
                        Some((book_hit, head_hit))
                    } else {
                        None
                    },
                });
            }
        }
        game.finalize_round_ledgers();
        game.refresh_read_rate();
        brains.insert(player, std::mem::replace(&mut game.cpu_brain, worm::CpuBrain::new()));
        if ri % 50 == 0 {
            eprintln!("  …round {}/{}", ri, rows.len());
        }
    }

    // ---- Receipts ----
    let report = |name: &str, sel: &dyn Fn(&Frame) -> Option<(bool, bool)>| {
        let picks: Vec<(usize, bool, bool)> = frames
            .iter()
            .filter_map(|f| sel(f).map(|(a, b)| (f.round, a, b)))
            .collect();
        let n = picks.len();
        if n == 0 {
            println!("{name}: n=0");
            return;
        }
        let inc = picks.iter().filter(|(_, a, _)| *a).count();
        let ch = picks.iter().filter(|(_, _, b)| *b).count();
        let b01 = picks.iter().filter(|(_, a, b)| !*a && *b).count();
        let b10 = picks.iter().filter(|(_, a, b)| *a && !*b).count();
        // Round-cluster bootstrap on the paired difference.
        let mut by_round: std::collections::HashMap<usize, (i64, usize)> =
            std::collections::HashMap::new();
        for (r, a, b) in &picks {
            let e = by_round.entry(*r).or_insert((0, 0));
            e.0 += (*b as i64) - (*a as i64);
            e.1 += 1;
        }
        let clusters: Vec<(i64, usize)> = by_round.values().cloned().collect();
        let mut rng = 0x243F6A8885A308D3u64;
        let mut next = move || {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            rng
        };
        let mut diffs: Vec<f64> = (0..2000)
            .map(|_| {
                let (mut d, mut m) = (0i64, 0usize);
                for _ in 0..clusters.len() {
                    let c = clusters[(next() % clusters.len() as u64) as usize];
                    d += c.0;
                    m += c.1;
                }
                d as f64 / m.max(1) as f64
            })
            .collect();
        diffs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let (lo, hi) = (diffs[50], diffs[1949]);
        println!(
            "{name}: n={n}  incumbent {:.1}%  challenger {:.1}%  paired diff {:+.2}pp  \
             McNemar b01={b01} b10={b10}  cluster95CI [{:+.2}, {:+.2}]pp",
            inc as f64 / n as f64 * 100.0,
            ch as f64 / n as f64 * 100.0,
            (ch as f64 - inc as f64) / n as f64 * 100.0,
            lo * 100.0,
            hi * 100.0
        );
    };

    println!("\n=== THE FIRST CORPUS SHOOTOUT ===");
    println!("frames scored: {}  (decisive: {})\n", frames.len(), frames.iter().filter(|f| f.decisive).count());
    report("ALL FRAMES     linear", &|f| Some((f.inc_hit, f.lin_hit)));
    report("ALL FRAMES     mlp   ", &|f| Some((f.inc_hit, f.mlp_hit)));
    report("DECISIVE       linear", &|f| f.decisive.then_some((f.inc_hit, f.lin_hit)));
    report("DECISIVE       mlp   ", &|f| f.decisive.then_some((f.inc_hit, f.mlp_hit)));
    report("DECISIVE  book-vs-lat", &|f| f.lat_hit);
    let declared: usize = frames.iter().filter(|f| f.lat_hit.is_some()).count();
    println!("  (book declared a side on all decisive frames tapped: {declared})");
    let owner_only = owner;
    report("OWNER decisive linear", &|f| {
        (f.decisive && f.player == owner_only).then_some((f.inc_hit, f.lin_hit))
    });
    report("OTHERS decisive linear", &|f| {
        (f.decisive && f.player != owner_only).then_some((f.inc_hit, f.lin_hit))
    });
    // Learning curve by decile (linear, decisive).
    let dec: Vec<&Frame> = frames.iter().filter(|f| f.decisive).collect();
    if dec.len() >= 10 {
        println!("\nlearning curve (decisive, by corpus decile): inc% / lin%");
        for d in 0..10 {
            let lo = d * dec.len() / 10;
            let hi = (d + 1) * dec.len() / 10;
            let sl = &dec[lo..hi];
            let i = sl.iter().filter(|f| f.inc_hit).count();
            let l = sl.iter().filter(|f| f.lin_hit).count();
            println!(
                "  d{d}: {:>5.1} / {:<5.1}  (n={})",
                i as f64 / sl.len() as f64 * 100.0,
                l as f64 / sl.len() as f64 * 100.0,
                sl.len()
            );
        }
    }
}
