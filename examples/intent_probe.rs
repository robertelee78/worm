//! Intent-driven prediction probe. READ-ONLY on /opt/worm.
//!
//! Goal-driven personas play real games against the real CPU with a persistent
//! brain. We measure which ensemble model carries the forecast, per-model raw
//! and masked skill, and A/B upgraded intent models in a SHADOW ensemble fed by
//! the same frame stream at the same anchor.

use worm::cpu_ai::{
    compute_ensemble, mask_to_legal, COLD_START_EPISODES, ENSEMBLE_MODELS, KNN_MODEL, MODEL_NAMES,
};
use worm::{legal_options_from, option_count, Direction, Turn, WormGame};

const M_EAT: usize = 7;
const M_HUNT: usize = 8;
const M_ARM: usize = 9;

/* ------------------------------- utilities ------------------------------- */

struct Rng(u64);
impl Rng {
    fn next_f32(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
        ((self.0 >> 33) as f32) / (u32::MAX as f32 / 2.0)
    }
}

fn right_of(d: Direction) -> Direction {
    match d {
        Direction::Up => Direction::Right,
        Direction::Right => Direction::Down,
        Direction::Down => Direction::Left,
        Direction::Left => Direction::Up,
    }
}

fn dist_field(game: &WormGame, sources: &[(u16, u16)]) -> Vec<u16> {
    let w = game.width as usize;
    let h = game.height as usize;
    let mut dist = vec![u16::MAX; w * h];
    let mut q = std::collections::VecDeque::new();
    for &(x, y) in sources {
        if (x as usize) < w && (y as usize) < h {
            let i = y as usize * w + x as usize;
            if dist[i] == u16::MAX {
                dist[i] = 0;
                q.push_back((x, y));
            }
        }
    }
    while let Some((x, y)) = q.pop_front() {
        let d = dist[y as usize * w + x as usize];
        for (dx, dy) in [(0i16, -1i16), (0, 1), (-1, 0), (1, 0)] {
            let nx = x as i16 + dx;
            let ny = y as i16 + dy;
            if nx < 0 || ny < 0 || nx >= game.width as i16 || ny >= game.height as i16 {
                continue;
            }
            let (nx, ny) = (nx as u16, ny as u16);
            let i = ny as usize * w + nx as usize;
            if dist[i] != u16::MAX || !game.passable(nx, ny) {
                continue;
            }
            dist[i] = d + 1;
            q.push_back((nx, ny));
        }
    }
    dist
}

fn open_space_from(game: &WormGame, start: (u16, u16)) -> u32 {
    let w = game.width as usize;
    let h = game.height as usize;
    if !game.passable(start.0, start.1) {
        return 0;
    }
    let mut seen = vec![false; w * h];
    let mut q = std::collections::VecDeque::new();
    seen[start.1 as usize * w + start.0 as usize] = true;
    q.push_back(start);
    let mut n = 0;
    while let Some((x, y)) = q.pop_front() {
        n += 1;
        for (dx, dy) in [(0i16, -1i16), (0, 1), (-1, 0), (1, 0)] {
            let nx = x as i16 + dx;
            let ny = y as i16 + dy;
            if nx < 0 || ny < 0 || nx >= game.width as i16 || ny >= game.height as i16 {
                continue;
            }
            let (nx, ny) = (nx as u16, ny as u16);
            let i = ny as usize * w + nx as usize;
            if seen[i] || !game.passable(nx, ny) {
                continue;
            }
            seen[i] = true;
            q.push_back((nx, ny));
        }
    }
    n
}

fn step_cell(head: (u16, u16), d: Direction) -> (u16, u16) {
    let (dx, dy) = d.as_delta();
    (
        (head.0 as i16 + dx).max(0) as u16,
        (head.1 as i16 + dy).max(0) as u16,
    )
}

fn survival_step(game: &WormGame) -> Option<Direction> {
    let head = game.cycles[0].head;
    legal_options_from(game, 0, game.cycles[0].direction)
        .into_iter()
        .max_by_key(|&d| open_space_from(game, step_cell(head, d)))
}

/// BFS-routed step toward a target set with an optional survival floor.
fn bfs_step(game: &WormGame, sources: &[(u16, u16)], floor: u32) -> Option<Direction> {
    if sources.is_empty() {
        return None;
    }
    let field = dist_field(game, sources);
    let w = game.width as usize;
    let head = game.cycles[0].head;
    legal_options_from(game, 0, game.cycles[0].direction)
        .into_iter()
        .filter(|&d| {
            let c = step_cell(head, d);
            field[c.1 as usize * w + c.0 as usize] != u16::MAX
                && (floor == 0 || open_space_from(game, c) >= floor)
        })
        .min_by_key(|&d| {
            let c = step_cell(head, d);
            field[c.1 as usize * w + c.0 as usize]
        })
}

/// BFS-routed step that HOLDS THE LINE on ties: among the steps that close the
/// distance equally, take the one the player is already travelling. Humans do
/// not zigzag across an open arena; a strict distance-minimiser does.
fn bfs_step_hold(game: &WormGame, sources: &[(u16, u16)]) -> Option<Direction> {
    if sources.is_empty() {
        return None;
    }
    let field = dist_field(game, sources);
    let w = game.width as usize;
    let head = game.cycles[0].head;
    let heading = game.cycles[0].direction;
    let legal = legal_options_from(game, 0, heading);
    let d_of = |d: Direction| -> u32 {
        let c = step_cell(head, d);
        let v = field[c.1 as usize * w + c.0 as usize];
        if v == u16::MAX { u32::MAX } else { v as u32 }
    };
    let best = legal.iter().copied().map(d_of).min()?;
    if best == u32::MAX {
        return None;
    }
    if legal.contains(&heading) && d_of(heading) == best {
        return Some(heading);
    }
    legal.into_iter().filter(|&d| d_of(d) == best)
        .max_by_key(|&d| open_space_from(game, step_cell(head, d)))
}

