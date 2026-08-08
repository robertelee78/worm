//! Versioned browser wire state. This module is deliberately independent of
//! wasm-bindgen so native tests can prove the exact JSON contract shipped to
//! the browser.

use serde::Serialize;

use crate::cpu_ai::{
    ensemble_rank_score, CpuDecisionTrace, ForecastTrace, PlayerProjection, ScoredForecast,
    COLD_START_EPISODES, ENSEMBLE_MODELS, MAX_EPISODES, MODEL_NAMES,
};
use crate::game::PowerUpKind;
use crate::{Direction, WormGame};

pub const STATE_SCHEMA_VERSION: u8 = 2;

const MODEL_DISPLAY_NAMES: [&str; ENSEMBLE_MODELS] = [
    "Streak reader",
    "Pattern hunter",
    "Habit tracker",
    "Rotation guesser",
    "Wall reader · R",
    "Wall reader · L",
    "Deep memory",
    "Food-seeker",
    "Hunter",
    "Arming-up",
    "Food-seeker · weaving",
    "Hunter · weaving",
    "Arming-up · weaving",
    "Rhythm reader",
];

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GameState {
    schema_version: u8,
    w: u16,
    h: u16,
    /// SLIPSTREAM: the player is out in the corridor and world time runs
    /// at half speed (drives the browser's visual effect).
    slipstream: bool,
    frame: u32,
    time: u32,
    over: bool,
    winner: Option<usize>,
    score: u32,
    scores: [u32; 2],
    food_eaten: [u32; 2],
    wins: [u32; 2],
    speed: u32,
    cycles: Vec<CycleState>,
    food: Vec<(u16, u16, u8)>,
    powerups: Vec<(u16, u16, u8)>,
    bolts: Vec<(u16, u16, i16, i16)>,
    bombs: Vec<(u16, u16, u32)>,
    /// World v8 telegraph: flashing decoys only — (x, y, tier). A bomb
    /// below tier 1 stays inside the food list (the disguise); this
    /// channel EXISTS only once the flash would reveal it anyway, so
    /// the browser cannot leak a pre-flash danger zone (ADR-022).
    bomb_flash: Vec<(u16, u16, u8)>,
    particles: Vec<(f32, f32, u8, u8, u8, u32)>,
    /// ADR-023 beam render layer: (cells, age). Age 0 = lethal core,
    /// 1-5 dimming afterimage, 6-20 embers. The cells are the SIM's own
    /// beam cells — the renderer never recomputes geometry.
    beams: Vec<(Vec<(u16, u16)>, u32)>,
    cause: Option<String>,
    brain: BrainState,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CycleState {
    head: [u16; 2],
    dir: u8,
    alive: bool,
    held: Option<u8>,
    color: [u8; 3],
    pos: Vec<[u16; 2]>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BrainState {
    frame: u32,
    scored: Option<ScoredState>,
    decision: Option<DecisionState>,
    last_decision: Option<DecisionState>,
    next_forecast: Option<ForecastState>,
    accuracy: AccuracyScopes,
    /// The honest metric: read rate against the player's OWN base rate.
    read_rate: ReadRateScopes,
    /// Where the earned difficulty actually comes from (ADR-020): the
    /// published forecast's channels, or the turn book's precommitted
    /// side calls. The HUD must never imply "forecast performance" when
    /// the evidence is the book's (codex round 3 note).
    book: BookState,
    /// How well the CPU reads this player, in [0,1], and the HUD tier it buys.
    read_lift: f32,
    difficulty: u32,
    seal: SealState,
    memory: MemoryState,
    habits: [f32; 4],
    models: Vec<ModelState>,
}

/// Prediction seals revealed this round, chained into one verifiable number.
#[derive(Serialize)]
struct SealState {
    /// Hex — a u64 exceeds JS safe-integer range and would silently round if
    /// emitted as a JSON number.
    chain: String,
    frames: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReadRateState {
    hits: u32,
    samples: u32,
    /// Frames where the CPU and the trivial baseline DISAGREED. This, not the
    /// frame count, is the real sample size of "it beat your habits".
    discordant: u32,
    min_discordant: u32,
    ready: bool,
    significant: bool,
    /// null until `ready` — a rate on five disagreements is noise, and a
    /// front-end that forgets that would render it anyway.
    rate: Option<f32>,
    base_rate: Option<f32>,
    lift: Option<f32>,
    p_value: Option<f32>,
    uniform_chance: Option<f32>,
}

impl From<&crate::cpu_ai::ReadRate> for ReadRateState {
    fn from(r: &crate::cpu_ai::ReadRate) -> Self {
        let ready = r.is_ready();
        let g = |v: f32| if ready { Some(v) } else { None };
        Self {
            hits: r.hits,
            samples: r.samples,
            discordant: r.discordant(),
            min_discordant: crate::cpu_ai::READ_RATE_MIN_DISCORDANT,
            ready,
            significant: r.is_significant(),
            rate: g(r.rate()),
            base_rate: g(r.base_rate()),
            lift: g(r.lift()),
            p_value: g(r.p_value()),
            uniform_chance: g(r.uniform_chance()),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BookState {
    /// The book's side accuracy on your genuine two-sided turns (None
    /// until it has events).
    side_accuracy: Option<f32>,
    side_events: u32,
    coverage: f32,
    /// The round-boundary earned read difficulty is spending, and which
    /// family half it came from.
    earned: f32,
    earned_source: &'static str,
    /// The drift alarm (narration only): has this player's swerve
    /// grammar measurably changed within the recent window?
    drift_detected: bool,
    /// The epistemic self-map: of the CPU's situation cells for THIS
    /// player, how many are populated / thin / never seen. Quantity, not
    /// significance — kept apart from `earned` on purpose (conflating
    /// them is the phantom-confidence class).
    map_populated: u32,
    map_thin: u32,
    map_unseen: u32,
    /// THE CUMULATIVE NOTEBOOK (ADR-021): everything durable the CPU
    /// knows about this player, exported so the explain pipeline can
    /// ground its narrative in the whole relationship, not one round.
    rounds_observed: u32,
    drift_z: f32,
    rhythm_events: u32,
    /// P(next voluntary swerve breaks left) from the persisted grammar.
    rhythm_p_left: Option<f32>,
    boxer_aversion: f32,
    /// (tactic name, episodic attempts, kills) — which hunts work on YOU.
    tactics: Vec<(String, u32, u32)>,
    /// (weapon name, fires, lethal) — which bait works on YOU.
    weapons: Vec<(String, u32, u32)>,
    /// (death cause, total, while-chased) — how YOU kill the CPU.
    cpu_losses: Vec<(String, u32, u32)>,
}

#[derive(Serialize)]
struct ReadRateScopes {
    round: ReadRateState,
    lifetime: ReadRateState,
}

#[derive(Serialize)]
struct AccuracyScopes {
    round: AccuracyState,
    lifetime: AccuracyState,
}

#[derive(Serialize)]
struct AccuracyState {
    hits: u32,
    samples: u32,
    rate: f32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MemoryState {
    survival_retained: usize,
    opponent_retained: usize,
    survival_observed: u32,
    opponent_observed: u32,
    capacity: usize,
    warm_samples: usize,
    warm_at: usize,
    ready: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ForecastState {
    target_frame: u32,
    source_key: &'static str,
    source_name: &'static str,
    source_index: usize,
    predicted: Option<u8>,
    confidence: f32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ScoredState {
    target_frame: u32,
    source_key: &'static str,
    source_name: &'static str,
    source_index: usize,
    predicted: u8,
    actual: u8,
    hit: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DecisionState {
    frame: u32,
    heading: u8,
    reason: &'static str,
    forecast: Option<ForecastState>,
    projection: Option<ProjectionState>,
}

#[derive(Serialize)]
struct ProjectionState {
    direction: u8,
    path: Vec<[u16; 2]>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelState {
    key: &'static str,
    name: &'static str,
    predicted: Option<u8>,
    raw_score: f32,
    effective_score: f32,
    hits: u32,
    samples: u32,
}

fn dir_u8(direction: Direction) -> u8 {
    match direction {
        Direction::Up => 0,
        Direction::Down => 1,
        Direction::Left => 2,
        Direction::Right => 3,
    }
}

fn powerup_u8(kind: PowerUpKind) -> u8 {
    match kind {
        PowerUpKind::Laser => 0,
        PowerUpKind::TriShot => 1,
        PowerUpKind::Bomb => 2,
    }
}

impl From<ForecastTrace> for ForecastState {
    fn from(trace: ForecastTrace) -> Self {
        Self {
            target_frame: trace.target_frame,
            source_key: MODEL_NAMES[trace.source],
            source_name: MODEL_DISPLAY_NAMES[trace.source],
            source_index: trace.source,
            predicted: trace.predicted.map(dir_u8),
            confidence: trace.confidence,
        }
    }
}

impl From<ScoredForecast> for ScoredState {
    fn from(scored: ScoredForecast) -> Self {
        let source = scored.forecast.source;
        Self {
            target_frame: scored.forecast.target_frame,
            source_key: MODEL_NAMES[source],
            source_name: MODEL_DISPLAY_NAMES[source],
            source_index: source,
            predicted: dir_u8(scored.forecast.predicted.expect("scored forecast predicts")),
            actual: dir_u8(scored.actual),
            hit: scored.hit,
        }
    }
}

impl From<PlayerProjection> for ProjectionState {
    fn from(projection: PlayerProjection) -> Self {
        Self {
            direction: dir_u8(projection.direction),
            path: projection.path.into_iter().map(|(x, y)| [x, y]).collect(),
        }
    }
}

impl From<CpuDecisionTrace> for DecisionState {
    fn from(trace: CpuDecisionTrace) -> Self {
        Self {
            frame: trace.frame,
            heading: dir_u8(trace.heading),
            reason: trace.reason.as_str(),
            forecast: trace.forecast.map(Into::into),
            projection: trace.projection.map(Into::into),
        }
    }
}

impl GameState {
    fn from_game(game: &WormGame) -> Self {
        let brain = &game.cpu_brain;
        let ensemble = &brain.ensemble;
        let habits = brain.opp_brain.prior_distribution();
        let warm_samples = brain.opp_brain.episodes.len().min(COLD_START_EPISODES);
        let models = (0..ENSEMBLE_MODELS)
            .map(|index| ModelState {
                key: MODEL_NAMES[index],
                name: MODEL_DISPLAY_NAMES[index],
                // Pending predictions are forecasts for the next frame only.
                // When no next forecast was produced (for example after a
                // lethal early return), retain scores but expose no forecast.
                predicted: game
                    .cpu_telemetry
                    .next_forecast
                    .and_then(|_| ensemble.pending[index])
                    .map(dir_u8),
                raw_score: ensemble.score(index),
                effective_score: ensemble_rank_score(brain, index),
                hits: ensemble.hits[index],
                samples: ensemble.total[index],
            })
            .collect();

        Self {
            schema_version: STATE_SCHEMA_VERSION,
            slipstream: game.player_in_corridor(),
            w: game.width,
            h: game.height,
            frame: game.frame_count,
            time: game.time,
            over: game.game_over,
            winner: game.winner,
            score: game.score,
            scores: [game.cycles[0].score, game.cycles[1].score],
            food_eaten: game.food_eaten_by,
            wins: game.displayed_wins(),
            speed: game.speed_pct(),
            cycles: game
                .cycles
                .iter()
                .map(|cycle| CycleState {
                    head: [cycle.head.0, cycle.head.1],
                    dir: dir_u8(cycle.direction),
                    alive: cycle.alive,
                    held: cycle.held_powerup.map(powerup_u8),
                    color: [cycle.color.0, cycle.color.1, cycle.color.2],
                    pos: cycle.positions.iter().map(|&(x, y)| [x, y]).collect(),
                })
                .collect(),
            // Planted mines are emitted AS FOOD, with the value they are
            // masquerading as. The client cannot distinguish them because
            // there is nothing to distinguish — that is the mechanic. Tracking
            // where the opponent planted theirs is the counter-play.
            food: {
                let mut f = game.food_items.clone();
                f.extend(game.bombs.iter().map(|b| (b.x, b.y, b.disguise)));
                f
            },
            powerups: game
                .powerups
                .iter()
                .map(|&(x, y, kind)| (x, y, powerup_u8(kind)))
                .collect(),
            bolts: game
                .projectiles
                .iter()
                .map(|bolt| (bolt.x, bolt.y, bolt.dx, bolt.dy))
                .collect(),
            // Deliberately EMPTY. Mines ride in `food` above; exporting them
            // here as well would hand the client the answer and the blast
            // overlay would paint a target on every one of them. The field is
            // kept rather than removed so the wire shape stays stable.
            bombs: Vec::new(),
            bomb_flash: game
                .bombs
                .iter()
                .filter_map(|b| {
                    let t = game.bomb_flash_tier(b);
                    (t > 0).then_some((b.x, b.y, t))
                })
                .collect(),
            beams: game
                .beam_fx
                .iter()
                .map(|fx| (fx.cells.clone(), fx.age))
                .collect(),
            particles: game
                .particles
                .iter()
                .take(300)
                .map(|particle| {
                    (
                        particle.x,
                        particle.y,
                        particle.color.0,
                        particle.color.1,
                        particle.color.2,
                        particle.lifetime,
                    )
                })
                .collect(),
            cause: game.death_cause.map(|cause| cause.as_str().to_owned()),
            brain: BrainState {
                frame: game.cpu_telemetry.frame,
                scored: game.cpu_telemetry.scored.map(Into::into),
                decision: game.cpu_telemetry.decision.clone().map(Into::into),
                last_decision: game.round_last_cpu_decision.clone().map(Into::into),
                next_forecast: game.cpu_telemetry.next_forecast.map(Into::into),
                book: {
                    let b = &game.cpu_brain.class_books;
                    let published = game.cpu_brain.lifetime_read.earned_read();
                    let book = b.spendable();
                    let earned = game.cpu_brain.earned_snapshot;
                    BookState {
                        side_accuracy: if b.turn_events > 0 { Some(b.a_turn()) } else { None },
                        side_events: b.turn_events,
                        coverage: b.coverage(),
                        earned,
                        earned_source: if earned <= 0.0 {
                            "none"
                        } else if book >= published {
                            "book"
                        } else {
                            "forecast"
                        },
                        drift_detected: game.cpu_brain.ledgers.drift_latched,
                        map_populated: {
                            let (p, _, _) = b.map_summary();
                            p
                        },
                        map_thin: {
                            let (_, t, _) = b.map_summary();
                            t
                        },
                        map_unseen: {
                            let (_, _, u) = b.map_summary();
                            u
                        },
                        rounds_observed: game.cpu_brain.ledgers.rounds_seen,
                        drift_z: game.cpu_brain.ledgers.drift_z,
                        rhythm_events: game.cpu_brain.voluntary_pattern.events,
                        rhythm_p_left: if game.cpu_brain.voluntary_pattern.events
                            >= crate::cpu_ai::VOMM_MIN_EVENTS
                        {
                            Some(game.cpu_brain.voluntary_pattern.p_left())
                        } else {
                            None
                        },
                        boxer_aversion: game.cpu_brain.ledgers.boxer_aversion(),
                        tactics: game
                            .cpu_brain
                            .ledgers
                            .tactic_attempts
                            .iter()
                            .map(|e| {
                                let name = match e.0 {
                                    0 => "direct intercept",
                                    1 => "corner cutoff",
                                    2 => "food-path ambush",
                                    _ => "wall-follow press",
                                };
                                (name.to_string(), e.3, e.4)
                            })
                            .collect(),
                        weapons: game
                            .cpu_brain
                            .ledgers
                            .weapon_ops
                            .iter()
                            .map(|e| {
                                let name = match e.0 {
                                    0 => "laser",
                                    1 => "tri-shot",
                                    _ => "disguised mine",
                                };
                                (name.to_string(), e.3, e.4)
                            })
                            .collect(),
                        cpu_losses: game
                            .cpu_brain
                            .ledgers
                            .loss_causes
                            .iter()
                            .map(|e| {
                                let name = match e.0 {
                                    0 => "wall",
                                    1 => "its own trail",
                                    2 => "your trail",
                                    3 => "head-on",
                                    4 => "your mine",
                                    5 => "your laser",
                                    _ => "your tri-shot",
                                };
                                (name.to_string(), e.1, e.2)
                            })
                            .collect(),
                    }
                },
                read_rate: ReadRateScopes {
                    round: (&game.round_read).into(),
                    lifetime: (&game.cpu_brain.lifetime_read).into(),
                },
                read_lift: game.read_rate,
                difficulty: game.difficulty,
                seal: SealState {
                    chain: format!("0x{:016x}", game.seal_chain),
                    frames: game.seal_frames,
                },
                accuracy: AccuracyScopes {
                    round: AccuracyState {
                        hits: game.round_pred_hits,
                        samples: game.round_pred_total,
                        rate: game.round_pred_accuracy(),
                    },
                    lifetime: AccuracyState {
                        hits: brain.opp_pred_hits,
                        samples: brain.opp_pred_total,
                        rate: brain.opp_pred_accuracy(),
                    },
                },
                memory: MemoryState {
                    survival_retained: brain.episodes.len(),
                    opponent_retained: brain.opp_brain.episodes.len(),
                    survival_observed: brain.cpu_seq,
                    opponent_observed: brain.opp_brain.seq,
                    capacity: MAX_EPISODES,
                    warm_samples,
                    warm_at: COLD_START_EPISODES,
                    ready: warm_samples >= COLD_START_EPISODES,
                },
                habits,
                models,
            },
        }
    }
}

pub fn to_json(game: &WormGame) -> String {
    serde_json::to_string(&GameState::from_game(game)).expect("browser game state serializes")
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::{CpuDecisionReason, CpuDecisionTrace, CpuFrameTelemetry, ForecastTrace};

    #[test]
    fn schema_is_versioned_unambiguous_and_has_seven_models() {
        let game = WormGame::with_size_seed(120, 38, 7);
        let value: serde_json::Value = serde_json::from_str(&to_json(&game)).unwrap();
        assert_eq!(value["schemaVersion"], STATE_SCHEMA_VERSION);
        assert!(value["brain"].get("active").is_none());
        assert!(value["brain"].get("pred").is_none());
        assert!(value["brain"].get("path").is_none());

        let models = value["brain"]["models"].as_array().unwrap();
        assert_eq!(models.len(), ENSEMBLE_MODELS);
        let keys: HashSet<_> = models
            .iter()
            .map(|model| model["key"].as_str().unwrap())
            .collect();
        assert_eq!(keys.len(), ENSEMBLE_MODELS);
    }

    #[test]
    fn decision_and_next_forecast_keep_their_own_sources() {
        let mut game = WormGame::with_size_seed(120, 38, 9);
        game.cpu_telemetry = CpuFrameTelemetry {
            frame: 12,
            scored: None,
            decision: Some(CpuDecisionTrace {
                frame: 12,
                heading: Direction::Up,
                reason: CpuDecisionReason::DirectIntercept,
                forecast: Some(ForecastTrace {
                    target_frame: 12,
                    source: 1,
                    predicted: Some(Direction::Right),
                    confidence: 0.75,
                    book: 0,
                    seal: 0,
                }),
                projection: None,
            }),
            next_forecast: Some(ForecastTrace {
                target_frame: 13,
                source: 2,
                predicted: Some(Direction::Down),
                confidence: 0.5,
                book: 0,
                seal: 0,
            }),
        };

        let value: serde_json::Value = serde_json::from_str(&to_json(&game)).unwrap();
        assert_eq!(value["brain"]["decision"]["forecast"]["sourceKey"], "pat");
        assert_eq!(value["brain"]["decision"]["forecast"]["targetFrame"], 12);
        assert_eq!(value["brain"]["nextForecast"]["sourceKey"], "frq");
        assert_eq!(value["brain"]["nextForecast"]["targetFrame"], 13);
    }
}
