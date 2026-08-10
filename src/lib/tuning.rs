//! Runtime-tunable CPU temperament knobs — the substrate for worm-native
//! Darwin evolution (ADR-015).
//!
//! Every value defaults to the named constant in `cpu_ai.rs` that has always
//! governed play; a `WORM_TUNE_*` environment variable overrides it AT
//! PROCESS START (read once, `OnceLock`). This exists so an evolution driver
//! can run the seeded gauntlet across hundreds of candidate temperaments
//! WITHOUT recompiling — same binary, different knobs, deterministic seeds.
//!
//! Deliberately NOT a config file: a file invites hand-drift, an env var
//! scopes to one process and dies with it. The committed defaults are the
//! champion; a knob change becomes real only by being promoted into the
//! constant it shadows, through the gauntlet, with receipts (ADR-009/010).

use std::sync::OnceLock;

use crate::cpu_ai;

#[derive(Debug, Clone)]
pub struct Tuning {
    /// Escape floor: reachable cells required per unit of body length.
    pub escape_multiple: f32,
    /// Escape floor: flat margin in cells.
    pub escape_margin: f32,
    /// Fraction of the escape floor a fully-read hunt may spend.
    pub hunt_spend: f32,
    /// Read-rate exponent shaping how fast that spend unlocks.
    pub hunt_curve: f32,
    /// Confidence gate for the corner intercept layer.
    pub corner_gate: f32,
    /// Confidence gate for the direct intercept layer.
    pub direct_gate: f32,
    /// Fixed-share fast horizon: learning rate.
    pub eta_fast: f32,
    /// Fixed-share slow horizon: learning rate.
    pub eta_slow: f32,
    /// Fixed-share fast horizon: share (recovery) rate.
    pub share_fast: f32,
    /// Fixed-share slow horizon: share rate.
    pub share_slow: f32,
    /// Warm-corpus multiplier bonus for the k-NN model.
    pub knn_bonus: f32,
    /// THE BEATABLE OPENING (ADR-018). Survival-floor multiplier at zero
    /// read — the unread CPU keeps only this fraction of its escape floor,
    /// playing bold enough to make killable mistakes. Reading the player
    /// linearly restores full discipline.
    pub discipline_floor: f32,
    /// Extra hunt-margin spend at zero read (reckless dives), decaying to
    /// zero as the read grows and the champion hunt economics take over.
    pub bold_spend: f32,
    /// Fraction of raw forecast confidence the hunt gates may use at zero
    /// read (extrapolation-chasing), decaying as the real read takes over.
    pub bold_drive: f32,
    /// Decision latency at zero read, in frames: the unread CPU re-decides
    /// only every Nth frame — casual-human attention, not tick-perfect
    /// play — reaching every-frame wits as the read grows. THE lever that
    /// makes the opening genuinely losable: held headings meet walls.
    pub open_latency: f32,
    /// ADR-025 stage 3: step-1 laser lead (0 = off, 1 = on). Pure
    /// geometry — fire when the player's straight-ahead next cell is on
    /// the beam; the ADR-023 reconciliation makes that entry lethal.
    pub laser_lead: f32,
    /// Attribution-arm switches (ADR-020 stage 2.1, codex D10): 1.0 = on.
    /// book_bend: the turn book may bend the 5-frame player projection.
    /// book_spend: the book's earned evidence may feed difficulty.
    /// Kept as tuning knobs so the promotion arms (straight / bent-only /
    /// bent+spend) run without rebuilds, and Darwin can see them.
    pub book_bend: f32,
    pub book_spend: f32,
}

fn env_f32(name: &str, default: f32) -> f32 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse::<f32>().ok())
        .filter(|v| v.is_finite())
        .unwrap_or(default)
}

pub fn tuning() -> &'static Tuning {
    static T: OnceLock<Tuning> = OnceLock::new();
    T.get_or_init(|| Tuning {
        escape_multiple: env_f32("WORM_TUNE_ESCAPE_MULTIPLE", cpu_ai::ESCAPE_LENGTH_MULTIPLE),
        escape_margin: env_f32("WORM_TUNE_ESCAPE_MARGIN", cpu_ai::ESCAPE_MARGIN_CELLS),
        hunt_spend: env_f32("WORM_TUNE_HUNT_SPEND", cpu_ai::HUNT_MARGIN_SPEND),
        hunt_curve: env_f32("WORM_TUNE_HUNT_CURVE", cpu_ai::HUNT_MARGIN_CURVE),
        corner_gate: env_f32("WORM_TUNE_CORNER_GATE", 0.35),
        direct_gate: env_f32("WORM_TUNE_DIRECT_GATE", 0.45),
        eta_fast: env_f32("WORM_TUNE_ETA_FAST", 1.2),
        eta_slow: env_f32("WORM_TUNE_ETA_SLOW", 0.3),
        share_fast: env_f32("WORM_TUNE_SHARE_FAST", 0.08),
        share_slow: env_f32("WORM_TUNE_SHARE_SLOW", 0.01),
        knn_bonus: env_f32("WORM_TUNE_KNN_BONUS", cpu_ai::KNN_SCORE_BONUS),
        discipline_floor: env_f32("WORM_TUNE_DISCIPLINE_FLOOR", 0.35),
        bold_spend: env_f32("WORM_TUNE_BOLD_SPEND", 0.40),
        bold_drive: env_f32("WORM_TUNE_BOLD_DRIVE", 1.0),
        open_latency: env_f32("WORM_TUNE_OPEN_LATENCY", 10.0),
        laser_lead: env_f32("WORM_TUNE_LASER_LEAD", 1.0),
        // Default ON — EARNED by measurement, in two steps (ADR-014
        // discipline): with the original 64-cell aligned-boolean hazard
        // the bend measured WORSE on the authority-active subset (8075
        // vs 7989 over 1,134 windows) and shipped off; with the 96-cell
        // food-side hazard and the learned toward-food split (stage 2.2)
        // it WINS: 15,247 vs 15,463 over 2,075 authority-active windows
        // on the 63-round owner corpus. Darwin can still veto it.
        book_bend: env_f32("WORM_TUNE_BOOK_BEND", 1.0),
        book_spend: env_f32("WORM_TUNE_BOOK_SPEND", 1.0),
    })
}