/// One learned number: on frames where two steps closed the distance equally,
/// how often did this player hold the line rather than weave? Cheat-free — the
/// CPU can compute the tie itself and then watch what they did.
#[derive(Default, Clone, Copy)]
struct TieHabit {
    hold: f32,
    total: f32,
}
impl TieHabit {
    /// KT-smoothed, matching the turn prior's estimator.
    fn p_hold(&self) -> f32 {
        (self.hold + 0.5) / (self.total + 1.0)
    }
    fn observe(&mut self, held: bool) {
        self.hold *= 0.995;
        self.total *= 0.995;
        self.total += 1.0;
        if held {
            self.hold += 1.0;
        }
    }
}

/// The tied steps toward a target set, and the distance they achieve.
fn tied_steps(game: &WormGame, sources: &[(u16, u16)]) -> Vec<Direction> {
    if sources.is_empty() {
        return Vec::new();
    }
    let field = dist_field(game, sources);
    let w = game.width as usize;
    let head = game.cycles[0].head;
    let legal = legal_options_from(game, 0, game.cycles[0].direction);
    let d_of = |d: Direction| -> u32 {
        let c = step_cell(head, d);
        let v = field[c.1 as usize * w + c.0 as usize];
        if v == u16::MAX { u32::MAX } else { v as u32 }
    };
    let best = match legal.iter().copied().map(d_of).min() {
        Some(b) if b != u32::MAX => b,
        _ => return Vec::new(),
    };
    legal.into_iter().filter(|&d| d_of(d) == best).collect()
}

/// BFS goal step with a LEARNED tie-break.
fn bfs_step_adaptive(game: &WormGame, sources: &[(u16, u16)], tie: TieHabit) -> Option<Direction> {
    let tied = tied_steps(game, sources);
    if tied.is_empty() {
        return None;
    }
    let heading = game.cycles[0].direction;
    let head = game.cycles[0].head;
    if tied.len() == 1 {
        return Some(tied[0]);
    }
    let holds = tie.p_hold() >= 0.5;
    if holds && tied.contains(&heading) {
        return Some(heading);
    }
    if !holds {
        if let Some(d) = tied.iter().copied().find(|&d| d != heading) {
            return Some(d);
        }
    }
    tied.into_iter().max_by_key(|&d| open_space_from(game, step_cell(head, d)))
}

/// Greedy Manhattan step toward the nearest-Manhattan target — the SHIPPED shape.
fn greedy_step(game: &WormGame, targets: &[(u16, u16)]) -> Option<Direction> {
    let (px, py) = game.cycles[0].head;
    let target = targets
        .iter()
        .copied()
        .min_by_key(|&(x, y)| (x as i32 - px as i32).abs() + (y as i32 - py as i32).abs())?;
    legal_options_from(game, 0, game.cycles[0].direction)
        .into_iter()
        .min_by_key(|d| {
            let c = step_cell((px, py), *d);
            (c.0 as i32 - target.0 as i32).abs() + (c.1 as i32 - target.1 as i32).abs()
        })
}

fn adjacent_passable(game: &WormGame, cell: (u16, u16)) -> Vec<(u16, u16)> {
    [(0i16, -1i16), (0, 1), (-1, 0), (1, 0)]
        .into_iter()
        .filter_map(|(dx, dy)| {
            let nx = cell.0 as i16 + dx;
            let ny = cell.1 as i16 + dy;
            if nx < 0 || ny < 0 || nx >= game.width as i16 || ny >= game.height as i16 {
                return None;
            }
            let c = (nx as u16, ny as u16);
            game.passable(c.0, c.1).then_some(c)
        })
        .collect()
}

/* -------------------------------- personas -------------------------------- */

#[derive(Clone, Copy, PartialEq, Debug)]
enum Persona {
    FoodSeeker,
    /// Human-ish forager: commits to one morsel, coasts straight while that
    /// still closes the distance, turns once when it must, and lapses 10% of
    /// the time. Deliberately NOT strict-BFS, so a BFS model cannot match it
    /// by construction.
    HumanFood,
    ArmSeeker,
    Hunter,
    WallFollower,
}

impl Persona {
    fn name(self) -> &'static str {
        match self {
            Persona::FoodSeeker => "food-seeker (strict BFS)",
            Persona::HumanFood => "human forager (noisy)",
            Persona::ArmSeeker => "powerup-seeker",
            Persona::Hunter => "hunter (wall-in)",
            Persona::WallFollower => "wall-follower (NULL)",
        }
    }
    /// The model this persona's intent should elect, if intent inference works.
    fn matching_model(self) -> Option<usize> {
        match self {
            Persona::FoodSeeker | Persona::HumanFood => Some(M_EAT),
            Persona::ArmSeeker => Some(M_ARM),
            Persona::Hunter => Some(M_HUNT),
            Persona::WallFollower => None,
        }
    }
}

fn food_sources(game: &WormGame) -> Vec<(u16, u16)> {
    game.food_items.iter().map(|&(x, y, _)| (x, y)).collect()
}
fn powerup_sources(game: &WormGame) -> Vec<(u16, u16)> {
    game.powerups.iter().map(|&(x, y, _)| (x, y)).collect()
}
fn mine_sources(game: &WormGame) -> Vec<(u16, u16)> {
    game.bombs
        .iter()
        .filter(|b| b.owner == 1)
        .map(|b| (b.x, b.y))
        .collect()
}

