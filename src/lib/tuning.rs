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
    })
}
