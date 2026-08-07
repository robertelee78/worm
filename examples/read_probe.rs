//! Scratch probe: warm-vs-cold outcome forensics against the domination
//! persona, at honest (earned) read. Prints per-game outcomes and death
//! causes so a warm handicap can be attributed, not guessed at.
use worm::{Direction, WormGame};

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

fn run(warm: bool, games: u32, seed: u64) {
    let mut game = WormGame::with_size_seed(120, 38, seed);
    let mut rng = Rng(seed ^ 0xABCD);
    let (mut cw, mut pw, mut dr) = (0, 0, 0);
    for g in 0..games {
        if g > 0 {
            if warm {
                game.restart();
            } else {
                game = WormGame::with_size_seed(120, 38, seed.wrapping_add(g as u64));
            }
        }
        let mut frames = 0;
        while !game.game_over && frames < 4000 {
            let dir = habitual(&game, &mut rng);
            game.change_direction(dir);
            game.update();
            frames += 1;
        }
        let outcome = match game.winner {
            Some(1) => { cw += 1; "CPU-WIN" }
            Some(0) => { pw += 1; "cpu-die" }
            _ => { dr += 1; "draw" }
        };
        let r = &game.cpu_brain.lifetime_read;
        println!(
            "{} g{g:>2} {outcome:<8} f={frames:<5} death={:?}/{:?} earned={:.2} lat n={} h={} ch={:.0}",
            if warm { "WARM" } else { "COLD" },
            game.death_cause,
            game.round_last_cpu_decision.as_ref().map(|d| d.reason),
            r.earned_read(), r.lat_samples, r.lat_hits, r.lat_chance,
        );
    }
    println!("== {} total cpu {} player {} draw {}\n", if warm { "WARM" } else { "COLD" }, cw, pw, dr);
}

fn main() {
    run(false, 30, 20260805);
    run(true, 30, 20260805);
}