/// The target set the persona is actually routing to this frame (ground truth).
fn persona_targets(p: Persona, game: &WormGame) -> Vec<(u16, u16)> {
    match p {
        Persona::FoodSeeker | Persona::HumanFood => food_sources(game),
        Persona::ArmSeeker => {
            let pu = powerup_sources(game);
            if pu.is_empty() {
                food_sources(game)
            } else {
                pu
            }
        }
        Persona::Hunter => adjacent_passable(game, game.cycles[1].head),
        Persona::WallFollower => Vec::new(),
    }
}

struct Act {
    dir: Direction,
    /// The persona pursued its goal this frame (rather than falling back to
    /// survival), and had a real choice.
    goal_frame: bool,
}

fn act(p: Persona, game: &WormGame, rng: &mut Rng, commit: &mut Option<(u16, u16)>) -> Act {
    let cur = game.cycles[0].direction;
    let legal = legal_options_from(game, 0, cur);
    if legal.is_empty() {
        return Act { dir: cur, goal_frame: false };
    }
    const FLOOR: u32 = 30;
    if p == Persona::HumanFood {
        let targets = food_sources(game);
        if targets.is_empty() {
            return Act { dir: survival_step(game).unwrap_or(legal[0]), goal_frame: false };
        }
        // Attention lapse: coast.
        if rng.next_f32() < 0.10 && legal.contains(&cur) {
            return Act { dir: cur, goal_frame: false };
        }
        let head = game.cycles[0].head;
        let still = commit.map(|t| targets.contains(&t)).unwrap_or(false);
        if !still {
            let from_head = dist_field(game, &[head]);
            let w = game.width as usize;
            *commit = targets.iter().copied().min_by_key(|&(x, y)| {
                from_head[y as usize * w + x as usize]
            });
        }
        let t = match *commit {
            Some(t) => t,
            None => return Act { dir: survival_step(game).unwrap_or(legal[0]), goal_frame: false },
        };
        let field = dist_field(game, &[t]);
        let w = game.width as usize;
        let d_of = |d: Direction| -> u32 {
            let c = step_cell(head, d);
            let v = field[c.1 as usize * w + c.0 as usize];
            if v == u16::MAX { u32::MAX } else { v as u32 }
        };
        let best = legal.iter().copied().map(d_of).min().unwrap_or(u32::MAX);
        if best == u32::MAX {
            return Act { dir: survival_step(game).unwrap_or(legal[0]), goal_frame: false };
        }
        // Never zigzag: hold the line whenever it still closes the distance.
        let pick = if legal.contains(&cur) && d_of(cur) == best {
            cur
        } else {
            legal
                .iter()
                .copied()
                .filter(|&d| d_of(d) == best)
                .max_by_key(|&d| open_space_from(game, step_cell(head, d)))
                .unwrap_or(legal[0])
        };
        if open_space_from(game, step_cell(head, pick)) < FLOOR {
            return Act { dir: survival_step(game).unwrap_or(legal[0]), goal_frame: false };
        }
        return Act { dir: pick, goal_frame: legal.len() >= 2 };
    }
    let want = match p {
        Persona::WallFollower => {
            Some(if legal.contains(&cur) { cur } else { right_of(cur) })
        }
        _ => bfs_step(game, &persona_targets(p, game), FLOOR),
    };
    match want {
        Some(d) if legal.contains(&d) => Act { dir: d, goal_frame: legal.len() >= 2 },
        _ => Act {
            dir: survival_step(game).unwrap_or(legal[0]),
            goal_frame: false,
        },
    }
}

/* ---------------------------- shadow ensemble ---------------------------- */

/// Faithful local replica of `Ensemble`'s fixed-share scoring + argmax
/// selection, so candidate model sets A/B on the identical stream.
#[derive(Clone)]
struct Shadow {
    n: usize,
    num: Vec<f32>,
    den: Vec<f32>,
    hits: Vec<u32>,
    total: Vec<u32>,
    w_fast: Vec<f32>,
    w_slow: Vec<f32>,
    pending: Vec<Option<Direction>>,
    active: usize,
    drive_hits: [u32; 4],
    drive_n: [u32; 4],
    src_count: Vec<u32>,
    src_hits: Vec<u32>,
    weight_sum: Vec<f64>,
    weight_samples: u32,
    knn_warm: bool,
    /// Judge models only on frames where they disagree (McNemar logic).
    discordant_only: bool,
    scored_frames: u32,
    /// No forecast at all (every model abstained) — coverage loss.
    silent: u32,
}

