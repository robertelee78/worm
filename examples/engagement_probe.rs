//! Engagement-quality + death-forensics probe for /opt/worm.
//! READ-ONLY on the repo; this crate is a path-dep consumer.

use std::collections::{HashMap, VecDeque};
use worm::game::DeathCause;
use worm::{CellType, Direction, WormGame};

// ---------------------------------------------------------------- rng/persona

struct Rng(u64);
impl Rng {
    fn next_f32(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
        ((self.0 >> 33) as f32) / (u32::MAX as f32 / 2.0)
    }
}

fn left_of(d: Direction) -> Direction {
    match d {
        Direction::Up => Direction::Left,
        Direction::Left => Direction::Down,
        Direction::Down => Direction::Right,
        Direction::Right => Direction::Up,
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

fn can_step(game: &WormGame, d: Direction) -> bool {
    worm::legal_options_from(game, 0, game.cycles[0].direction).contains(&d)
}

/// Exactly the `habitual` opponent from tests/domination.rs.
fn habitual(game: &WormGame, rng: &mut Rng) -> Direction {
    let cur = game.cycles[0].direction;
    let (l, r) = (left_of(cur), right_of(cur));
    let own_len = game.cycles[0].positions.len() as f32;
    let survivable = |d: Direction| -> bool {
        if !can_step(game, d) {
            return false;
        }
        let (dx, dy) = d.as_delta();
        let nx = (game.cycles[0].head.0 as i16 + dx).max(0) as u16;
        let ny = (game.cycles[0].head.1 as i16 + dy).max(0) as u16;
        worm::count_open_space(game, nx, ny) >= own_len * 3.0 + 8.0
    };
    if survivable(cur) {
        return cur;
    }
    let (first, second) = if rng.next_f32() < 0.85 { (l, r) } else { (r, l) };
    for d in [first, second] {
        if survivable(d) {
            return d;
        }
    }
    for d in [cur, first, second] {
        if can_step(game, d) {
            return d;
        }
    }
    cur
}

/// A food-hungry opponent: routes to the best-value nearby food when it can do
/// so safely, otherwise behaves like `habitual`. Used to check the CPU's food
/// share against an opponent that actually contests the economy.
fn forager(game: &WormGame, rng: &mut Rng) -> Direction {
    let cur = game.cycles[0].direction;
    let (hx, hy) = game.cycles[0].head;
    let own_len = game.cycles[0].positions.len() as f32;
    let mut best: Option<(f32, Direction)> = None;
    for d in [cur, left_of(cur), right_of(cur)] {
        if !can_step(game, d) {
            continue;
        }
        let (dx, dy) = d.as_delta();
        let nx = (hx as i16 + dx).max(0) as u16;
        let ny = (hy as i16 + dy).max(0) as u16;
        if worm::count_open_space(game, nx, ny) < own_len * 3.0 + 8.0 {
            continue;
        }
        for &(fx, fy, fv) in &game.food_items {
            let dist = (nx as i16 - fx as i16).abs() + (ny as i16 - fy as i16).abs();
            let score = fv as f32 / (dist as f32 + 1.0);
            if best.map_or(true, |(b, _)| score > b) {
                best = Some((score, d));
            }
        }
    }
    match best {
        Some((_, d)) => d,
        None => habitual(game, rng),
    }
}

// -------------------------------------------------------- geometry / metrics

/// Chebyshev-free distance from (x,y) to the nearest live arena wall line.
fn wall_dist(game: &WormGame, x: u16, y: u16) -> i32 {
    if !game.has_corridor() {
        return 99;
    }
    let off = game.arena_wall_offset() as i32;
    let (w, h) = (game.width as i32, game.height as i32);
    let (x, y) = (x as i32, y as i32);
    let l = (x - off).abs();
    let r = (x - (w - 1 - off)).abs();
    let t = (y - off).abs();
    let b = (y - (h - 1 - off)).abs();
    l.min(r).min(t).min(b)
}

/// Is (x,y) inside a 6x6 box anchored at one of the four playable corners?
fn in_corner_box(game: &WormGame, x: u16, y: u16) -> bool {
    if !game.has_corridor() {
        return false;
    }
    let off = game.arena_wall_offset() as i32;
    let (w, h) = (game.width as i32, game.height as i32);
    let (lo_x, hi_x) = (off + 1, w - 2 - off);
    let (lo_y, hi_y) = (off + 1, h - 2 - off);
    let (x, y) = (x as i32, y as i32);
    let near_x = x <= lo_x + 5 || x >= hi_x - 5;
    let near_y = y <= lo_y + 5 || y >= hi_y - 5;
    near_x && near_y
}

// ------------------------------------------- tail-aware reachability (probe)

/// Timed flood fill that models tail retraction.
///
/// `count_open_space` treats every body cell as a permanent wall, so a coil of
/// length L reports only the cells outside the coil even though the coil is
/// survivable by tail-chasing. This BFS advances a clock with the frontier:
/// cell `positions[i]` of cycle `who` becomes enterable at time
/// `len - i + pending_growth`.
///
/// Returns `(reachable_cells, tail_reachable)`. `tail_reachable` is the classic
/// snake survival invariant: after the move, can the head still reach its own
/// tail (and therefore keep following it indefinitely)?
fn tail_aware_reach(game: &WormGame, who: usize, from: (u16, u16)) -> (f32, bool) {
    let w = game.width as usize;
    let h = game.height as usize;
    let c = &game.cycles[who];
    let len = c.positions.len() as i32;
    let grow = c.pending_growth as i32;
    if len == 0 {
        return (0.0, false);
    }

    let mut vacate = vec![i32::MAX; w * h];
    for (i, &(px, py)) in c.positions.iter().enumerate() {
        let t = len - i as i32 + grow;
        let idx = py as usize * w + px as usize;
        if t < vacate[idx] {
            vacate[idx] = t;
        }
    }
    // The opponent's tail retracts too; treated as static here — conservative,
    // and it is the CPU's OWN coil the hypothesis is about.

    let tail_cell = *c.positions.last().unwrap();
    let tail_idx = tail_cell.1 as usize * w + tail_cell.0 as usize;

    let mut best_t = vec![i32::MAX; w * h];
    let mut q: VecDeque<(u16, u16, i32)> = VecDeque::new();
    let start_idx = from.1 as usize * w + from.0 as usize;
    let start_ok = game.passable(from.0, from.1) || vacate[start_idx] <= 1;
    if !start_ok {
        return (0.0, false);
    }
    best_t[start_idx] = 1;
    q.push_back((from.0, from.1, 1));

    let mut count = 0.0f32;
    let mut tail_reachable = false;
    while let Some((x, y, t)) = q.pop_front() {
        count += 1.0;
        let idx = y as usize * w + x as usize;
        if idx == tail_idx && t >= vacate[tail_idx] {
            tail_reachable = true;
        }
        for (dx, dy) in [(0i16, -1i16), (0, 1), (-1, 0), (1, 0)] {
            let nx = x as i16 + dx;
            let ny = y as i16 + dy;
            if nx < 0 || ny < 0 || nx >= game.width as i16 || ny >= game.height as i16 {
                continue;
            }
            let (nx, ny) = (nx as u16, ny as u16);
            let nidx = ny as usize * w + nx as usize;
            if best_t[nidx] != i32::MAX {
                continue;
            }
            let nt = t + 1;
            let free_now = matches!(
                game.grid[ny as usize][nx as usize],
                CellType::Empty | CellType::Food | CellType::Hole | CellType::PowerUp
            ) && !game.bombs.iter().any(|b| b.x == nx && b.y == ny);
            let frees_in_time = vacate[nidx] <= nt;
            if free_now || frees_in_time {
                best_t[nidx] = nt;
                q.push_back((nx, ny, nt));
            }
        }
    }
    (count, tail_reachable)
}

fn step_cell(game: &WormGame, from: (u16, u16), d: Direction) -> (u16, u16) {
    let (dx, dy) = d.as_delta();
    let nx = (from.0 as i16 + dx).max(0).min((game.width - 1) as i16) as u16;
    let ny = (from.1 as i16 + dy).max(0).min((game.height - 1) as i16) as u16;
    (nx, ny)
}

fn escape_floor(game: &WormGame, who: usize) -> f32 {
    let c = &game.cycles[who];
    (c.positions.len() as f32 + c.pending_growth as f32) * 3.0 + 8.0
}

const TRACE_FRAMES: usize = 220;

// ---------------------------------------------------------------- run/metrics

#[derive(Default)]
struct Agg {
    games: u32,
    cpu_wins: u32,
    player_wins: u32,
    draws: u32,
    frames: u64,
    cpu_food: u64,
    player_food: u64,
    frames_near_wall: u64,
    frames_mid: u64,
    corner_frames: u64,
    max_corner_dwell: u32,
    corner_dwell_ge_20: u32,
    len_sum: f64,
    len_end_sum: f64,
    len_max: usize,
    reasons: HashMap<&'static str, u64>,
    deaths: Vec<DeathRec>,
    frames_open_below_floor: u64,
    frames_open_below_floor_but_tail_safe: u64,
    dist_sum: f64,
    frames_within_10: u64,
    frames_within_20: u64,
    frames_tail_unsafe: u64,
    // per-game series for "length over time"
    per_game_end_len: Vec<usize>,
    per_game_cpu_food: Vec<u32>,
    per_game_frames: Vec<u32>,
    dwells: Vec<(u32, u32, u32, &'static str)>, // (game, start_frame, len, reason at start)
}

struct DeathRec {
    game: u32,
    frame: u32,
    cause: Option<DeathCause>,
    reason: Option<&'static str>,
    len: usize,
    read: f32,
    trail: Vec<FrameSnap>,
}

#[derive(Clone)]
struct FrameSnap {
    frame: u32,
    len: usize,
    reason: &'static str,
    open_chosen: f32,
    escape_floor: f32,
    tail_reach_chosen: f32,
    tail_ok_chosen: bool,
    legal: usize,
    any_tail_safe: bool,
    wall_d: i32,
    head: (u16, u16),
    phead: (u16, u16),
    dist_player: i32,
    decided: bool,
}

fn run_sz(games: u32, seed: u64, warm: bool, forage: bool, keep_deaths: bool, w: u16, h: u16) -> Agg {
    let mut a = Agg::default();
    let mut game = WormGame::with_size_seed(w, h, seed);
    let mut rng = Rng(seed ^ 0xA5A5_1234);

    for g in 0..games {
        if g > 0 {
            game.restart();
            if !warm {
                game.cpu_brain = worm::CpuBrain::new();
                game.refresh_read_rate();
            }
        }
        let food_before = game.food_eaten_by;
        let mut frames = 0u32;
        let mut dwell = 0u32;
        let mut last_decision_frame: Option<u32> = None;
        let mut dwell_reason: &'static str = "?";
        let mut dwell_start_reason_pending = false;
        let mut ring: VecDeque<FrameSnap> = VecDeque::with_capacity(TRACE_FRAMES + 2);

        while !game.game_over && frames < 4000 {
            let d = if forage {
                forager(&game, &mut rng)
            } else {
                habitual(&game, &mut rng)
            };
            game.change_direction(d);

            let head_pre = game.cycles[1].head;
            let legal_pre = worm::legal_directions(&game, &game.cycles[1]);
            let floor = escape_floor(&game, 1);
            let mut any_tail_safe = false;
            if keep_deaths {
                for &ld in &legal_pre {
                    let cell = step_cell(&game, head_pre, ld);
                    if tail_aware_reach(&game, 1, cell).1 {
                        any_tail_safe = true;
                        break;
                    }
                }
            }

            game.update();
            frames += 1;

            let (cx, cy) = game.cycles[1].head;
            let len = game.cycles[1].positions.len();
            a.frames += 1;
            a.len_sum += len as f64;
            if len > a.len_max {
                a.len_max = len;
            }

            let wd = wall_dist(&game, cx, cy);
            if wd <= 3 {
                a.frames_near_wall += 1;
            } else {
                a.frames_mid += 1;
            }
            if in_corner_box(&game, cx, cy) {
                a.corner_frames += 1;
                if dwell == 0 { dwell_start_reason_pending = true; }
                dwell += 1;
                if dwell > a.max_corner_dwell {
                    a.max_corner_dwell = dwell;
                }
            } else {
                if dwell >= 20 {
                    a.corner_dwell_ge_20 += 1;
                    a.dwells.push((g, game.frame_count - dwell, dwell, dwell_reason));
                }
                dwell = 0;
            }

            let dec = game.round_last_cpu_decision.as_ref();
            let reason = dec.map(|d| d.reason.as_str()).unwrap_or("(none)");
            let decided_this_frame = dec.map(|d| d.frame) != last_decision_frame;
            last_decision_frame = dec.map(|d| d.frame);
            if decided_this_frame {
                *a.reasons.entry(reason).or_insert(0) += 1;
            }
            if dwell_start_reason_pending {
                dwell_reason = reason;
                dwell_start_reason_pending = false;
            }

            let (pxh, pyh) = game.cycles[0].head;
            let dist_player = (cx as i32 - pxh as i32).abs() + (cy as i32 - pyh as i32).abs();
            a.dist_sum += dist_player as f64;
            if dist_player <= 10 { a.frames_within_10 += 1; }
            if dist_player <= 20 { a.frames_within_20 += 1; }
            // Post-move: from where we now stand, does ANY legal move keep the
            // tail reachable? (Calling tail_aware_reach on the head cell itself
            // is meaningless — the head is occupied by us.)
            let legal_post = worm::legal_directions(&game, &game.cycles[1]);
            let mut tr = 0.0f32;
            let mut tok = false;
            for &ld in &legal_post {
                let c = step_cell(&game, (cx, cy), ld);
                let (rr, t) = tail_aware_reach(&game, 1, c);
                if rr > tr { tr = rr; }
                if t { tok = true; }
            }
            if !tok { a.frames_tail_unsafe += 1; }
            let open_now = worm::count_open_space(&game, cx, cy);
            if open_now < floor {
                a.frames_open_below_floor += 1;
                if tok {
                    a.frames_open_below_floor_but_tail_safe += 1;
                }
            }

            if keep_deaths {
                ring.push_back(FrameSnap {
                    frame: game.frame_count,
                    len,
                    reason,
                    open_chosen: open_now,
                    escape_floor: floor,
                    tail_reach_chosen: tr,
                    tail_ok_chosen: tok,
                    legal: legal_pre.len(),
                    any_tail_safe,
                    wall_d: wd,
                    head: (cx, cy),
                    phead: (pxh, pyh),
                    dist_player,
                    decided: decided_this_frame,
                });
                if ring.len() > TRACE_FRAMES {
                    ring.pop_front();
                }
            }
        }

        a.games += 1;
        let cf = game.food_eaten_by[1] - food_before[1];
        let pf = game.food_eaten_by[0] - food_before[0];
        a.cpu_food += cf as u64;
        a.player_food += pf as u64;
        a.per_game_cpu_food.push(cf);
        a.per_game_frames.push(frames);
        a.len_end_sum += game.cycles[1].positions.len() as f64;
        a.per_game_end_len.push(game.cycles[1].positions.len());
        if dwell >= 20 {
            a.corner_dwell_ge_20 += 1;
            a.dwells.push((g, game.frame_count - dwell, dwell, dwell_reason));
        }
        match game.winner {
            Some(1) => a.cpu_wins += 1,
            Some(0) => {
                a.player_wins += 1;
                a.deaths.push(DeathRec {
                    game: g,
                    frame: game.frame_count,
                    cause: game.death_cause,
                    reason: game
                        .round_last_cpu_decision
                        .as_ref()
                        .map(|d| d.reason.as_str()),
                    len: game.cycles[1].positions.len(),
                    read: game.read_rate,
                    trail: ring.iter().cloned().collect(),
                });
            }
            _ => a.draws += 1,
        }
    }
    a
}

fn run(games: u32, seed: u64, warm: bool, forage: bool, keep_deaths: bool) -> Agg {
    run_sz(games, seed, warm, forage, keep_deaths, 120, 38)
}

fn report(label: &str, a: &Agg, show_trails: bool) {
    let f = a.frames.max(1) as f64;
    let total_food = (a.cpu_food + a.player_food).max(1) as f64;
    println!("\n===== {label} =====");
    println!(
        "games {}  cpu {} player {} draw {}  win {:.0}%   frames/game {:.0}",
        a.games,
        a.cpu_wins,
        a.player_wins,
        a.draws,
        a.cpu_wins as f32 / a.games.max(1) as f32 * 100.0,
        f / a.games.max(1) as f64
    );
    println!(
        "food: cpu {} player {}  -> CPU SHARE {:.1}%   ({:.1} morsels/game)",
        a.cpu_food,
        a.player_food,
        a.cpu_food as f64 / total_food * 100.0,
        a.cpu_food as f64 / a.games.max(1) as f64
    );
    println!(
        "cpu length: mean-over-frames {:.1}  mean-at-end {:.1}  max {}",
        a.len_sum / f,
        a.len_end_sum / a.games.max(1) as f64,
        a.len_max
    );
    println!(
        "position: <=3 cells from a wall {:.1}%   mid-arena {:.1}%",
        a.frames_near_wall as f64 / f * 100.0,
        a.frames_mid as f64 / f * 100.0
    );
    println!(
        "engagement: mean CPU-player distance {:.1}   within 10 cells {:.1}% of frames   within 20 {:.1}%",
        a.dist_sum / f,
        a.frames_within_10 as f64 / f * 100.0,
        a.frames_within_20 as f64 / f * 100.0
    );
    println!(
        "frames with NO legal move that keeps the CPU's own tail reachable: {:.2}%",
        a.frames_tail_unsafe as f64 / f * 100.0
    );
    println!(
        "corner 6x6 box: {:.1}% of frames   longest dwell {} frames   dwell episodes >=20f: {}",
        a.corner_frames as f64 / f * 100.0,
        a.max_corner_dwell,
        a.corner_dwell_ge_20
    );
    println!(
        "count_open_space < escape floor on {:.2}% of frames; of those tail-aware says SAFE: {:.1}%",
        a.frames_open_below_floor as f64 / f * 100.0,
        if a.frames_open_below_floor == 0 {
            0.0
        } else {
            a.frames_open_below_floor_but_tail_safe as f64 / a.frames_open_below_floor as f64 * 100.0
        }
    );
    // length trajectory in blocks of 10 games
    let n = a.per_game_end_len.len();
    if n >= 10 {
        print!("end-length by game block: ");
        for chunk in a.per_game_end_len.chunks(10) {
            let m: f64 = chunk.iter().map(|&x| x as f64).sum::<f64>() / chunk.len() as f64;
            print!("{:.1} ", m);
        }
        println!();
        print!("cpu food by game block:   ");
        for chunk in a.per_game_cpu_food.chunks(10) {
            let m: f64 = chunk.iter().map(|&x| x as f64).sum::<f64>() / chunk.len() as f64;
            print!("{:.1} ", m);
        }
        println!();
    }
    if !a.dwells.is_empty() {
        let mut d = a.dwells.clone();
        d.sort_by(|x, y| y.2.cmp(&x.2));
        println!("corner-dwell episodes >=20 frames (longest first):");
        for (g, f0, n, r) in d.iter().take(8) {
            println!("   game {:>2}  frames {}..{}  ({} frames)  entered while: {}", g, f0, f0 + n, n, r);
        }
    }
    let mut rs: Vec<_> = a.reasons.iter().collect();
    rs.sort_by(|x, y| y.1.cmp(x.1));
    let dtot: u64 = a.reasons.values().sum();
    println!("decision reasons (of {} actual CPU decisions):", dtot);
    for (k, v) in rs {
        println!("   {:>7} ({:>5.2}%)  {}", v, *v as f64 / dtot.max(1) as f64 * 100.0, k);
    }
    if !a.deaths.is_empty() {
        println!("CPU deaths ({}):", a.deaths.len());
        for d in &a.deaths {
            println!(
                "  game {:>2} frame {:>4} cause {:?} reason {:?} len {} read {:.2}",
                d.game, d.frame, d.cause, d.reason, d.len, d.read
            );
            let ponr = d.trail.iter().rev().find(|s| s.any_tail_safe).map(|s| s.frame);
            match ponr {
                Some(fr) => println!(
                    "     point of no return (last frame with a tail-safe alternative): f{}  = {} frames before death",
                    fr, d.frame as i64 - fr as i64
                ),
                None => println!("     point of no return: BEFORE this {}-frame window", d.trail.len()),
            }
            if show_trails {
                for s in &d.trail {
                    println!(
                        "      f{:<5} {}cpu({:>3},{:>3}) you({:>3},{:>3}) len{:<4} legal{} wallD{:<3} dP{:<3} {:<28} open {:>6.0}/floor {:>6.0}  tailreach {:>6.0} tail_ok {}  any_alt_tail_safe {}",
                        s.frame,
                        if s.decided { "*" } else { " " },
                        s.head.0, s.head.1, s.phead.0, s.phead.1,
                        s.len,
                        s.legal,
                        s.wall_d,
                        s.dist_player,
                        s.reason,
                        s.open_chosen,
                        s.escape_floor,
                        s.tail_reach_chosen,
                        if s.tail_ok_chosen { "Y" } else { "n" },
                        if s.any_tail_safe { "Y" } else { "n" }
                    );
                }
            }
        }
    }
}

// ------------------------------------------------- controlled tail-aware A/B

struct AbResult {
    cpu_deaths: u32,
    own_trail: u32,
    wall_nolegal: u32,
    len_max: usize,
    len_mean: f64,
    causes: HashMap<String, u32>,
}

/// Greedy BFS step toward the nearest collectible (probe-local, so both A/B
/// arms grow at the same rate and the only variable is the safety metric).
fn nearest_food_step(game: &WormGame, from: (u16, u16), legal: &[Direction]) -> Option<Direction> {
    if game.food_items.is_empty() && game.powerups.is_empty() {
        return None;
    }
    let w = game.width as usize;
    let mut seen = vec![false; w * game.height as usize];
    let mut q: VecDeque<(u16, u16, Direction)> = VecDeque::new();
    for &d in legal {
        let c = step_cell(game, from, d);
        if game.passable(c.0, c.1) {
            let i = c.1 as usize * w + c.0 as usize;
            if !seen[i] {
                seen[i] = true;
                q.push_back((c.0, c.1, d));
            }
        }
    }
    while let Some((x, y, sd)) = q.pop_front() {
        if matches!(game.grid[y as usize][x as usize], CellType::Food | CellType::PowerUp) {
            return Some(sd);
        }
        for (dx, dy) in [(0i16, -1i16), (0, 1), (-1, 0), (1, 0)] {
            let nx = x as i16 + dx;
            let ny = y as i16 + dy;
            if nx < 0 || ny < 0 || nx >= game.width as i16 || ny >= game.height as i16 { continue; }
            let (nx, ny) = (nx as u16, ny as u16);
            let i = ny as usize * w + nx as usize;
            if !seen[i] && matches!(
                game.grid[ny as usize][nx as usize],
                CellType::Empty | CellType::Food | CellType::Hole | CellType::PowerUp
            ) {
                seen[i] = true;
                q.push_back((nx, ny, sd));
            }
        }
    }
    None
}

/// Minimal survival policy driving cycle 1 externally (cpu_autopilot = false),
/// isolating exactly one variable: the reachability metric behind the floor.
/// Arm A = `count_open_space` (what cpu_decide uses today).
/// Arm B = tail-aware timed reachability.
fn ab_arm(games: u32, seed: u64, tail_aware: bool) -> AbResult {
    let mut game = WormGame::with_size_seed(120, 38, seed);
    game.cpu_autopilot = false;
    let mut rng = Rng(seed ^ 0xA5A5_1234);
    let mut r = AbResult {
        cpu_deaths: 0,
        own_trail: 0,
        wall_nolegal: 0,
        len_max: 0,
        len_mean: 0.0,
        causes: HashMap::new(),
    };
    let mut len_sum = 0.0f64;
    let mut nframes = 0u64;

    for g in 0..games {
        if g > 0 {
            game.restart();
            game.cpu_autopilot = false;
        }
        let mut frames = 0;
        while !game.game_over && frames < 4000 {
            let pd = habitual(&game, &mut rng);
            game.change_direction(pd);

            let legal = worm::legal_directions(&game, &game.cycles[1]);
            if !legal.is_empty() {
                let head = game.cycles[1].head;
                let wall_dir = worm::wall_follow_decide(&game, &game.cycles[1]);
                let floor = escape_floor(&game, 1);
                let ok = |d: Direction| -> bool {
                    let c = step_cell(&game, head, d);
                    if tail_aware {
                        tail_aware_reach(&game, 1, c).1
                    } else {
                        worm::count_open_space(&game, c.0, c.1) >= floor
                    }
                };
                // Food drive first — identical in both arms — gated by the
                // arm's own safety metric. This is what makes the CPU grow, and
                // growth is the precondition for the coil deaths under test.
                let mut chosen = wall_dir;
                if let Some(fd) = nearest_food_step(&game, head, &legal) {
                    if ok(fd) {
                        chosen = fd;
                    }
                }
                if !legal.contains(&chosen) || !ok(chosen) {
                    let mut best = None;
                    let mut best_score = f32::NEG_INFINITY;
                    for &d in &legal {
                        let c = step_cell(&game, head, d);
                        let score = if tail_aware {
                            let (rr, t) = tail_aware_reach(&game, 1, c);
                            rr + if t { 100000.0 } else { 0.0 }
                        } else {
                            worm::count_open_space(&game, c.0, c.1)
                        };
                        if score > best_score {
                            best_score = score;
                            best = Some(d);
                        }
                    }
                    chosen = best.unwrap_or(legal[0]);
                }
                game.cycles[1].change_direction(chosen);
            }
            game.update();
            frames += 1;
            let l = game.cycles[1].positions.len();
            len_sum += l as f64;
            nframes += 1;
            if l > r.len_max {
                r.len_max = l;
            }
        }
        if game.winner == Some(0) {
            r.cpu_deaths += 1;
            if let Some(c) = game.death_cause {
                *r.causes.entry(format!("{:?}", c)).or_insert(0) += 1;
                if c == DeathCause::OwnTrail {
                    r.own_trail += 1;
                }
                if c == DeathCause::Wall {
                    r.wall_nolegal += 1;
                }
            }
        }
    }
    r.len_mean = len_sum / nframes.max(1) as f64;
    r
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(|s| s.as_str()).unwrap_or("engage");

    match mode {
        "engage" => {
            let warm = run(40, 4242, true, false, true);
            report("WARM 40g vs habitual (seed 4242)", &warm, true);
            let cold = run(40, 4242, false, false, true);
            report("COLD 40g vs habitual (seed 4242)", &cold, false);
        }
        "forage" => {
            for s in [4242u64, 20260805, 77] {
                let a = run(40, s, true, true, false);
                report(&format!("WARM 40g vs FORAGER seed {}", s), &a, false);
            }
        }
        "seeds" => {
            for s in [4242u64, 20260805, 77, 31337] {
                let a = run(40, s, true, false, true);
                report(&format!("WARM 40g seed {}", s), &a, true);
            }
        }
        "ab" | "abmore" => {
            let seeds: &[u64] = if mode == "abmore" {
                &[11, 22, 33, 44, 55, 66, 88, 99, 101, 202]
            } else {
                &[4242u64, 20260805, 77]
            };
            let mut ta = (0u32, 0u32, 0u32);
            let mut tb = (0u32, 0u32, 0u32);
            for &seed in seeds {
                let a = ab_arm(40, seed, false);
                let b = ab_arm(40, seed, true);
                println!(
                    "\n=== A/B seed {} (40 games, external minimal survival policy) ===",
                    seed
                );
                println!(
                    "  A count_open_space : cpu deaths {:>2}  OwnTrail {:>2}  Wall {:>2}  meanlen {:>5.1} maxlen {:>3}  {:?}",
                    a.cpu_deaths, a.own_trail, a.wall_nolegal, a.len_mean, a.len_max, a.causes
                );
                println!(
                    "  B tail-aware       : cpu deaths {:>2}  OwnTrail {:>2}  Wall {:>2}  meanlen {:>5.1} maxlen {:>3}  {:?}",
                    b.cpu_deaths, b.own_trail, b.wall_nolegal, b.len_mean, b.len_max, b.causes
                );
                ta = (ta.0 + a.cpu_deaths, ta.1 + a.own_trail, ta.2 + a.wall_nolegal);
                tb = (tb.0 + b.cpu_deaths, tb.1 + b.own_trail, tb.2 + b.wall_nolegal);
            }
            println!("\nTOTAL over {} games/arm:", seeds.len() * 40);
            println!("  A count_open_space : deaths {}  OwnTrail {}  Wall {}", ta.0, ta.1, ta.2);
            println!("  B tail-aware       : deaths {}  OwnTrail {}  Wall {}", tb.0, tb.1, tb.2);
        }
        "browser" => {
            // The board the browser actually builds on a laptop: ~55x40.
            for s in [4242u64, 20260805, 77] {
                let a = run_sz(40, s, true, false, true, 55, 40);
                report(&format!("BROWSER 55x40 · WARM 40g vs habitual · seed {}", s), &a, false);
                let b = run_sz(40, s, true, true, true, 55, 40);
                report(&format!("BROWSER 55x40 · WARM 40g vs FORAGER · seed {}", s), &b, false);
            }
        }
        "btrace" => {
            let a = run_sz(40, 4242, true, false, true, 55, 40);
            report("BROWSER 55x40 trace seed 4242", &a, true);
        }
        _ => eprintln!("modes: engage | forage | seeds | ab | browser | btrace"),
    }
}
