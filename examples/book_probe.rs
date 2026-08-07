//! Turn-book probe: does the derived gate open for a player whose hazard
//! genuinely spikes? A metronome alternator (turn every 5th frame, strictly
//! alternating sides) is the strongest learnable slalom — its due-cell
//! hazard approaches 1.0 and its side is fully determined. The gate MUST
//! fire here, and the published forecast must start calling the swerves.
use worm::{Direction, WormGame};

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

fn main() {
    let mut game = WormGame::with_size_seed(120, 38, 20260806);
    let mut gap = 0u32;
    let mut last_left = false;
    let mut book_frames = 0u32;
    let mut book_hits = 0u32;
    let mut vol_frames = 0u32;
    let mut vol_hits = 0u32;
    for g in 0..12 {
        if g > 0 {
            game.restart();
        }
        let mut frames = 0;
        while !game.game_over && frames < 4000 {
            let cur = game.cycles[0].direction;
            let legal = worm::legal_options_from(&game, 0, cur);
            let (l, r) = (left_of(cur), right_of(cur));
            let forecast_book = game
                .cpu_telemetry
                .next_forecast
                .map(|f| (f.book == 1, f.predicted))
                .unwrap_or((false, None));
            let dir = if gap >= 4
                && legal.contains(&cur)
                && legal.contains(&l)
                && legal.contains(&r)
            {
                gap = 0;
                last_left = !last_left;
                if last_left { l } else { r }
            } else if legal.contains(&cur) {
                gap += 1;
                cur
            } else {
                gap = 0;
                *[l, r]
                    .iter()
                    .find(|d| legal.contains(d))
                    .unwrap_or(&cur)
            };
            let voluntary_lateral = legal.contains(&cur) && dir != cur;
            game.change_direction(dir);
            game.update();
            frames += 1;
            if voluntary_lateral {
                vol_frames += 1;
                if forecast_book.1 == Some(dir) {
                    vol_hits += 1;
                }
                if forecast_book.0 {
                    book_frames += 1;
                    if forecast_book.1 == Some(dir) {
                        book_hits += 1;
                    }
                }
            }
        }
    }
    let b = &game.cpu_brain.class_books;
    let mut max_h = 0.0f32;
    for cell in 0..worm::cpu_ai::HAZARD_CELLS {
        if b.hz_total[cell] >= 10.0 {
            max_h = max_h.max(b.hazard(cell));
        }
    }
    println!(
        "events={} aT={:.2} aS={:.2} gate_open={} max_h={:.2}",
        b.turn_events,
        b.a_turn(),
        b.a_straight(),
        b.gate_open,
        max_h
    );
    println!(
        "voluntary swerves: {}  published hit {:.0}%  book-published {} (hit {:.0}%)",
        vol_frames,
        100.0 * vol_hits as f32 / vol_frames.max(1) as f32,
        book_frames,
        100.0 * book_hits as f32 / book_frames.max(1) as f32,
    );
}