impl Shadow {
    fn new(n: usize) -> Self {
        Shadow {
            n,
            num: vec![0.0; n],
            den: vec![0.0; n],
            hits: vec![0; n],
            total: vec![0; n],
            w_fast: vec![1.0; n],
            w_slow: vec![1.0; n],
            pending: vec![None; n],
            active: 0,
            drive_hits: [0; 4],
            drive_n: [0; 4],
            src_count: vec![0; n],
            src_hits: vec![0; n],
            weight_sum: vec![0.0; n],
            weight_samples: 0,
            knn_warm: false,
            discordant_only: false,
            scored_frames: 0,
            silent: 0,
        }
    }
    fn snapshot_weights(&mut self) {
        for i in 0..self.n {
            self.weight_sum[i] += (self.w_fast[i] + self.w_slow[i]) as f64 / 2.0;
        }
        self.weight_samples += 1;
    }
    fn reset_game(&mut self) {
        self.num = vec![0.0; self.n];
        self.den = vec![0.0; self.n];
        self.w_fast = vec![1.0; self.n];
        self.w_slow = vec![1.0; self.n];
        self.pending = vec![None; self.n];
        self.active = 0;
    }
    fn forecast(&mut self, masked: &[Option<Direction>]) {
        let mut best = usize::MAX;
        let mut best_w = f32::NEG_INFINITY;
        for i in 0..self.n {
            if self.den[i] <= 0.0 || masked[i].is_none() {
                continue;
            }
            let mut w = self.w_fast[i] + self.w_slow[i];
            if i == KNN_MODEL && self.knn_warm {
                w *= 1.15;
            }
            if w > best_w {
                best_w = w;
                best = i;
            }
        }
        if best == usize::MAX {
            best = (0..self.n).find(|&i| masked[i].is_some()).unwrap_or(0);
        }
        self.active = best;
        self.pending.clear();
        self.pending.extend_from_slice(masked);
    }
    fn score(&mut self, actual: Direction, frame: u32, class: usize) {
        let w = (frame as f32).max(1.0);
        let w2 = w * w;
        const ETA_FAST: f32 = 1.2;
        const ETA_SLOW: f32 = 0.3;
        const SHARE_FAST: f32 = 0.08;
        const SHARE_SLOW: f32 = 0.01;
        let driver = self.active;
        let driver_pred = self.pending[driver];
        // Discordance gate: a frame on which every model said the same thing
        // carries no evidence about which model is better.
        let mut distinct: Vec<Direction> = Vec::new();
        for p in self.pending.iter().flatten() {
            if !distinct.iter().any(|d| d == p) {
                distinct.push(*p);
            }
        }
        let informative = !self.discordant_only || distinct.len() >= 2;
        if informative {
            self.scored_frames += 1;
        }
        for i in 0..self.n {
            if !informative {
                self.pending[i] = None;
                continue;
            }
            if let Some(pred) = self.pending[i].take() {
                let hit = pred == actual;
                self.num[i] += if hit { w2 } else { -w2 };
                self.den[i] += w2;
                self.total[i] += 1;
                if hit {
                    self.hits[i] += 1;
                }
                let loss = if hit { 0.0 } else { 1.0 };
                self.w_fast[i] *= (-ETA_FAST * loss).exp();
                self.w_slow[i] *= (-ETA_SLOW * loss).exp();
            }
        }
        for k in 0..2 {
            if !informative {
                break;
            }
            let (weights, share) = if k == 0 {
                (&mut self.w_fast, SHARE_FAST)
            } else {
                (&mut self.w_slow, SHARE_SLOW)
            };
            let sum: f32 = weights.iter().sum();
            if sum > 0.0 && sum.is_finite() {
                let pool = share * sum / self.n as f32;
                for v in weights.iter_mut() {
                    *v = (1.0 - share) * *v + pool;
                }
                let sum: f32 = weights.iter().sum();
                let inv = self.n as f32 / sum;
                for v in weights.iter_mut() {
                    *v *= inv;
                }
            } else {
                *weights = vec![1.0; self.n];
            }
        }
        match driver_pred {
            Some(p) => {
                let hit = p == actual;
                self.src_count[driver] += 1;
                if hit {
                    self.src_hits[driver] += 1;
                }
                for c in [0usize, class] {
                    self.drive_n[c] += 1;
                    if hit {
                        self.drive_hits[c] += 1;
                    }
                }
            }
            None => {
                self.silent += 1;
                // A silent frame is a miss for coverage purposes.
                for c in [0usize, class] {
                    self.drive_n[c] += 1;
                }
            }
        }
    }
    fn rate(&self, class: usize) -> f32 {
        100.0 * self.drive_hits[class] as f32 / self.drive_n[class].max(1) as f32
    }
}

/* ------------------------ candidate intent machinery ------------------------ */

/// Goal hysteresis: commit to one target until it is gone or the errand expires.
#[derive(Default)]
struct Hysteresis {
    target: Option<(u16, u16)>,
    age: u32,
}
impl Hysteresis {
    fn step_hold(&mut self, game: &WormGame, targets: &[(u16, u16)], ttl: u32) -> Option<Direction> {
        let t = self.pick(game, targets, ttl)?;
        bfs_step_hold(game, &[t])
    }
    fn pick(&mut self, game: &WormGame, targets: &[(u16, u16)], ttl: u32) -> Option<(u16, u16)> {
        if targets.is_empty() {
            self.target = None;
            return None;
        }
        let still = self.target.map(|t| targets.contains(&t)).unwrap_or(false);
        if !still || self.age >= ttl {
            let head = game.cycles[0].head;
            let from_head = dist_field(game, &[head]);
            let w = game.width as usize;
            self.target = targets.iter().copied().min_by_key(|&(x, y)| {
                from_head[y as usize * w + x as usize]
            });
            self.age = 0;
        }
        self.age += 1;
        self.target
    }
    fn step(&mut self, game: &WormGame, targets: &[(u16, u16)], ttl: u32) -> Option<Direction> {
        if targets.is_empty() {
            self.target = None;
            return None;
        }
        let still = self.target.map(|t| targets.contains(&t)).unwrap_or(false);
        if !still || self.age >= ttl {
            let head = game.cycles[0].head;
            let from_head = dist_field(game, &[head]);
            let w = game.width as usize;
            self.target = targets.iter().copied().min_by_key(|&(x, y)| {
                let d = from_head[y as usize * w + x as usize];
                if d == u16::MAX {
                    u16::MAX as u32
                } else {
                    d as u32
                }
            });
            self.age = 0;
        }
        self.age += 1;
        let t = self.target?;
        bfs_step(game, &[t], 0)
    }
    fn reset(&mut self) {
        self.target = None;
        self.age = 0;
    }
}

/// Trajectory-aligned target inference: which target has the player been
/// closing on over the last k moves?
#[derive(Default)]
struct Trajectory {
    history: std::collections::VecDeque<(u16, u16)>,
}
impl Trajectory {
    fn push(&mut self, head: (u16, u16)) {
        self.history.push_back(head);
        while self.history.len() > 8 {
            self.history.pop_front();
        }
    }
    fn reset(&mut self) {
        self.history.clear();
    }
    fn step(&self, game: &WormGame, targets: &[(u16, u16)]) -> Option<Direction> {
        if targets.is_empty() {
            return None;
        }
        if self.history.len() < 3 {
            return bfs_step(game, targets, 0);
        }
        let first = *self.history.front().unwrap();
        let last = *self.history.back().unwrap();
        let span = (self.history.len() - 1) as i32;
        let best = targets.iter().copied().max_by_key(|&(x, y)| {
            let d0 = (first.0 as i32 - x as i32).abs() + (first.1 as i32 - y as i32).abs();
            let d1 = (last.0 as i32 - x as i32).abs() + (last.1 as i32 - y as i32).abs();
            (d0 - d1) * 1000 / span.max(1) - d1
        })?;
        bfs_step(game, &[best], 0)
    }
}

