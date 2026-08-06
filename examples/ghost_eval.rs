//! Ghost evaluator (ADR-016/017): score the CURRENT brain against a REAL
//! player's recorded rounds.
//!
//!     cargo run --release --example ghost_eval -- <export.json | rounds.jsonl>
//!
//! Accepts the browser's EXPORT MY ROUNDS download, a per-player file from
//! scripts/collect_to_export.py, or a raw collected JSONL file. Rounds are
//! replayed OLDEST-FIRST with one persistent brain and `shadow_learning`
//! on: the real pipeline — episodes, ensemble, sealed forecasts, the
//! McNemar-gated read — watches the recorded human exactly as it would
//! have live, while never steering. Ghost v2 logs only (the ordered event
//! stream); v1 logs are skipped loudly, not guessed at.
//! Candidates evaluate identically via WORM_TUNE_* env knobs.

use worm::WormGame;

struct Round {
    ended_at: u64,
    id: String,
    frames: u32,
    seed: u64,
    w: u16,
    h: u16,
    events: Vec<(u32, u8, u8)>,
}

fn parse_round(rec: &serde_json::Value) -> Result<Round, String> {
    let replay = rec.get("replay").ok_or("no replay")?;
    if replay.get("v").and_then(|v| v.as_u64()) != Some(2) {
        return Err("ghost v1 (pre event-stream) — skipped".into());
    }
    let seed = replay
        .get("seed")
        .and_then(|s| s.as_str())
        .and_then(|s| s.parse::<u64>().ok())
        .ok_or("bad seed (must be a decimal string)")?;
    let w = replay.get("w").and_then(|v| v.as_u64()).ok_or("bad w")? as u16;
    let h = replay.get("h").and_then(|v| v.as_u64()).ok_or("bad h")? as u16;
    let frames = replay
        .get("frames")
        .and_then(|v| v.as_u64())
        .ok_or("bad frames")? as u32;
    if !(10..=400).contains(&w) || !(10..=400).contains(&h) || frames > 100_000 {
        return Err("dimensions/frames out of range".into());
    }
    let mut events = Vec::new();
    let mut last_frame = 0u32;
    for ev in replay
        .get("ev")
        .and_then(|v| v.as_array())
        .ok_or("bad ev")?
    {
        let t = ev.as_array().ok_or("event not an array")?;
        if t.len() != 3 {
            return Err("event arity != 3".into());
        }
        let f = t[0].as_u64().ok_or("bad event frame")? as u32;
        let k = t[1].as_u64().ok_or("bad event kind")? as u8;
        let v = t[2].as_u64().ok_or("bad event value")? as u8;
        if k > 3 || v > 3 || f > frames + 1 || f < last_frame {
            return Err("event out of domain or non-monotonic".into());
        }
        last_frame = f;
        events.push((f, k, v));
    }
    Ok(Round {
        ended_at: rec.get("endedAt").and_then(|v| v.as_u64()).unwrap_or(0),
        id: rec
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("?")
            .to_string(),
        frames,
        seed,
        w,
        h,
        events,
    })
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: ghost_eval <export.json | rounds.jsonl>");
    let text = std::fs::read_to_string(&path).expect("read input file");

    // Export wrapper {rounds:[...]} or raw JSONL — both typed, no scraping.
    let mut records: Vec<serde_json::Value> = Vec::new();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
        if let Some(rounds) = v.get("rounds").and_then(|r| r.as_array()) {
            records = rounds.clone();
        } else {
            records = vec![v];
        }
    } else {
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            if let Ok(v) = serde_json::from_str(line) {
                records.push(v);
            }
        }
    }

    let mut rounds = Vec::new();
    let mut skipped = 0;
    for rec in &records {
        match parse_round(rec) {
            Ok(r) => rounds.push(r),
            Err(e) => {
                skipped += 1;
                eprintln!("skipped round ({e})");
            }
        }
    }
    // Oldest first, stable: (endedAt, id).
    rounds.sort_by(|a, b| (a.ended_at, &a.id).cmp(&(b.ended_at, &b.id)));
    if rounds.is_empty() {
        eprintln!("no usable ghost-v2 rounds in {path} ({skipped} skipped)");
        std::process::exit(1);
    }
    println!(
        "{} recorded round(s) ({} skipped) — replaying chronologically…\n",
        rounds.len(),
        skipped
    );

    // ONE persistent brain across all rounds, exactly like a live session.
    let mut game = WormGame::with_size_seed(55, 40, 1);
    game.cpu_autopilot = false;
    game.shadow_learning = true;

    for (i, r) in rounds.iter().enumerate() {
        game.start_recorded_round(r.seed, r.w, r.h, r.events.clone());
        while !game.game_over && game.frame_count < r.frames {
            game.update();
        }
        let complete = game.frame_count == r.frames
            && game
                .script
                .as_ref()
                .map(|s| s.cursor == s.events.len())
                .unwrap_or(false);
        let rr = &game.round_read;
        println!(
            "round {:>3}: {:>5}/{:<5} frames{} · read {:>5.1}% vs your-usual {:>5.1}% · lift {:>+5.1}% · cum {:>+5.1}%",
            i + 1,
            game.frame_count,
            r.frames,
            if complete { "" } else { " (INCOMPLETE — replay diverged?)" },
            rr.rate() * 100.0,
            rr.base_rate() * 100.0,
            rr.lift() * 100.0,
            game.cpu_brain.lifetime_read.lift() * 100.0,
        );
    }

    let life = &game.cpu_brain.lifetime_read;
    println!(
        "\n==== THE REAL-HUMAN READ ====\nlifetime lift {:+.1}% over your own base rate · {} scored frames · {}",
        life.lift() * 100.0,
        life.samples,
        if life.is_significant() {
            "statistically significant (McNemar)"
        } else {
            "not yet significant — more rounds needed"
        }
    );
}