/* -------------------------------- the run -------------------------------- */

const C_ALL: usize = 0;
const C_FORCED: usize = 1;
const C_VOLTURN: usize = 2;
const C_STRAIGHT: usize = 3;
const C_NAMES: [&str; 4] = ["all", "forced-turn", "voluntary-turn", "straight-frames"];

#[derive(Default)]
struct Stats {
    src_count: [u32; ENSEMBLE_MODELS],
    src_hits: [u32; ENSEMBLE_MODELS],
    class_n: [u32; 4],
    class_hits: [u32; 4],
    class_src: [[u32; ENSEMBLE_MODELS]; 4],
    raw_n: [u32; ENSEMBLE_MODELS],
    raw_hits: [u32; ENSEMBLE_MODELS],
    abstain: [u32; ENSEMBLE_MODELS],
    masked_hits: [u64; ENSEMBLE_MODELS],
    masked_total: [u64; ENSEMBLE_MODELS],
    weight_sum: [f64; ENSEMBLE_MODELS],
    weight_samples: u32,
    // errand geometry against the persona's OWN target set
    errand_n: u32,
    greedy_eq_bfs: u32,
    greedy_eq_actual: u32,
    bfs_eq_actual: u32,
    hyst_eq_actual: u32,
    traj_eq_actual: u32,
    /// errand agreement split by frame class [all, forced, volturn, straight]
    cls_n: [u32; 4],
    cls_greedy: [u32; 4],
    cls_bfs: [u32; 4],
    cls_hyst: [u32; 4],
    // hunt anchor A/B
    hunt_n: u32,
    hunt_early_hit: u32,
    hunt_late_hit: u32,
    hunt_bfs_late_hit: u32,
    // board composition
    food_present: u32,
    powerup_present: u32,
    bait_nearest: u32,
    bait_n: u32,
    frames: u32,
    games: u32,
    forced_frames: u32,
    turn_prior: [f32; 3],
    tie_hold: f32,
    tie_n: f32,
    lifetime_lift: f32,
}

struct Run {
    stats: Stats,
    shadows: Vec<(&'static str, Shadow)>,
}

const VARIANTS: [&str; 12] = [
    "V0 baseline (shipped)",
    "V1 BFS goal step",
    "V2 V1 + hysteresis",
    "V3 V1 + trajectory",
    "V4 V1 + late hunt anchor",
    "V5 honest abstention",
    "V6 V1+hyst+late+abstain",
    "V7 V6 + hold-the-line",
    "V8 V7 + discordant-only",
    "V9 shipped + discordant",
    "V10 V7 + learned tie-break",
    "V11 intent PAIRS (12 models)",
];
/// Extra slots used only by V11: an intent model per (goal x commitment style).
const M_EAT_WEAVE: usize = 10;
const M_HUNT_WEAVE: usize = 11;
const M_ARM_WEAVE: usize = 12;
/// Variants that keep an abstention as an abstention.
const HONEST: [usize; 7] = [5, 6, 7, 8, 9, 10, 11];
/// Variants judged only on frames where the models disagree.
const DISCORD: [usize; 2] = [8, 9];

fn play(p: Persona, games: u32, seed: u64, max_frames: u32) -> Run {
    let mut game = WormGame::with_size_seed(120, 38, seed);
    let mut st = Stats::default();
    let mut rng = Rng(seed ^ 0x9E37_79B9);
    let mut commit: Option<(u16, u16)> = None;
    let mut shadows: Vec<(&'static str, Shadow)> = VARIANTS
        .iter()
        .enumerate()
        .map(|(i, v)| (*v, Shadow::new(if i == 11 { 13 } else { ENSEMBLE_MODELS })))
        .collect();
    let mut hyst_eat = Hysteresis::default();
    let mut hyst_arm = Hysteresis::default();
    let mut hyst_eat_hold = Hysteresis::default();
    let mut hyst_arm_hold = Hysteresis::default();
    let mut hyst_eat_adapt = Hysteresis::default();
    let mut hyst_arm_adapt = Hysteresis::default();
    let mut tie_eat = TieHabit::default();
    let mut tie_arm = TieHabit::default();
    let mut traj = Trajectory::default();
    let mut prev_cpu_head = game.cycles[1].head;

    for g in 0..games {
        if g > 0 {
            game.restart();
            for (_, s) in shadows.iter_mut() {
                s.reset_game();
            }
            commit = None;
            hyst_eat.reset();
            hyst_arm.reset();
            hyst_eat_hold.reset();
            hyst_arm_hold.reset();
            hyst_eat_adapt.reset();
            hyst_arm_adapt.reset();
            traj.reset();
            prev_cpu_head = game.cycles[1].head;
        }
        st.games += 1;
        let mut frames = 0;
        while !game.game_over && frames < max_frames {
            let heading = game.cycles[0].direction;
            let legal = legal_options_from(&game, 0, heading);
            if legal.is_empty() {
                break;
            }
            let forced = !legal.contains(&heading);
            let options = option_count(&game, 0).max(1);
            traj.push(game.cycles[0].head);

            let (raw, _a, _c, _t) = compute_ensemble(&game, &game.cpu_brain);
            let turn_prior = game.cpu_brain.opp_brain.turn_prior();
            let pattern_left =
                if game.cpu_brain.turn_pattern.events >= worm::cpu_ai::VOMM_MIN_EVENTS {
                    Some(game.cpu_brain.turn_pattern.p_left())
                } else {
                    None
                };
            let knn_warm = game.cpu_brain.opp_brain.episodes.len() >= COLD_START_EPISODES;

            // ---- candidate steps -------------------------------------------------
            let food = food_sources(&game);
            let mines = mine_sources(&game);
            let mut apparent = food.clone();
            apparent.extend(mines.iter().copied());
            let pu = powerup_sources(&game);
            if !food.is_empty() {
                st.food_present += 1;
            }
            if !pu.is_empty() {
                st.powerup_present += 1;
            }

            let eat_bfs = bfs_step(&game, &apparent, 0);
            let eat_hyst = hyst_eat.step(&game, &apparent, 25);
            let eat_traj = traj.step(&game, &apparent);
            let arm_bfs = bfs_step(&game, &pu, 0);
            let arm_hyst = hyst_arm.step(&game, &pu, 25);
            let arm_traj = traj.step(&game, &pu);
            let hunt_early_targets = adjacent_passable(&game, prev_cpu_head);
            let hunt_late_targets = adjacent_passable(&game, game.cycles[1].head);
            let hunt_bfs_early = bfs_step(&game, &hunt_early_targets, 0);
            let hunt_bfs_late = bfs_step(&game, &hunt_late_targets, 0);
            let hunt_greedy_late = greedy_step(&game, &[game.cycles[1].head]);
            let eat_hold = hyst_eat_hold.step_hold(&game, &apparent, 25);
            let arm_hold = hyst_arm_hold.step_hold(&game, &pu, 25);
            let hunt_hold = bfs_step_hold(&game, &hunt_late_targets);
            let eat_tgt = hyst_eat_adapt.pick(&game, &apparent, 25);
            let eat_adapt = eat_tgt.and_then(|t| bfs_step_adaptive(&game, &[t], tie_eat));
            let eat_tied = eat_tgt.map(|t| tied_steps(&game, &[t])).unwrap_or_default();
            let arm_tgt = hyst_arm_adapt.pick(&game, &pu, 25);
            let arm_adapt = arm_tgt.and_then(|t| bfs_step_adaptive(&game, &[t], tie_arm));
            let hunt_adapt = bfs_step_adaptive(&game, &hunt_late_targets, tie_eat);

            let build = |v: usize| -> Vec<Option<Direction>> {
                let mut r = raw.to_vec();
                match v {
                    0 | 5 => {}
                    1 | 4 => {
                        r[M_EAT] = eat_bfs;
                        r[M_ARM] = arm_bfs;
                        r[M_HUNT] = if v == 4 { hunt_bfs_late } else { hunt_bfs_early };
                    }
                    2 => {
                        r[M_EAT] = eat_hyst;
                        r[M_ARM] = arm_hyst;
                        r[M_HUNT] = hunt_bfs_early;
                    }
                    3 => {
                        r[M_EAT] = eat_traj;
                        r[M_ARM] = arm_traj;
                        r[M_HUNT] = hunt_bfs_early;
                    }
                    6 => {
                        r[M_EAT] = eat_hyst;
                        r[M_ARM] = arm_hyst;
                        r[M_HUNT] = hunt_bfs_late;
                    }
                    7 | 8 => {
                        r[M_EAT] = eat_hold;
                        r[M_ARM] = arm_hold;
                        r[M_HUNT] = hunt_hold;
                    }
                    10 => {
                        r[M_EAT] = eat_adapt;
                        r[M_ARM] = arm_adapt;
                        r[M_HUNT] = hunt_adapt;
                    }
                    11 => {
                        // hold-the-line family
                        r[M_EAT] = eat_hold;
                        r[M_ARM] = arm_hold;
                        r[M_HUNT] = hunt_hold;
                        // ...and its weaving twin, exactly as wlR/wlL pair up
                        r.push(eat_hyst);
                        r.push(hunt_bfs_late);
                        r.push(arm_hyst);
                    }
                    _ => {}
                }
                r
            };

            for (i, (_, s)) in shadows.iter_mut().enumerate() {
                s.knn_warm = knn_warm;
                let honest = HONEST.contains(&i);
                s.discordant_only = DISCORD.contains(&i);
                let set = build(i);
                let masked: Vec<Option<Direction>> = set
                    .iter()
                    .map(|&pp| {
                        if honest && pp.is_none() {
                            None
                        } else {
                            mask_to_legal(pp, &legal, heading, &turn_prior, pattern_left)
                        }
                    })
                    .collect();
                s.forecast(&masked);
            }

            if !apparent.is_empty() {
                let head = game.cycles[0].head;
                let nearest = apparent
                    .iter()
                    .copied()
                    .min_by_key(|&(x, y)| {
                        (x as i32 - head.0 as i32).abs() + (y as i32 - head.1 as i32).abs()
                    })
                    .unwrap();
                st.bait_n += 1;
                if mines.contains(&nearest) {
                    st.bait_nearest += 1;
                }
            }

            // ---- the persona moves ----
            let a = act(p, &game, &mut rng, &mut commit);
            game.change_direction(a.dir);
            let actual = game.cycles[0].direction;

            if eat_tied.len() >= 2 && eat_tied.contains(&actual) {
                tie_eat.observe(actual == heading);
            }
            if let Some(t) = arm_tgt {
                let at = tied_steps(&game, &[t]);
                if at.len() >= 2 && at.contains(&actual) {
                    tie_arm.observe(actual == heading);
                }
            }

            // errand geometry against the persona's own targets
            let ptargets = persona_targets(p, &game);
            if a.goal_frame && !ptargets.is_empty() {
                st.errand_n += 1;
                let g_step = greedy_step(&game, &ptargets);
                let b_step = bfs_step(&game, &ptargets, 0);
                let h_step = match p {
                    Persona::ArmSeeker => arm_hyst.or(eat_hyst),
                    _ => eat_hyst,
                };
                let t_step = match p {
                    Persona::ArmSeeker => arm_traj.or(eat_traj),
                    Persona::Hunter => bfs_step(&game, &ptargets, 0),
                    _ => eat_traj,
                };
                if g_step.is_some() && g_step == b_step {
                    st.greedy_eq_bfs += 1;
                }
                if g_step == Some(actual) {
                    st.greedy_eq_actual += 1;
                }
                if b_step == Some(actual) {
                    st.bfs_eq_actual += 1;
                }
                if h_step == Some(actual) {
                    st.hyst_eq_actual += 1;
                }
                if t_step == Some(actual) {
                    st.traj_eq_actual += 1;
                }
                let cls = if forced {
                    C_FORCED
                } else if Turn::from_dirs(heading, actual).map(|t| t != Turn::Straight) == Some(true) {
                    C_VOLTURN
                } else {
                    C_STRAIGHT
                };
                for c in [C_ALL, cls] {
                    st.cls_n[c] += 1;
                    if g_step == Some(actual) { st.cls_greedy[c] += 1; }
                    if b_step == Some(actual) { st.cls_bfs[c] += 1; }
                    if h_step == Some(actual) { st.cls_hyst[c] += 1; }
                }
            }

            // hunt anchor A/B
            st.hunt_n += 1;
            if raw[M_HUNT] == Some(actual) {
                st.hunt_early_hit += 1;
            }
            if hunt_greedy_late == Some(actual) {
                st.hunt_late_hit += 1;
            }
            if hunt_bfs_late == Some(actual) {
                st.hunt_bfs_late_hit += 1;
            }

            for i in 0..ENSEMBLE_MODELS {
                match raw[i] {
                    Some(pred) => {
                        st.raw_n[i] += 1;
                        if pred == actual {
                            st.raw_hits[i] += 1;
                        }
                    }
                    None => st.abstain[i] += 1,
                }
            }

            game.update();
            frames += 1;
            st.frames += 1;
            if forced {
                st.forced_frames += 1;
            }

            let sclass = if forced {
                C_FORCED
            } else if Turn::from_dirs(heading, actual).map(|t| t != Turn::Straight) == Some(true) {
                C_VOLTURN
            } else {
                C_STRAIGHT
            };

            if let Some(sc) = game.cpu_telemetry.scored {
                let src = sc.forecast.source.min(ENSEMBLE_MODELS - 1);
                st.src_count[src] += 1;
                if sc.hit {
                    st.src_hits[src] += 1;
                }
                for c in [C_ALL, sclass] {
                    st.class_n[c] += 1;
                    if sc.hit {
                        st.class_hits[c] += 1;
                    }
                    st.class_src[c][src] += 1;
                }
                let _ = options;
            }

            for (_, s) in shadows.iter_mut() {
                s.score(actual, game.frame_count, sclass);
            }
            prev_cpu_head = game.cycles[1].head;
        }
        let e = &game.cpu_brain.ensemble;
        for i in 0..ENSEMBLE_MODELS {
            st.masked_hits[i] += e.hits[i] as u64;
            st.masked_total[i] += e.total[i] as u64;
            st.weight_sum[i] += (e.w_fast[i] + e.w_slow[i]) as f64 / 2.0;
        }
        st.weight_samples += 1;
        for (_, s) in shadows.iter_mut() {
            s.snapshot_weights();
        }
    }
    st.tie_hold = tie_eat.p_hold();
    st.tie_n = tie_eat.total;
    st.turn_prior = game.cpu_brain.opp_brain.turn_prior();
    st.lifetime_lift = if game.cpu_brain.lifetime_read.is_ready() {
        game.cpu_brain.lifetime_read.lift()
    } else {
        f32::NAN
    };
    Run { stats: st, shadows }
}

fn pct(a: u32, b: u32) -> f32 {
    if b == 0 {
        0.0
    } else {
        100.0 * a as f32 / b as f32
    }
}

fn main() {
    let games: u32 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(30);
    let max_frames: u32 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(1500);
    let seed: u64 = std::env::args().nth(3).and_then(|s| s.parse().ok()).unwrap_or(20260805);

    for p in [
        Persona::FoodSeeker,
        Persona::HumanFood,
        Persona::ArmSeeker,
        Persona::Hunter,
        Persona::WallFollower,
    ] {
        let run = play(p, games, seed, max_frames);
        let st = &run.stats;
        println!("\n================ {} ================", p.name());
        println!(
            "games {}  frames {}  forced-turn {} ({:.1}%)  food-on-board {:.1}%  powerup-on-board {:.1}%",
            st.games, st.frames, st.forced_frames,
            pct(st.forced_frames, st.frames),
            pct(st.food_present, st.frames),
            pct(st.powerup_present, st.frames),
        );
        println!(
            "learned turn prior  S {:.3}  L {:.3}  R {:.3}   |  lifetime read lift {:.3}  |  learned tie-hold {:.3} (mass {:.0})",
            st.turn_prior[0], st.turn_prior[1], st.turn_prior[2], st.lifetime_lift, st.tie_hold, st.tie_n
        );

        println!("\n  model   select-share  drive-hit%  raw-skill%(n)      abstain%  masked-hit%  mean-w");
        let total_sel: u32 = st.src_count.iter().sum();
        for i in 0..ENSEMBLE_MODELS {
            let mark = if p.matching_model() == Some(i) { "*" } else { " " };
            println!(
                " {}{:<6} {:>6.2}% ({:>5})  {:>7.1}%   {:>6.1}% ({:>5})  {:>6.1}%   {:>7.1}%  {:>6.3}",
                mark,
                MODEL_NAMES[i],
                pct(st.src_count[i], total_sel),
                st.src_count[i],
                pct(st.src_hits[i], st.src_count[i]),
                pct(st.raw_hits[i], st.raw_n[i]),
                st.raw_n[i],
                pct(st.abstain[i], st.frames),
                if st.masked_total[i] > 0 {
                    100.0 * st.masked_hits[i] as f32 / st.masked_total[i] as f32
                } else {
                    0.0
                },
                st.weight_sum[i] / st.weight_samples.max(1) as f64
            );
        }

        println!("\n  real-ensemble read rate by frame class:");
        for c in 0..4 {
            if st.class_n[c] == 0 {
                continue;
            }
            let mut idx: Vec<usize> = (0..ENSEMBLE_MODELS).collect();
            idx.sort_by_key(|&i| std::cmp::Reverse(st.class_src[c][i]));
            let top: Vec<String> = idx
                .iter()
                .take(3)
                .filter(|&&i| st.class_src[c][i] > 0)
                .map(|&i| format!("{} {:.0}%", MODEL_NAMES[i], pct(st.class_src[c][i], st.class_n[c])))
                .collect();
            println!(
                "    {:<18} n={:<6} read={:>5.1}%   drivers: {}",
                C_NAMES[c], st.class_n[c], pct(st.class_hits[c], st.class_n[c]), top.join("  ")
            );
        }

        if st.errand_n > 0 {
            println!(
                "\n  errand geometry (persona's own targets, goal frames n={}):\n    greedy==BFS {:.1}%  |  predicts the persona's actual move: greedy {:.1}%  BFS {:.1}%  hysteresis {:.1}%  trajectory {:.1}%",
                st.errand_n,
                pct(st.greedy_eq_bfs, st.errand_n),
                pct(st.greedy_eq_actual, st.errand_n),
                pct(st.bfs_eq_actual, st.errand_n),
                pct(st.hyst_eq_actual, st.errand_n),
                pct(st.traj_eq_actual, st.errand_n),
            );
        }
        for c in 1..4 {
            if st.cls_n[c] == 0 { continue; }
            println!(
                "      {:<16} n={:<6} greedy {:>5.1}%  BFS {:>5.1}%  hysteresis {:>5.1}%",
                C_NAMES[c], st.cls_n[c],
                pct(st.cls_greedy[c], st.cls_n[c]),
                pct(st.cls_bfs[c], st.cls_n[c]),
                pct(st.cls_hyst[c], st.cls_n[c]),
            );
        }
        println!(
            "  hunt anchor (n={}): shipped greedy@stale-CPU {:.1}%  greedy@fresh-CPU {:.1}%  BFS@fresh-CPU {:.1}%",
            st.hunt_n,
            pct(st.hunt_early_hit, st.hunt_n),
            pct(st.hunt_late_hit, st.hunt_n),
            pct(st.hunt_bfs_late_hit, st.hunt_n),
        );
        if st.bait_n > 0 {
            println!(
                "  nearest apparent food is a disguised CPU mine on {:.1}% of frames",
                pct(st.bait_nearest, st.bait_n)
            );
        }

        println!("\n  SHADOW ensembles (same stream, same masking):");
        println!(
            "    {:<26} {:>8} {:>12} {:>15} {:>10}   {:>10}",
            "variant", "read all", "forced-turn", "voluntary-turn", "straight", "match-share"
        );
        for (name, s) in run.shadows.iter() {
            let tot: u32 = s.src_count.iter().sum();
            let fam: Vec<usize> = match p.matching_model() {
                Some(M_EAT) => vec![M_EAT, M_EAT_WEAVE],
                Some(M_HUNT) => vec![M_HUNT, M_HUNT_WEAVE],
                Some(M_ARM) => vec![M_ARM, M_ARM_WEAVE],
                Some(i) => vec![i],
                None => vec![],
            };
            let fam: Vec<usize> = fam.into_iter().filter(|&i| i < s.n).collect();
            let fc: u32 = fam.iter().map(|&i| s.src_count[i]).sum();
            let fh: u32 = fam.iter().map(|&i| s.src_hits[i]).sum();
            let share = pct(fc, tot);
            let mhit = pct(fh, fc);
            println!(
                "    {:<26} {:>7.2}% {:>11.2}% {:>14.2}% {:>9.2}%   {:>6.2}% (hit {:>5.1}%)  silent {}  judged {}",
                name,
                s.rate(C_ALL),
                s.rate(C_FORCED),
                s.rate(C_VOLTURN),
                s.rate(C_STRAIGHT),
                share,
                mhit,
                s.silent,
                s.scored_frames,
            );
        }
        // Per-model weights in the best variant, to see whether intent survives.
        for (name, s) in run.shadows.iter().filter(|(n, _)| n.starts_with("V11") || n.starts_with("V7")) {
            let ws: Vec<String> = (0..s.n)
                .map(|i| {
                    let nm = if i < ENSEMBLE_MODELS { MODEL_NAMES[i] } else if i == M_EAT_WEAVE { "eatW" } else if i == M_HUNT_WEAVE { "huntW" } else { "armW" };
                    format!("{} {:.2}", nm, s.weight_sum[i] / s.weight_samples.max(1) as f64)
                })
                .collect();
            println!("    mean weights under {}: {}", name, ws.join("  "));
        }
        if let Some((name, s)) = run.shadows.last() {
            let ws: Vec<String> = (0..ENSEMBLE_MODELS)
                .map(|i| {
                    format!(
                        "{} {:.2}",
                        MODEL_NAMES[i],
                        s.weight_sum[i] / s.weight_samples.max(1) as f64
                    )
                })
                .collect();
            println!("    mean weights under {}: {}", name, ws.join("  "));
        }
    }
}
