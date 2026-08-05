//! The CPU opponent's live-learning brain, ported from the REAL rps-ai
//! mechanism (/opt/rps-ai/src/model.py — a Flask app; there is no TypeScript
//! rps-ai, and no k-NN/temperature machinery in it — earlier revisions of
//! this file claimed otherwise):
//!
//!   - **The rps-ai ensemble** (`Ensemble`, `compute_ensemble`): seven
//!     specialist models, each a falsifiable assumption about the player
//!     (rep/pat/frq/due/wlR/wlL/knn). Every model predicts every frame;
//!     predictions are recorded and scored next frame with quadratic
//!     recency weights (frame j counts j² — a figured-out model craters
//!     fast). The argmax-score model drives the opponent prediction.
//!     Scores reset per game; the k-NN memory persists as the corpus.
//!
//!   - **The k-NN opponent model** (`PlayerBrain`, `predict_player_move`):
//!     the ensemble's sophisticated member — k-NN over player-centric
//!     context vectors (cosine recall, proximity × recency × trailing-match
//!     weights, margin·support·maturity confidence, Laplace-smoothed EMA
//!     prior blend). Abstains while cold, like rps-ai's NN/DT models.
//!
//!   - **The self-survival memory** (`CpuBrain`, `record_episode`):
//!     something rps-ai never had — k-NN over CPU-centric situation vectors
//!     recording (pre-move situation → direction taken → frames survived on
//!     the heading + 10× food). It casts one gated vote in cpu_decide; the
//!     wall-follow floor and flood-fill openness veto traps.

use crate::{CellType, Direction, LightCycle, WormGame};
use std::collections::VecDeque;

/* ----------------------------- constants (rps-ai) ----------------------------- */

const EPS: f32 = 0.01; // 1/(EPS + d^2) softening
                       // exp(-age/DECAY_TAU) — rps-ai's corpus NEVER decays (their Postgres record
                       // grows with every game ever played and retrains the sophisticated models).
                       // A 150-frame tau made our "persistent" memory effectively one game long;
                       // 1500 keeps ~10 games of experience live while proximity dominates recency,
                       // exactly the reference split (corpus global, per-game ensemble responsive).
const DECAY_TAU: f32 = 1500.0;
const MATCH_BONUS: f32 = 1.0; // trailing-match re-rank multiplier
const SUPPORT_TARGET: f32 = 5.0; // effective-N full-support threshold
pub const COLD_START_EPISODES: usize = 60;
const EXPLORE_RATE: f32 = 0.05; // outright random legal throw rate
const MEMORY_VOTE_MIN_OPEN: f32 = 0.05; // destination must keep >=5% of the arena reachable

/// How much the CPU wants a power-up of this kind, given what it already holds.
///
/// It could not previously tell them apart — every site matched on the bare
/// `CellType::PowerUp` — so it would detour for anything and destroy a held
/// Laser by driving over a Bomb. The human has always been able to see the
/// icons and choose; this closes that gap.
pub fn powerup_worth_taking(game: &WormGame, who: usize, _kind: crate::game::PowerUpKind) -> bool {
    // Refuse only one thing: trading away a laser that is MID-CHARGE. The shot
    // is nearly paid for and a pickup silently overwrites the slot.
    //
    // A ranked version of this — refuse anything that does not outrank what we
    // hold — was implemented and MEASURED, and it made the CPU markedly worse:
    // win rate 97% -> 83%, player wins 1 -> 5. Laser outranks everything, so
    // once holding one the CPU refused every other power-up; and because its
    // laser needs ten consecutively-aligned frames to fire, it frequently held
    // that laser forever and stopped collecting at all, leaving the board to
    // the human. Preference is not worth a famine. The KIND information stays
    // available (powerup_at, and the opponent's held kind in the feature
    // vector) — it was the policy that was wrong, not the knowledge.
    !(who == 1 && game.cpu_laser_charge > 0)
}

/// Reachable cells a destination must leave for the cycle to be able to keep
/// playing from it: enough room to outrun its own body, plus a manoeuvring
/// margin. Counts `pending_growth` because food already eaten is body the
/// cycle does not have yet — entering a twelve-cell pocket right after
/// swallowing a nine is the classic snake-AI death.
///
/// Length-relative by design. See the note at the `escape_cells` call site for
/// why the previous arena-fraction floor failed at both ends of a round.
fn escape_floor_cells(game: &WormGame, who: usize) -> f32 {
    let c = &game.cycles[who];
    let own_len = c.positions.len() as f32 + c.pending_growth as f32;
    own_len * ESCAPE_LENGTH_MULTIPLE + ESCAPE_MARGIN_CELLS
}

/// Leave a sudden-death ring this many frames before it seals, rather than at
/// the last instant — one frame of slack is not enough when the move that
/// takes you off the ring may itself be blocked.
const RING_EVACUATE_FRAMES: u32 = 3;

/// Would stepping `d` from `from` land on a sudden-death ring about to seal?
/// `close_ring` kills any head standing on the ring it closes.
fn ring_doomed_step(game: &WormGame, from: (u16, u16), d: Direction) -> bool {
    let (ddx, ddy) = d.as_delta();
    let nx = (from.0 as i16 + ddx).max(0).min((game.width - 1) as i16) as u16;
    let ny = (from.1 as i16 + ddy).max(0).min((game.height - 1) as i16) as u16;
    matches!(game.ring_seal_eta(nx, ny), Some(eta) if eta <= RING_EVACUATE_FRAMES)
}

/// `preferred`, unless it steps onto a ring about to seal — then the first
/// candidate that does not.
///
/// Sudden death outranks EVERY decision layer, including the cold-start
/// wall-follow: a head on the sealing ring dies regardless of how thoughtfully
/// the move was chosen. Applying this only to the lower layers was the bug —
/// a fresh brain is always cold, so the cold-start path is exactly the one
/// running when a first-time player is watching.
fn evacuate_ring(
    game: &WormGame,
    from: (u16, u16),
    preferred: Direction,
    pool: &[Direction],
) -> Direction {
    if !ring_doomed_step(game, from, preferred) {
        return preferred;
    }
    pool.iter()
        .copied()
        .find(|&d| !ring_doomed_step(game, from, d))
        .unwrap_or(preferred)
}

/// How much of its escape margin a HUNT may spend at a perfect read. At
/// `read_rate` 0 the floor is untouched (today's behaviour exactly); at 1.0 the
/// CPU commits to an intercept on ~45% of the room it normally insists on.
const HUNT_MARGIN_SPEND: f32 = 0.55;
/// Superlinear, so a strong read bites noticeably harder than a middling one
/// rather than the whole range feeling the same.
const HUNT_MARGIN_CURVE: f32 = 0.7;

/// The floor a HUNT deviation must clear, given how well the CPU reads this
/// player.
///
/// Confidence buys COMMITMENT, never safety margin. `escape_floor_cells` — the
/// survival floor — is untouched at every read rate, and the threat-dodge,
/// ring-evacuation, forced-move and wall-follow layers do not consult this at
/// all. A well-read player faces a CPU that takes intercepts it would
/// otherwise decline; they never face one that suicides. Never drops below the
/// flat manoeuvring allowance, so it cannot be tuned into recklessness.
pub fn hunt_floor_cells(game: &WormGame, who: usize, read_rate: f32) -> f32 {
    let spend = HUNT_MARGIN_SPEND * read_rate.clamp(0.0, 1.0).powf(HUNT_MARGIN_CURVE);
    (escape_floor_cells(game, who) * (1.0 - spend)).max(ESCAPE_MARGIN_CELLS)
}

/// How many times its own length a cycle wants reachable before committing.
const ESCAPE_LENGTH_MULTIPLE: f32 = 3.0;
/// Flat manoeuvring allowance on top, so a very short cycle still needs room.
const ESCAPE_MARGIN_CELLS: f32 = 8.0;
/// Survival floor for hunt-layer deviations: the destination must keep at
/// least this much of the arena reachable (and at least half of wall-follow's
/// space) — the anti-kamikaze gate.
// (The former `SURVIVAL_MIN_OPEN = 0.12` arena-fraction floor was replaced by
// the length-relative `escape_floor_cells`; see its call site in `cpu_decide`.)
const SELF_VOTE_MIN_CONFIDENCE: f32 = 0.4; // margin x support x maturity gate (rps-ai confidence)
const RECALL_K: usize = 16;
const CLEAR_BIAS: f32 = 0.125; // prior saturation point (1/8 bias over 4 dirs)
const PRIOR_DECAY: f32 = 0.99; // EMA prior (~100-round window)

/// Retention cap — the corpus, mirroring rps-ai's ever-growing record table.
/// The seq counter keeps climbing past this; that is what recency decay ages
/// against, NOT the episode count.
pub const MAX_EPISODES: usize = 4000;

/// Why the CPU chose its final heading this frame. This is deliberately
/// separate from the ensemble's active model: the model predicts the player;
/// safety, item, intercept, memory, and wall-follow layers decide the move.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum CpuDecisionReason {
    Opening,
    NoLegalMove,
    ForcedMove,
    WarmingUp,
    ThreatDodge,
    CloseEvasion,
    ItemPickup,
    ItemPath,
    CornerIntercept,
    DirectIntercept,
    SurvivalMemory,
    WallFollow,
}

impl CpuDecisionReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Opening => "opening scan",
            Self::NoLegalMove => "no safe move",
            Self::ForcedMove => "only safe move",
            Self::WarmingUp => "warming up · wall safety",
            Self::ThreatDodge => "dodging a weapon",
            Self::CloseEvasion => "evading your predicted path",
            Self::ItemPickup => "taking a nearby item",
            Self::ItemPath => "routing to an item",
            Self::CornerIntercept => "cutting off your next corner",
            Self::DirectIntercept => "intercepting your predicted path",
            Self::SurvivalMemory => "reusing a surviving move",
            Self::WallFollow => "following the survival floor",
        }
    }
}

/// A model forecast with an explicit target frame. Keeping the target beside
/// the prediction prevents the HUD from presenting a newly-computed forecast
/// as the evidence behind an action that already happened.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ForecastTrace {
    pub target_frame: u32,
    pub source: usize,
    pub predicted: Option<Direction>,
    pub confidence: f32,
    /// Hash of the prediction, published BEFORE the player's input for
    /// `target_frame` is read, and revealed after. See [`seal_commit`].
    pub seal: u64,
}

/// The previous forecast scored against the player's move on this frame.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ScoredForecast {
    pub forecast: ForecastTrace,
    pub actual: Direction,
    pub hit: bool,
}

/// Counterfactual player path consulted by the decision layers.
#[derive(Clone, PartialEq, Debug)]
pub struct PlayerProjection {
    pub direction: Direction,
    pub path: Vec<(u16, u16)>,
}

/// What the CPU actually chose this frame and the evidence it consulted.
#[derive(Clone, PartialEq, Debug)]
pub struct CpuDecisionTrace {
    pub frame: u32,
    pub heading: Direction,
    pub reason: CpuDecisionReason,
    pub forecast: Option<ForecastTrace>,
    pub projection: Option<PlayerProjection>,
}

/// One coherent telemetry transaction for a game frame.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct CpuFrameTelemetry {
    pub frame: u32,
    pub scored: Option<ScoredForecast>,
    pub decision: Option<CpuDecisionTrace>,
    pub next_forecast: Option<ForecastTrace>,
}

impl CpuFrameTelemetry {
    pub fn for_frame(frame: u32) -> Self {
        Self {
            frame,
            ..Self::default()
        }
    }
}

pub const CPU_FEATURE_DIM: usize = 25;
/// Dimensionality of the opponent-centric context vector. Slots 0..13 are
/// coded; 13..29 encode a 4×4 player direction-transition matrix
/// (previous direction → current direction, order-matters for corner patterns);
/// 29..32 are zero-padding.
pub const PLAYER_FEATURE_DIM: usize = 32;

/// A learned episode: the situation vector, the direction that won from it, the
/// reward that move earned (survival frames + food), and a monotonic seq.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CpuEpisode {
    pub vector: [f32; CPU_FEATURE_DIM],
    pub surviving_dir: Direction,
    pub reward: f32,
    pub seq: u32,
}

/// An opponent-centric learned episode: the context vector before the player
/// moved, and the direction the player took next. The k-NN vote operates on
/// `next_dir` to build a prediction of the player's intent.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PlayerEpisode {
    pub vector: [f32; PLAYER_FEATURE_DIM],
    pub next_dir: Direction,
    pub seq: u32,
}

/// A dual-mode brain: the legacy self-centric CpuBrain is always present as a
/// survival fallback; the optional opp_brain is the opponent model that
/// powers adaptive play once it has enough data.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PlayerBrain {
    pub episodes: VecDeque<PlayerEpisode>,
    /// Monotonic counter, mirrors CpuBrain's usage for recency decay.
    pub seq: u32,
    /// EMA base-rate on the player's observed moves, a fallback prior.
    pub tally: [f32; 4],
    /// EMA base-rate over the player's TURNS relative to their own heading.
    ///
    /// The absolute `tally` above cannot hold a habit like "breaks left when
    /// cornered": measured, a persona turning left 88% of the time produced an
    /// absolute distribution of Up .11 Down .11 Left .38 Right .39 — the habit
    /// smears across all four compass directions and the model confidently
    /// learns the wrong thing. Relative to the heading it is one number.
    ///
    /// `#[serde(skip)]` + its own WRM2 section: bincode is not field-tolerant,
    /// and a serialized field here would break the legacy WRM1 path.
    #[serde(skip)]
    pub turn_tally: [f32; TURNS],
}

impl Default for PlayerBrain {
    fn default() -> Self {
        Self {
            episodes: VecDeque::new(),
            seq: 0,
            tally: [0.0; 4],
            turn_tally: [0.0; TURNS],
        }
    }
}

impl PlayerBrain {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an opponent observation `(vector, player_next_dir)` with a monotonic seq.
    pub fn remember(&mut self, vector: [f32; PLAYER_FEATURE_DIM], next_dir: Direction) {
        let seq = self.seq;
        self.seq += 1;
        self.episodes.push_back(PlayerEpisode {
            vector,
            next_dir,
            seq,
        });
        while self.episodes.len() > MAX_EPISODES {
            self.episodes.pop_front();
        }
    }

    /// Laplace-smoothed prior over the player's directions (a fallback vote).
    pub fn prior_distribution(&self) -> [f32; 4] {
        let pseudo = 1.0;
        let counts: [f32; 4] = [
            self.tally[0] + pseudo,
            self.tally[1] + pseudo,
            self.tally[2] + pseudo,
            self.tally[3] + pseudo,
        ];
        let total: f32 = counts.iter().sum();
        let inv = 1.0 / total;
        [
            counts[0] * inv,
            counts[1] * inv,
            counts[2] * inv,
            counts[3] * inv,
        ]
    }

    /// TV-distance from uniform, normalised (see CpuBrain::prior_strength).
    pub fn prior_strength(&self) -> f32 {
        let prior = self.prior_distribution();
        let tvd: f32 = prior.iter().map(|p| (p - 0.25).abs()).sum::<f32>() / 2.0;
        (tvd / CLEAR_BIAS).min(1.0)
    }

    /// Fold one observed turn into the relative-turn prior.
    pub fn observe_turn(&mut self, turn: Turn) {
        for i in 0..TURNS {
            self.turn_tally[i] *= PRIOR_DECAY;
        }
        self.turn_tally[turn_index(turn)] += 1.0;
    }

    /// KT-smoothed (Jeffreys, Dirichlet-1/2) prior over turns. Uniform — and
    /// therefore inert — until the player has shown a bias.
    ///
    /// This was add-one Laplace, which is the estimator the sequence-
    /// prediction literature rejects for exactly our regime: with a handful of
    /// observations, add-one over-smooths systematically. Concretely, after
    /// four all-left observations Laplace says 0.714 while KT says 0.818 —
    /// against a true habit of 0.85. The Krichevsky–Trofimov estimator
    /// ((n + 1/2)/(N + K/2)) is the asymptotically minimax choice and the one
    /// the whole CTW family is built on. Same shape, better constants, still
    /// trivially explainable: "it counts your turns, with the half-count
    /// start suited to small samples".
    pub fn turn_prior(&self) -> [f32; TURNS] {
        let c = [
            self.turn_tally[0] + 0.5,
            self.turn_tally[1] + 0.5,
            self.turn_tally[2] + 0.5,
        ];
        let inv = 1.0 / (c[0] + c[1] + c[2]);
        [c[0] * inv, c[1] * inv, c[2] * inv]
    }

    /// Total observed turn mass — how many genuine left/right choices the
    /// prior is standing on. The confidence input for exploitation gating.
    pub fn turn_observations(&self) -> f32 {
        self.turn_tally.iter().sum()
    }

    /// Update the EMA base-rate tally for the player's moves (rps-ai `moveTally`).
    pub fn observe(&mut self, dir: Direction) {
        let idx = dir_index(dir);
        for i in 0..4 {
            self.tally[i] *= PRIOR_DECAY;
        }
        self.tally[idx] += 1.0;
    }
}

/// A move expressed RELATIVE to the heading it was made from.
///
/// The opponent model's habits live in this space, not in absolute
/// directions. "Breaks left when cornered" is one pattern here and four
/// unrelated patterns in absolute space, so relative labelling is what lets a
/// single observation generalise across all four headings.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, serde::Serialize, serde::Deserialize)]
pub enum Turn {
    Straight,
    Left,
    Right,
}

pub const TURNS: usize = 3;

#[inline]
pub fn turn_index(t: Turn) -> usize {
    match t {
        Turn::Straight => 0,
        Turn::Left => 1,
        Turn::Right => 2,
    }
}

impl Turn {
    /// The absolute direction this turn produces from `heading`.
    #[inline]
    pub fn apply(self, heading: Direction) -> Direction {
        match self {
            Turn::Straight => heading,
            Turn::Left => left_turn(heading),
            Turn::Right => right_turn(heading),
        }
    }

    /// The turn taking `heading` to `next`, or `None` for a reversal.
    ///
    /// `None` is reachable: the 180 latch lives in `change_direction`, and
    /// tests assign `cycles[n].direction` directly, bypassing it. Callers
    /// must skip the frame, never unwrap.
    #[inline]
    pub fn from_dirs(heading: Direction, next: Direction) -> Option<Turn> {
        if next == heading {
            Some(Turn::Straight)
        } else if next == left_turn(heading) {
            Some(Turn::Left)
        } else if next == right_turn(heading) {
            Some(Turn::Right)
        } else {
            None
        }
    }
}

/// How many distinct moves cycle `who` could legally commit to right now.
///
/// Counted at frame start, so the board is what the player actually saw. The
/// reversal is excluded against `prev_direction` — the direction actually
/// MOVED last tick — because that is the same latch `change_direction`
/// enforces. Range is 0..=3, never 4, which is why uniform chance at a
/// decision is ~1/3 and never the 1/4 the UI used to claim.
pub fn option_count(game: &WormGame, who: usize) -> u8 {
    let c = &game.cycles[who];
    let banned = match c.prev_direction {
        Direction::Up => Direction::Down,
        Direction::Down => Direction::Up,
        Direction::Left => Direction::Right,
        Direction::Right => Direction::Left,
    };
    let vacating = |cy: &LightCycle, cell: (u16, u16)| {
        cy.positions.len() > 1 && cy.pending_growth == 0 && cy.positions.last() == Some(&cell)
    };
    [
        Direction::Up,
        Direction::Down,
        Direction::Left,
        Direction::Right,
    ]
    .into_iter()
    .filter(|&d| d != banned)
    .filter(|&d| {
        let (dx, dy) = d.as_delta();
        let nx = c.head.0 as i16 + dx;
        let ny = c.head.1 as i16 + dy;
        if nx < 0 || ny < 0 || nx >= game.width as i16 || ny >= game.height as i16 {
            return false;
        }
        let cell = (nx as u16, ny as u16);
        if game.passable(cell.0, cell.1) {
            return true;
        }
        // A tail tip about to retract is a legal target, and ignoring that
        // would under-count a coiled player's real options.
        if game.bombs.iter().any(|b| (b.x, b.y) == cell) {
            return false;
        }
        let other = 1 - who;
        vacating(c, cell) || (game.cycles[other].alive && vacating(&game.cycles[other], cell))
    })
    .count() as u8
}

/// The directions cycle `who` could legally commit to next, in a fixed order.
pub fn legal_options(game: &WormGame, who: usize) -> Vec<Direction> {
    legal_options_from(game, who, game.cycles[who].prev_direction)
}

/// As `legal_options`, but with the heading stated explicitly.
///
/// The reversal ban is relative to the direction actually MOVED, and which
/// field holds that depends on where in the frame you are: before
/// `snapshot_direction` runs, `prev_direction` is last frame's move and
/// `direction` is this frame's. Forecasting the NEXT frame from the end of
/// this one therefore has to pass `direction` — using the default here would
/// ban the wrong move and admit an illegal one.
pub fn legal_options_from(game: &WormGame, who: usize, heading: Direction) -> Vec<Direction> {
    let c = &game.cycles[who];
    let banned = match heading {
        Direction::Up => Direction::Down,
        Direction::Down => Direction::Up,
        Direction::Left => Direction::Right,
        Direction::Right => Direction::Left,
    };
    let vacating = |cy: &LightCycle, cell: (u16, u16)| {
        cy.positions.len() > 1 && cy.pending_growth == 0 && cy.positions.last() == Some(&cell)
    };
    [
        Direction::Up,
        Direction::Down,
        Direction::Left,
        Direction::Right,
    ]
    .into_iter()
    .filter(|&d| d != banned)
    .filter(|&d| {
        let (dx, dy) = d.as_delta();
        let nx = c.head.0 as i16 + dx;
        let ny = c.head.1 as i16 + dy;
        if nx < 0 || ny < 0 || nx >= game.width as i16 || ny >= game.height as i16 {
            return false;
        }
        let cell = (nx as u16, ny as u16);
        if game.passable(cell.0, cell.1) {
            return true;
        }
        if game.bombs.iter().any(|b| (b.x, b.y) == cell) {
            return false;
        }
        let other = 1 - who;
        vacating(c, cell) || (game.cycles[other].alive && vacating(&game.cycles[other], cell))
    })
    .collect()
}

/// Constrain a forecast to moves the player can actually make.
///
/// The models happily predict a direction into a wall. That is free accuracy
/// on the ~95% of frames where the player continues straight, and it is
/// *worthlessness* on the handful of frames that decide anything: when
/// straight is blocked, the player MUST turn, the answer is Left or Right, and
/// a model still answering "straight" has abstained from the only question
/// worth asking.
///
/// Those forced frames are exactly where a habit lives — "breaks left when
/// cornered" is a statement about them and nothing else. Masking to the legal
/// set converts an abstention into a real guess, and the direction prior picks
/// which way. Cheap, and it is the difference between measuring a habit and
/// using one.
pub fn mask_to_legal(
    predicted: Option<Direction>,
    legal: &[Direction],
    heading: Direction,
    turn_prior: &[f32; TURNS],
) -> Option<Direction> {
    if legal.is_empty() {
        return predicted;
    }
    // A FORCED TURN: the player cannot continue straight, so they must break
    // one way or the other. This is the only moment a turning habit is
    // expressed, and it is precisely where the absolute models have NEGATIVE
    // skill — measured, they read a left-breaking persona at 38% against a
    // 50% baseline, because their answer defaults to the current heading and
    // a forced turn guarantees that is wrong. So do not consult them here;
    // the relative prior is the only estimator with any claim to the frame.
    let forced_turn = !legal.contains(&heading);
    if forced_turn {
        return legal
            .iter()
            .copied()
            .max_by(|a, b| {
                let score = |d: Direction| {
                    Turn::from_dirs(heading, d).map_or(0.0, |t| turn_prior[turn_index(t)])
                };
                score(*a)
                    .partial_cmp(&score(*b))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
    }

    match predicted {
        // The model named something they can do — take it.
        Some(d) if legal.contains(&d) => Some(d),
        // Impossible (or absent) while straight was still available.
        //
        // The fallback must be relative, not absolute. Asking "which compass
        // direction does this player like?" cannot answer it: a player who
        // breaks left 88% of the time has a near-uniform absolute
        // distribution, because left-of-Up and left-of-Right are different
        // compass directions. Asking "which way do they turn?" answers it in
        // one number.
        _ => legal
            .iter()
            .copied()
            .max_by(|a, b| {
                let score = |d: Direction| {
                    Turn::from_dirs(heading, d).map_or(0.0, |t| turn_prior[turn_index(t)])
                };
                score(*a)
                    .partial_cmp(&score(*b))
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
    }
}

/// FNV-1a 64. Deliberately hand-rolled and deliberately not cryptographic:
/// pulling in a crypto crate would imply a guarantee this cannot make.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    h
}

/// Per-frame salt. A pure function of `(seal_seed, target_frame)` so it never
/// touches the game RNG — drawing from that stream would shift food spawns and
/// explore rolls, and silently invalidate every seeded benchmark in the repo.
pub fn seal_salt(seal_seed: u64, target_frame: u32) -> u64 {
    fnv1a64(
        &[
            seal_seed.to_le_bytes().as_slice(),
            target_frame.to_le_bytes().as_slice(),
        ]
        .concat(),
    )
}

/// The commitment published before the player's input for `target_frame`.
///
/// WHAT THIS PROVES, precisely: that the forecast was fixed before your input
/// for that frame was read. It is tamper-evidence against a refactor that
/// moves forecast generation after input, and against a hand-edited
/// transcript.
///
/// WHAT IT DOES NOT PROVE: anything against a hostile host. The whole game
/// runs on your machine and the salt derives from a seed the page knows.
/// Claiming more would be the same dishonesty the read-rate work exists to
/// undo, so the explainer says this out loud.
pub fn seal_commit(salt: u64, predicted: Option<Direction>, target_frame: u32) -> u64 {
    let code = predicted.map(|d| dir_index(d) as u8).unwrap_or(255);
    fnv1a64(
        &[
            salt.to_le_bytes().as_slice(),
            &[code],
            target_frame.to_le_bytes().as_slice(),
        ]
        .concat(),
    )
}

/// Minimum frames on which the CPU and the trivial baseline DISAGREED before
/// a read rate is reported. Not a frame count — see `ReadRate::is_ready`.
pub const READ_RATE_MIN_DISCORDANT: u32 = 20;

/// Complementary error function, Abramowitz & Stegun 7.1.26. Only used on the
/// far tail where the exact McNemar sum is out of range.
fn erfc_approx(x: f64) -> f64 {
    let z = x.abs();
    let t = 1.0 / (1.0 + 0.5 * z);
    let y = t
        * (-z * z - 1.26551223
            + t * (1.00002368
                + t * (0.37409196
                    + t * (0.09678418
                        + t * (-0.18628806
                            + t * (0.27886807
                                + t * (-1.13520398
                                    + t * (1.48851587
                                        + t * (-0.82215223 + t * 0.17087277)))))))))
        .exp();
    if x >= 0.0 {
        y
    } else {
        2.0 - y
    }
}

/// How well the CPU actually reads this player.
///
/// The metric this replaces counted every frame and sat at 84-99% forever,
/// because ~95% of frames the player is continuing straight and predicting
/// that is free. This one is scored the same way but reported against the
/// right null, which is the part that matters.
///
/// THE BASELINE IS THE PLAYER'S OWN BASE RATE, not uniform chance. Measured
/// on this game: against a straight-driving opponent the player goes straight
/// 98.4% of the time, so "always predict Straight" scores 98%. Against a
/// uniform 33% baseline that reads as a triumph for a model that has learned
/// nothing — the vanity metric smuggled back in. rps-ai can use 33% because a
/// human playing rock-paper-scissors really is near-uniform; a human driving a
/// worm is not, and pretending otherwise flatters the CPU by construction.
///
/// So the headline number is LIFT over always-predicting-the-commonest-turn.
/// It cannot be gamed by predicting straight, which is exactly the property
/// the number needs to survive contact with someone who reads the source.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ReadRate {
    /// Correctly predicted decisions.
    pub hits: u32,
    /// Decisions scored.
    pub samples: u32,
    /// Turns actually taken, indexed by `turn_index`. Doubles as the running
    /// state of the baseline predictor.
    pub taken: [u32; TURNS],
    /// Scored decisions bucketed by how many legal options the player had.
    /// Only [2] and [3] can be non-zero (a reversal is never legal), which is
    /// why uniform chance here is ~1/3 and never 1/4.
    pub opts: [u32; 4],
    /// Correct calls by the ONLINE baseline — "predict the commonest turn seen
    /// so far". Counted at the same instants as `hits`, under the same
    /// information constraint.
    pub mode_hits: u32,
    /// McNemar discordant pairs. `cpu_only` = CPU right where the baseline was
    /// wrong; `mode_only` = the reverse. Frames where both agreed carry no
    /// evidence about which predictor is better and are deliberately not
    /// counted anywhere.
    pub cpu_only: u32,
    pub mode_only: u32,
}

impl ReadRate {
    /// The commonest turn seen SO FAR. Ties break to the lowest index, so the
    /// baseline is deterministic and a run is replayable. `None` before any
    /// evidence — a predictor with no data has no call, and that counts as a
    /// miss rather than a free pass.
    fn modal_turn(&self) -> Option<Turn> {
        let max = *self.taken.iter().max()?;
        if max == 0 {
            return None;
        }
        (0..TURNS)
            .find(|&i| self.taken[i] == max)
            .map(|i| match i {
                0 => Turn::Straight,
                1 => Turn::Left,
                _ => Turn::Right,
            })
    }

    pub fn record(&mut self, options: u8, actual: Turn, hit: bool) {
        // Score the baseline BEFORE folding this frame in, so it never peeks
        // at the outcome it is being judged on — the same information the CPU
        // had, at the same instant.
        let mode_hit = self.modal_turn() == Some(actual);

        self.samples += 1;
        self.taken[turn_index(actual)] += 1;
        self.opts[(options as usize).min(3)] += 1;
        if hit {
            self.hits += 1;
        }
        if mode_hit {
            self.mode_hits += 1;
        }
        // Only disagreements carry evidence about which predictor is better.
        match (hit, mode_hit) {
            (true, false) => self.cpu_only += 1,
            (false, true) => self.mode_only += 1,
            _ => {}
        }
    }

    /// Raw hit rate. On its own this number is not evidence of anything.
    pub fn rate(&self) -> f32 {
        if self.samples == 0 {
            0.0
        } else {
            self.hits as f32 / self.samples as f32
        }
    }

    /// What "always predict the commonest turn seen so far" actually scored —
    /// the trivial rival the model has to beat to have earned anything.
    ///
    /// Realized, not hindsight. Scoring `max(taken)/samples` would apply the
    /// FINAL mode retroactively to every frame including the ones where it was
    /// not yet knowable — an oracle no one could have run, and one that
    /// produces no per-trial outcomes to pair the CPU against.
    pub fn base_rate(&self) -> f32 {
        if self.samples == 0 {
            0.0
        } else {
            self.mode_hits as f32 / self.samples as f32
        }
    }

    /// Frames on which the CPU and the trivial baseline disagreed. This — not
    /// the frame count — is the real sample size of the claim "it beat your
    /// habits". Against a very predictable player the two agree almost always,
    /// and thousands of frames can carry a dozen frames of actual evidence.
    pub fn discordant(&self) -> u32 {
        self.cpu_only + self.mode_only
    }

    /// One-sided exact McNemar p-value: the probability of the CPU leading the
    /// baseline by this much among the frames where they disagreed, if the two
    /// were really equally good.
    ///
    /// Exact rather than a normal approximation, because `discordant()` is
    /// routinely small and "your normal approximation is invalid at n = 8" is
    /// the first thing a reader will say. Under the null each discordant frame
    /// is a coin flip, so this is the upper tail of Binomial(n, 0.5).
    pub fn p_value(&self) -> f32 {
        let n = self.discordant();
        if n == 0 {
            return 1.0;
        }
        if n > 1000 {
            // Beyond the exact range, fall back to a continuity-corrected
            // normal. At this n the answer is astronomically small anyway.
            let b = self.cpu_only as f64;
            let c = self.mode_only as f64;
            let z = ((b - c).abs() - 1.0).max(0.0) / (n as f64).sqrt();
            return if b > c {
                (0.5 * erfc_approx(z / std::f64::consts::SQRT_2)) as f32
            } else {
                1.0
            };
        }
        // Iterate the PMF multiplicatively; C(n,k) directly would overflow.
        let n_u = n as u64;
        let mut pmf = 0.5f64.powi(n as i32); // k = 0
        let mut tail = 0.0f64;
        for k in 0..=n_u {
            if k >= self.cpu_only as u64 {
                tail += pmf;
            }
            if k < n_u {
                pmf = pmf * (n_u - k) as f64 / (k + 1) as f64;
            }
        }
        tail.clamp(0.0, 1.0) as f32
    }

    /// Continuity-corrected z, for anyone who wants it on the wire.
    pub fn z(&self) -> f32 {
        let n = self.discordant();
        if n == 0 {
            return 0.0;
        }
        let b = self.cpu_only as f32;
        let c = self.mode_only as f32;
        ((b - c).abs() - 1.0).max(0.0) * (b - c).signum() / (n as f32).sqrt()
    }

    /// Uniform-choice chance, exact rather than assumed: the mean of 1/k over
    /// the decisions actually faced. Reported alongside the base rate because
    /// it is the number rps-ai shows, but it is NOT what significance is
    /// judged against here.
    pub fn uniform_chance(&self) -> f32 {
        if self.samples == 0 {
            return 0.0;
        }
        let expected: f32 = (2..=3).map(|k| self.opts[k] as f32 / k as f32).sum();
        expected / self.samples as f32
    }

    /// THE headline. Fraction of the improvement available over the trivial
    /// predictor that the model actually captured, in [0, 1].
    ///
    /// 0.0 means "no better than assuming you do the usual thing". 1.0 means
    /// every decision called. Self-normalising: a player who genuinely never
    /// turns produces a base rate near 1.0 and cannot inflate the score by
    /// being predictable.
    pub fn lift(&self) -> f32 {
        let base = self.base_rate().min(0.999);
        if self.samples == 0 {
            return 0.0;
        }
        ((self.rate() - base) / (1.0 - base)).clamp(0.0, 1.0)
    }

    /// Enough DISAGREEMENTS to say anything. Gating on frame count would be
    /// measuring the wrong thing: against a player whose habits the baseline
    /// already captures, the two predictors agree on nearly every frame, and
    /// no number of agreements is evidence about which is better.
    pub fn is_ready(&self) -> bool {
        self.discordant() >= READ_RATE_MIN_DISCORDANT
    }

    /// The CPU is beating the player's own habits, not just repeating them.
    pub fn is_significant(&self) -> bool {
        self.is_ready() && self.cpu_only > self.mode_only && self.p_value() <= 0.025
    }

    /// One phrase, worded identically in the browser panel and the terminal
    /// HUD so the two builds can never tell the player different stories.
    pub fn hud_phrase(&self) -> String {
        if self.samples == 0 {
            "read —/no decisions".to_string()
        } else if !self.is_ready() {
            format!("read —/{} of {}", self.samples, READ_RATE_MIN_DISCORDANT)
        } else {
            format!(
                "read {:.0}% vs usual {:.0}% · lift {:.0}%{}",
                self.rate() * 100.0,
                self.base_rate() * 100.0,
                self.lift() * 100.0,
                if self.is_significant() { " *" } else { "" },
            )
        }
    }
}

/// k-NN reasoning result.
#[derive(Debug)]
pub struct CpuAggregate {
    pub distribution: [f32; 4],
    pub confidence: f32,
    pub margin: f32,
    pub support: f32,
    pub maturity: f32,
    pub prior_weight: f32,
}

/// The CPU's vector memory plus its base-rate prior (rps-ai `moveTally`/`priorFrom`).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CpuBrain {
    pub episodes: VecDeque<CpuEpisode>,
    /// Monotonic counter — the recency term ages against THIS, never episode count.
    pub cpu_seq: u32,
    /// EMA counts of which direction earned reward globally — the prior.
    pub tally: [f32; 4],
    /// Rolling tail of the player's recent moves (for the trailing-match bonus).
    pub player_tail: VecDeque<Direction>,
    pub tail_len: usize,
    /// Opponent model: predicts the player's next move. Optional but always
    /// initialised so game restart/reset logic is unchanged.
    pub opp_brain: PlayerBrain,
    /// Legacy persistence slot for the active prediction. Runtime scoring uses
    /// `WormGame::cpu_telemetry.next_forecast`, which also retains its source,
    /// confidence, and target frame. Kept in the serialized brain so existing
    /// WRM1 corpora remain readable; cleared whenever a brain is restored.
    pub last_opp_prediction: Option<Direction>,
    /// Prediction hit/miss bookkeeping — accuracy = hits / total.
    pub opp_pred_hits: u32,
    pub opp_pred_total: u32,
    /// The rps-ai ensemble: per-model quadratic scores + the active driver.
    pub ensemble: Ensemble,
    /// Lifetime read record — how well the CPU reads THIS human, measured
    /// against their own base rate.
    ///
    /// `#[serde(skip)]` is load-bearing, not stylistic. `bincode` is not
    /// field-tolerant: a serialized field added here would make the legacy
    /// WRM1 path (`bincode::deserialize::<Self>`) fail on every old blob, so
    /// a returning player would lose their entire corpus in the release that
    /// added a metric. It rides in its own WRM2 section instead.
    #[serde(skip)]
    pub lifetime_read: ReadRate,
}

impl Default for CpuBrain {
    fn default() -> Self {
        Self {
            episodes: VecDeque::new(),
            cpu_seq: 0,
            tally: [0.0; 4],
            player_tail: VecDeque::new(),
            tail_len: 4,
            opp_brain: PlayerBrain::default(),
            last_opp_prediction: None,
            opp_pred_hits: 0,
            opp_pred_total: 0,
            ensemble: Ensemble::default(),
            lifetime_read: ReadRate::default(),
        }
    }
}

impl CpuBrain {
    pub fn new() -> Self {
        Self::default()
    }

    /// Laplace-smoothed prior over directions. Uniform tally → uniform prior,
    /// which is inert in a blend (rps-ai `priorFrom`).
    pub fn prior_distribution(&self) -> [f32; 4] {
        let pseudo = 1.0;
        let counts: [f32; 4] = [
            self.tally[0] + pseudo,
            self.tally[1] + pseudo,
            self.tally[2] + pseudo,
            self.tally[3] + pseudo,
        ];
        let total: f32 = counts.iter().sum();
        let inv = 1.0 / total;
        [
            counts[0] * inv,
            counts[1] * inv,
            counts[2] * inv,
            counts[3] * inv,
        ]
    }

    /// TV-distance from uniform, normalised so CLEAR_BIAS is fully established
    /// (rps-ai `priorStrength`). Inert when the prior is flat.
    pub fn prior_strength(&self) -> f32 {
        let prior = self.prior_distribution();
        let uniform = 0.25;
        let tvd: f32 = prior.iter().map(|p| (p - uniform).abs()).sum::<f32>() / 2.0;
        (tvd / CLEAR_BIAS).min(1.0)
    }

    /// Fold a *survived* move into the direction prior.
    ///
    /// Only rewarded moves are credited. A crash episode is recorded with
    /// `reward = 0.0`, and this used to add a flat `1.0 + 0.0` for it — so
    /// dying reinforced the direction that killed the CPU by exactly as much
    /// as surviving a frame did. The k-NN *vote* was already protected (crash
    /// episodes are zero-weighted by the `survived` factor in `aggregate`),
    /// but the *prior* was not — and the prior is what blends in when memory
    /// confidence is low, i.e. precisely when the CPU is least sure and most
    /// in need of not repeating a fatal move.
    ///
    /// The decay still runs on a death — every direction ages, none is
    /// credited. Note this does not *push the prior away* from the fatal
    /// direction: with a Laplace-smoothed prior, decaying the others slightly
    /// raises an uncredited direction's relative share. Actively penalising a
    /// death is a separate question (it belongs to a death-memory, not to a
    /// counter that only knows how to add). What this guarantees is the
    /// narrow, load-bearing thing: dying never *earns* credit.
    pub fn observe(&mut self, dir: Direction, reward: f32) {
        let idx = dir_index(dir);
        for i in 0..4 {
            self.tally[i] *= PRIOR_DECAY;
        }
        if reward > 0.0 {
            self.tally[idx] += 1.0 + reward;
        }
    }

    pub fn record_player_move(&mut self, dir: Direction) {
        self.player_tail.push_back(dir);
        while self.player_tail.len() > self.tail_len {
            self.player_tail.pop_front();
        }
    }

    pub fn remember(&mut self, vector: [f32; CPU_FEATURE_DIM], dir: Direction, reward: f32) {
        let seq = self.cpu_seq;
        self.cpu_seq += 1;
        self.episodes.push_back(CpuEpisode {
            vector,
            surviving_dir: dir,
            reward,
            seq,
        });
        while self.episodes.len() > MAX_EPISODES {
            self.episodes.pop_front();
        }
        self.observe(dir, reward);
    }

    /// Opponent-model hit rate over the session (0.0 before any prediction
    /// has been scored). The honest gauge of whether the player model is
    /// learning anything — watch it climb past the 25% chance floor.
    pub fn opp_pred_accuracy(&self) -> f32 {
        if self.opp_pred_total == 0 {
            0.0
        } else {
            self.opp_pred_hits as f32 / self.opp_pred_total as f32
        }
    }

    /* -------------------- persistence (per-player memory) -------------------- */

    /// Serialize the whole brain (both k-NN memories, priors, tail, ensemble
    /// scores, prediction bookkeeping). This is the per-player corpus: saved
    /// to a local file on native, to IndexedDB in the browser (keyed by the
    /// deviceId cookie).
    ///
    /// Written in the SECTIONED "WRM2" format — see [`BRAIN_MAGIC_V2`]. Each
    /// section is independently length-framed and independently decoded, so a
    /// schema change that invalidates one section (say, the survival feature
    /// space) never costs the player the others (their habit priors, their
    /// head-to-head prediction record). That partial-migration guarantee is
    /// the whole point: this game's premise is that the CPU learns YOU over
    /// many matches, so a version bump must never silently reset that.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut sections: Vec<(u16, Vec<u8>)> = Vec::new();

        push_section(
            &mut sections,
            SEC_CPU_CORE,
            &CpuCoreWire {
                cpu_seq: self.cpu_seq,
                tally: self.tally,
                player_tail: self.player_tail.iter().copied().collect(),
                tail_len: self.tail_len as u32,
            },
        );
        push_section(
            &mut sections,
            SEC_CPU_EPISODES,
            &EpisodesWire {
                dim: CPU_FEATURE_DIM as u16,
                items: self
                    .episodes
                    .iter()
                    .map(|e| EpisodeWire {
                        v: e.vector.to_vec(),
                        d: e.surviving_dir,
                        r: e.reward,
                        s: e.seq,
                    })
                    .collect(),
            },
        );
        push_section(
            &mut sections,
            SEC_OPP_CORE,
            &OppCoreWire {
                seq: self.opp_brain.seq,
                tally: self.opp_brain.tally,
                pred_hits: self.opp_pred_hits,
                pred_total: self.opp_pred_total,
            },
        );
        push_section(
            &mut sections,
            SEC_OPP_EPISODES,
            &EpisodesWire {
                dim: PLAYER_FEATURE_DIM as u16,
                items: self
                    .opp_brain
                    .episodes
                    .iter()
                    .map(|e| EpisodeWire {
                        v: e.vector.to_vec(),
                        d: e.next_dir,
                        r: 0.0,
                        s: e.seq,
                    })
                    .collect(),
            },
        );
        push_section(&mut sections, SEC_ENSEMBLE, &self.ensemble);
        push_section(&mut sections, SEC_READ_RATE, &self.lifetime_read);
        push_section(&mut sections, SEC_TURN_PRIOR, &self.opp_brain.turn_tally);

        let mut out = BRAIN_MAGIC_V2.to_le_bytes().to_vec();
        out.extend(BRAIN_FORMAT_V2.to_le_bytes());
        out.extend((sections.len() as u16).to_le_bytes());
        for (tag, body) in &sections {
            out.extend(tag.to_le_bytes());
            out.extend((body.len() as u32).to_le_bytes());
            out.extend(body);
        }
        out
    }

    /// Inverse of [`to_bytes`](Self::to_bytes). Reads both the sectioned
    /// "WRM2" format and the legacy single-blob "WRM1" format. `None` only
    /// when the bytes are not a brain at all — a brain that is merely STALE
    /// is migrated, never discarded.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        Self::from_bytes_report(bytes).map(|(brain, _)| brain)
    }

    /// [`from_bytes`](Self::from_bytes) plus an account of what survived the
    /// migration, so the UI can tell the player what was kept rather than
    /// silently resetting their opponent.
    pub fn from_bytes_report(bytes: &[u8]) -> Option<(Self, BrainRestore)> {
        if bytes.len() < 4 {
            return None;
        }
        let magic = u32::from_le_bytes(bytes[0..4].try_into().ok()?);
        let (mut brain, mut report) = match magic {
            BRAIN_MAGIC_V1 => Self::from_bytes_v1(&bytes[4..]),
            BRAIN_MAGIC_V2 => Self::from_bytes_v2(&bytes[4..]),
            _ => None,
        }?;
        brain.sanitize(&mut report);
        Some((brain, report))
    }

    /// Bring a decoded brain back into its invariants.
    ///
    /// The framing is bounds-checked, but a structurally VALID blob can still
    /// carry values the rest of the code assumes away. IndexedDB contents are
    /// editable by anyone with devtools, and a corrupt write is possible
    /// without any malice at all. The failure this guards is nastier than a
    /// crash: a NaN round-trips intact, `prior_distribution` then returns
    /// `[NaN; 4]`, every comparison goes false, argmax degrades to a constant,
    /// and the CPU becomes *silently* useless — no panic, nothing reported,
    /// just an opponent that stopped thinking.
    ///
    /// Same philosophy as the section decoder: drop the unusable part, keep
    /// everything else, and report what was dropped.
    fn sanitize(&mut self, report: &mut BrainRestore) {
        let before_cpu = self.episodes.len();
        self.episodes
            .retain(|e| e.vector.iter().all(|v| v.is_finite()) && e.reward.is_finite());
        report.cpu_episodes_dropped += before_cpu - self.episodes.len();

        let before_opp = self.opp_brain.episodes.len();
        self.opp_brain
            .episodes
            .retain(|e| e.vector.iter().all(|v| v.is_finite()));
        report.opp_episodes_dropped += before_opp - self.opp_brain.episodes.len();

        // A non-finite or negative tally poisons every prior that reads it.
        // Zeroing yields a uniform prior, which is inert rather than wrong.
        for tally in [&mut self.tally, &mut self.opp_brain.tally] {
            if tally.iter().any(|v| !v.is_finite() || *v < 0.0) {
                *tally = [0.0; 4];
            }
        }

        // `record_player_move` trims to tail_len; a huge value makes the trim
        // loop never fire and the tail grow without bound, a zero makes the
        // pattern models silently blind.
        self.tail_len = self.tail_len.clamp(1, 16);
        while self.player_tail.len() > self.tail_len {
            self.player_tail.pop_front();
        }

        // Accuracy is hits/total; hits > total reports above 100%.
        self.opp_pred_hits = self.opp_pred_hits.min(self.opp_pred_total);

        if self.ensemble.active >= ENSEMBLE_MODELS {
            self.ensemble.active = 0;
        }
        for v in self
            .ensemble
            .num
            .iter_mut()
            .chain(self.ensemble.den.iter_mut())
        {
            if !v.is_finite() {
                *v = 0.0;
            }
        }
        if !self.ensemble.confidence.is_finite() {
            self.ensemble.confidence = 0.0;
        }
    }

    /// Legacy "WRM1": magic + one bincode blob of the whole struct. Readable
    /// only while `CpuBrain`'s shape is unchanged — which is precisely why
    /// WRM2 exists. Every load rewrites as WRM2 (the game always saves in the
    /// current format), so this path drains as players return.
    fn from_bytes_v1(payload: &[u8]) -> Option<(Self, BrainRestore)> {
        let brain: Self = bincode::deserialize(payload).ok()?;
        let report = BrainRestore {
            format: 1,
            cpu_episodes_kept: brain.episodes.len(),
            cpu_episodes_dropped: 0,
            opp_episodes_kept: brain.opp_brain.episodes.len(),
            opp_episodes_dropped: 0,
            ensemble_kept: true,
            sections_skipped: 0,
        };
        Some((brain, report))
    }

    /// Sectioned "WRM2". Decodes section-by-section: an unknown tag, a
    /// corrupt body, or a feature-space mismatch costs only that section.
    fn from_bytes_v2(payload: &[u8]) -> Option<(Self, BrainRestore)> {
        if payload.len() < 4 {
            return None;
        }
        let format = u16::from_le_bytes(payload[0..2].try_into().ok()?);
        if format != BRAIN_FORMAT_V2 {
            // A NEWER format than this build understands. Section framing is
            // stable across format revisions by construction, so keep going
            // and let per-section decoding drop whatever we cannot read.
        }
        let count = u16::from_le_bytes(payload[2..4].try_into().ok()?) as usize;

        let mut brain = Self::default();
        let mut report = BrainRestore {
            format: 2,
            ..BrainRestore::default()
        };

        let mut off = 4usize;
        for _ in 0..count {
            if off + 6 > payload.len() {
                break; // truncated frame — keep what we already decoded
            }
            let tag = u16::from_le_bytes(payload[off..off + 2].try_into().ok()?);
            let len = u32::from_le_bytes(payload[off + 2..off + 6].try_into().ok()?) as usize;
            off += 6;
            let end = off.saturating_add(len);
            if end > payload.len() {
                break;
            }
            let body = &payload[off..end];
            off = end;

            match tag {
                SEC_CPU_CORE => match bincode::deserialize::<CpuCoreWire>(body) {
                    Ok(c) => {
                        brain.cpu_seq = c.cpu_seq;
                        brain.tally = c.tally;
                        brain.player_tail = c.player_tail.into_iter().collect();
                        brain.tail_len = c.tail_len as usize;
                    }
                    Err(_) => report.sections_skipped += 1,
                },
                SEC_CPU_EPISODES => match bincode::deserialize::<EpisodesWire>(body) {
                    // The survival corpus is bound to the feature encoding.
                    // A dimension change makes these vectors meaningless, so
                    // they go — but nothing else does.
                    Ok(e) if e.dim as usize == CPU_FEATURE_DIM => {
                        for it in e.items {
                            let Ok(vector) = <[f32; CPU_FEATURE_DIM]>::try_from(it.v.as_slice())
                            else {
                                report.cpu_episodes_dropped += 1;
                                continue;
                            };
                            brain.episodes.push_back(CpuEpisode {
                                vector,
                                surviving_dir: it.d,
                                reward: it.r,
                                seq: it.s,
                            });
                        }
                        while brain.episodes.len() > MAX_EPISODES {
                            brain.episodes.pop_front();
                        }
                        report.cpu_episodes_kept = brain.episodes.len();
                    }
                    Ok(e) => report.cpu_episodes_dropped = e.items.len(),
                    Err(_) => report.sections_skipped += 1,
                },
                SEC_OPP_CORE => match bincode::deserialize::<OppCoreWire>(body) {
                    // The habit priors and the head-to-head record are about
                    // the HUMAN, not about any encoding — these must survive
                    // every schema change we ever make.
                    Ok(c) => {
                        brain.opp_brain.seq = c.seq;
                        brain.opp_brain.tally = c.tally;
                        brain.opp_pred_hits = c.pred_hits;
                        brain.opp_pred_total = c.pred_total;
                    }
                    Err(_) => report.sections_skipped += 1,
                },
                SEC_OPP_EPISODES => match bincode::deserialize::<EpisodesWire>(body) {
                    Ok(e) if e.dim as usize == PLAYER_FEATURE_DIM => {
                        for it in e.items {
                            let Ok(vector) = <[f32; PLAYER_FEATURE_DIM]>::try_from(it.v.as_slice())
                            else {
                                report.opp_episodes_dropped += 1;
                                continue;
                            };
                            brain.opp_brain.episodes.push_back(PlayerEpisode {
                                vector,
                                next_dir: it.d,
                                seq: it.s,
                            });
                        }
                        while brain.opp_brain.episodes.len() > MAX_EPISODES {
                            brain.opp_brain.episodes.pop_front();
                        }
                        report.opp_episodes_kept = brain.opp_brain.episodes.len();
                    }
                    Ok(e) => report.opp_episodes_dropped = e.items.len(),
                    Err(_) => report.sections_skipped += 1,
                },
                SEC_ENSEMBLE => match bincode::deserialize::<Ensemble>(body) {
                    Ok(e) => {
                        brain.ensemble = e;
                        report.ensemble_kept = true;
                    }
                    Err(_) => report.sections_skipped += 1,
                },
                SEC_TURN_PRIOR => match bincode::deserialize::<[f32; TURNS]>(body) {
                    Ok(t) => brain.opp_brain.turn_tally = t,
                    Err(_) => report.sections_skipped += 1,
                },
                SEC_READ_RATE => match bincode::deserialize::<ReadRate>(body) {
                    Ok(r) => brain.lifetime_read = r,
                    Err(_) => report.sections_skipped += 1,
                },
                // Forward compatibility: a section this build has never heard
                // of is skipped by its length, not treated as corruption.
                _ => report.sections_skipped += 1,
            }
        }

        Some((brain, report))
    }

    /// Load a brain from a file (native terminal play).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_file(path: &std::path::Path) -> Option<Self> {
        let bytes = std::fs::read(path).ok()?;
        Self::from_bytes(&bytes)
    }

    /// Save the brain to a file (native terminal play).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn save_file(&self, path: &std::path::Path) -> std::io::Result<()> {
        std::fs::write(path, self.to_bytes())
    }
}

/* ----------------------- brain wire format (WRM2) ----------------------- */

/// Legacy format: `"WRM1"` + one bincode blob of the whole `CpuBrain`. Any
/// change to the struct made every saved brain unreadable — the reason this
/// module now writes [`BRAIN_MAGIC_V2`] instead. Still READ so returning
/// players keep their corpus; they are upgraded on their next save.
const BRAIN_MAGIC_V1: u32 = 0x31524D57;
/// Current format: `"WRM2"` + `[format:u16][section_count:u16]` followed by
/// `[tag:u16][len:u32][body]` frames.
///
/// Sections are decoded INDEPENDENTLY. A section whose tag is unknown, whose
/// body is corrupt, or whose feature dimension no longer matches this build is
/// skipped — and only that section is lost. That is what lets the survival
/// feature space evolve (adding power-up awareness, say) while the player's
/// habit priors and head-to-head record carry forward untouched.
///
/// Because the frame header is fixed-width and length-prefixed, a NEWER writer
/// can add sections an older reader has never seen and the older reader will
/// skip them cleanly. Bump [`BRAIN_FORMAT_V2`] only for a change to the
/// FRAMING itself; adding a section does not require it.
const BRAIN_MAGIC_V2: u32 = 0x32524D57;
const BRAIN_FORMAT_V2: u16 = 2;

/// Survival-memory scalars: monotonic seq, direction prior, player tail ring.
const SEC_CPU_CORE: u16 = 1;
/// Survival k-NN corpus. Bound to `CPU_FEATURE_DIM` — dropped when it changes.
const SEC_CPU_EPISODES: u16 = 2;
/// Opponent-model scalars: seq, habit prior, lifetime prediction record.
/// Encoding-independent — this is knowledge about the HUMAN and must never be
/// dropped by a schema change.
const SEC_OPP_CORE: u16 = 3;
/// Opponent k-NN corpus. Bound to `PLAYER_FEATURE_DIM`.
const SEC_OPP_EPISODES: u16 = 4;
/// The rps-ai ensemble scores.
const SEC_ENSEMBLE: u16 = 5;
/// Lifetime read record. Its own section rather than a new field on an
/// existing wire struct: `bincode` is not field-tolerant, so widening
/// `OppCoreWire` would make every saved blob fail to decode and cost the
/// player exactly the habit priors that section exists to protect.
/// Encoding-independent — this is knowledge about the human and must survive
/// every future feature-space change.
const SEC_READ_RATE: u16 = 6;
/// The player's relative-turn habit. Its own section because it is knowledge
/// about the HUMAN and encoding-independent — and because it was silently NOT
/// persisted at all: `OppCoreWire` carries seq/tally/prediction counts and
/// nothing else, so the one statistic that makes a heading-relative habit
/// learnable was forgotten between sessions. For a game whose premise is "it
/// remembers you", that is the worst possible thing to drop.
const SEC_TURN_PRIOR: u16 = 7;

/// One `(vector, direction, reward, seq)` row. The vector is a length-prefixed
/// `Vec<f32>` rather than a fixed array precisely so a dimension change is a
/// readable mismatch instead of a decode failure that eats the whole file.
#[derive(serde::Serialize, serde::Deserialize)]
struct EpisodeWire {
    v: Vec<f32>,
    d: Direction,
    /// Unused (always 0.0) for opponent episodes, which carry no reward.
    r: f32,
    s: u32,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct EpisodesWire {
    dim: u16,
    items: Vec<EpisodeWire>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CpuCoreWire {
    cpu_seq: u32,
    tally: [f32; 4],
    player_tail: Vec<Direction>,
    tail_len: u32,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct OppCoreWire {
    seq: u32,
    tally: [f32; 4],
    pred_hits: u32,
    pred_total: u32,
}

/// Encode one section and append it. A section that fails to serialize is
/// omitted rather than poisoning the whole save.
fn push_section<T: serde::Serialize>(out: &mut Vec<(u16, Vec<u8>)>, tag: u16, value: &T) {
    if let Ok(body) = bincode::serialize(value) {
        out.push((tag, body));
    }
}

/// What survived a brain restore. Surfaced to the UI so a player can be told
/// "your opponent still remembers you" instead of silently meeting a blank AI.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BrainRestore {
    /// Wire format the blob was written in (1 = legacy WRM1, 2 = WRM2).
    pub format: u8,
    pub cpu_episodes_kept: usize,
    pub cpu_episodes_dropped: usize,
    pub opp_episodes_kept: usize,
    pub opp_episodes_dropped: usize,
    pub ensemble_kept: bool,
    /// Sections skipped as unknown, corrupt, or unserializable.
    pub sections_skipped: usize,
}

impl BrainRestore {
    /// True when any learned state had to be discarded to load this brain.
    pub fn is_partial(&self) -> bool {
        self.cpu_episodes_dropped > 0 || self.opp_episodes_dropped > 0 || self.sections_skipped > 0
    }

    /// One-line status for the HUD / brain panel.
    pub fn summary(&self) -> String {
        if !self.is_partial() {
            return format!(
                "brain restored — {} survival, {} opponent episodes",
                self.cpu_episodes_kept, self.opp_episodes_kept
            );
        }
        format!(
            "brain migrated — kept {} opponent episodes, reset {} survival episodes",
            self.opp_episodes_kept, self.cpu_episodes_dropped
        )
    }
}

#[inline]
fn dir_index(dir: Direction) -> usize {
    match dir {
        Direction::Up => 0,
        Direction::Down => 1,
        Direction::Left => 2,
        Direction::Right => 3,
    }
}

#[inline]
fn index_dir(i: usize) -> Direction {
    match i {
        0 => Direction::Up,
        1 => Direction::Down,
        2 => Direction::Left,
        _ => Direction::Right,
    }
}

/// Whether stepping one cell in `dir` from `(hx,hy)` is free and in-bounds.
/// Delegates to `WormGame::passable` so the AI's view of legality matches the
/// physics exactly (Empty | Food | Hole | PowerUp; bombs and out-of-bounds
/// fatal). Previously this used its own bounds + Empty|Food check, which read
/// punched holes and power-up cells as illegal and bomb cells as safe.
pub fn free_step(game: &WormGame, hx: u16, hy: u16, dir: Direction) -> bool {
    let (dx, dy) = dir.as_delta();
    let nx = hx as i16 + dx;
    let ny = hy as i16 + dy;
    if nx < 0 || ny < 0 || nx >= game.width as i16 || ny >= game.height as i16 {
        return false;
    }
    game.passable(nx as u16, ny as u16)
}

/// BFS flood-fill open space — the survival prior. The k-NN vote must beat this.
/// Counts every occupiable cell (Empty | Food | Hole | PowerUp) with no
/// artificial cap, and can flow through punched holes into the outer corridor.
/// Previously this capped at 2000 (~44% of a 120×38 board) and hard-clipped to
/// the arena interior, so `norm_open` was systematically under-reported.
pub fn count_open_space(game: &WormGame, start_x: u16, start_y: u16) -> f32 {
    let mut visited = vec![vec![false; game.width as usize]; game.height as usize];
    let mut queue: VecDeque<(u16, u16)> = VecDeque::new();
    queue.push_back((start_x, start_y));
    visited[start_y as usize][start_x as usize] = true;
    let mut count = 0.0;
    let max_cells = game.width as f32 * game.height as f32;
    let neighbors = [(0i16, -1i16), (0, 1), (-1, 0), (1, 0)];
    while let Some((x, y)) = queue.pop_front() {
        count += 1.0;
        if count >= max_cells {
            break;
        }
        for (dx, dy) in &neighbors {
            let nx = x as i16 + dx;
            let ny = y as i16 + dy;
            if nx < 0 || ny < 0 || nx >= game.width as i16 || ny >= game.height as i16 {
                continue;
            }
            let (nx, ny) = (nx as u16, ny as u16);
            if !visited[ny as usize][nx as usize]
                && matches!(
                    game.grid[ny as usize][nx as usize],
                    CellType::Empty | CellType::Food | CellType::Hole | CellType::PowerUp
                )
            {
                visited[ny as usize][nx as usize] = true;
                queue.push_back((nx, ny));
            }
        }
    }
    count
}

/// BFS from (sx,sy) to the nearest collectible (food or power-up). Returns
/// (direction_to_take, open_space_at_item). Only considers legal directions
/// from the start, and checks that the item cell is reachable.
fn bfs_nearest_food_dir(
    game: &WormGame,
    sx: u16,
    sy: u16,
    legal: &[Direction],
) -> Option<(Direction, f32)> {
    if game.food_items.is_empty() && game.powerups.is_empty() {
        return None;
    }

    let mut visited = vec![vec![false; game.width as usize]; game.height as usize];
    let mut queue: VecDeque<(u16, u16, Direction)> = VecDeque::new();

    // Seed the queue with all legal first steps.
    for &d in legal {
        let (dx, dy) = d.as_delta();
        let nx = sx as i16 + dx;
        let ny = sy as i16 + dy;
        if nx < 0 || ny < 0 || nx >= game.width as i16 || ny >= game.height as i16 {
            continue;
        }
        let (nx, ny) = (nx as u16, ny as u16);
        if (nx, ny) != (sx, sy) && !visited[ny as usize][nx as usize] {
            visited[ny as usize][nx as usize] = true;
            queue.push_back((nx, ny, d));
        }
    }

    let dirs = [(0i16, -1i16), (0, 1), (-1, 0), (1, 0)];

    while let Some((x, y, start_dir)) = queue.pop_front() {
        // Check if this cell is a collectible (food or power-up).
        if matches!(
            game.grid[y as usize][x as usize],
            CellType::Food | CellType::PowerUp
        ) {
            let open = count_open_space(game, x, y);
            return Some((start_dir, open));
        }

        // Expand neighbors: travel through any occupiable non-food cell
        // (walls and trails block; holes and power-ups are walkable).
        for (dx, dy) in &dirs {
            let nx = x as i16 + dx;
            let ny = y as i16 + dy;
            if nx < 0 || ny < 0 || nx >= game.width as i16 || ny >= game.height as i16 {
                continue;
            }
            let (nx, ny) = (nx as u16, ny as u16);
            if !visited[ny as usize][nx as usize]
                && matches!(
                    game.grid[ny as usize][nx as usize],
                    CellType::Empty | CellType::Hole | CellType::PowerUp
                )
            {
                visited[ny as usize][nx as usize] = true;
                queue.push_back((nx, ny, start_dir));
            }
        }
    }
    None
}

/// Per-direction Manhattan distance to the nearest food, in the half-plane
/// the direction faces: `along + perp` for targets ahead, `cap` for targets
/// behind or exactly perpendicular. The old code clamped the projection at 0,
/// so food BEHIND the head read as distance 0 ("right here") in the opposite
/// direction, and off-axis food ignored its perpendicular offset entirely.
fn nearest_food_distance(game: &WormGame, hx: u16, hy: u16, cap: f32) -> [f32; 4] {
    let dirs = [
        Direction::Up,
        Direction::Down,
        Direction::Left,
        Direction::Right,
    ];
    let mut out = [cap; 4];
    for (i, d) in dirs.iter().enumerate() {
        let (dx, dy) = d.as_delta();
        let mut best = cap;
        for f in &game.food_items {
            let nx = (f.0 as i16 - hx as i16) as f32;
            let ny = (f.1 as i16 - hy as i16) as f32;
            let along = nx * dx as f32 + ny * dy as f32;
            if along <= 0.0 {
                continue; // behind or exactly perpendicular — not this way
            }
            let perp = (nx * dy as f32 - ny * dx as f32).abs();
            let dist = along + perp;
            if dist < best {
                best = dist;
            }
        }
        out[i] = best;
    }
    out
}

/// Per-direction distance to the player head (for kill pursuit awareness).
/// Same half-plane metric as `nearest_food_distance`; the old projection read
/// a player directly behind as distance 0 in the forward direction.
fn directional_player_distance(game: &WormGame, hx: u16, hy: u16, cap: f32) -> [f32; 4] {
    let dirs = [
        Direction::Up,
        Direction::Down,
        Direction::Left,
        Direction::Right,
    ];
    let ph = game.cycles[0].head;
    let mut out = [cap; 4];
    for (i, d) in dirs.iter().enumerate() {
        let (dx, dy) = d.as_delta();
        let nx = (ph.0 as i16 - hx as i16) as f32;
        let ny = (ph.1 as i16 - hy as i16) as f32;
        let along = nx * dx as f32 + ny * dy as f32;
        if along <= 0.0 {
            continue;
        }
        let perp = (nx * dy as f32 - ny * dx as f32).abs();
        out[i] = (along + perp).min(cap);
    }
    out
}

/// Distance to the nearest wall per direction, stopping at the first Wall
/// grid cell. The old version never looked at the grid and counted through
/// the ring-2 arena wall into the outer corridor, over-reporting by 2+ in
/// every direction on any terminal big enough to have a corridor.
fn wall_distance(game: &WormGame, hx: u16, hy: u16) -> [f32; 4] {
    let dirs = [
        Direction::Up,
        Direction::Down,
        Direction::Left,
        Direction::Right,
    ];
    let mut out = [0.0f32; 4];
    for (i, d) in dirs.iter().enumerate() {
        let (dx, dy) = d.as_delta();
        let mut dist = 0.0;
        let mut x = hx as i16;
        let mut y = hy as i16;
        loop {
            x += dx;
            y += dy;
            if x < 0 || y < 0 || x as u16 >= game.width || y as u16 >= game.height {
                break;
            }
            if game.grid[y as usize][x as usize] == CellType::Wall {
                break;
            }
            dist += 1.0;
        }
        out[i] = dist;
    }
    out
}

/// Encode the CPU's local situation into a fixed feature vector — the faithful
/// analog of `feature-embed.ts:embedContext` + `rps.ts:buildContext`.
///
/// Slots (fixed width → cosine is meaningful), with a phase-depth block so the
/// all-zero "new game" situation is not distance 1.0 to everything (the rps-ai
/// zero-vector trap):
///   0..4    open neighbour one-hot {Up,Down,Left,Right}
///   4..8    wall distance per direction, normalised by arena diagonal
///   8..12   nearest-own-trail distance per direction (binned 0..6)
///  12..16   nearest-food distance per direction (binned 0..6)
///  16..20   player-head distance per direction (binned 0..6)
///  20..24   current travel direction one-hot
///  24      phase-depth (frames played / 200, clamped)
pub fn encode_situation(game: &WormGame, brain: &CpuBrain) -> [f32; CPU_FEATURE_DIM] {
    let mut vector = [0.0f32; CPU_FEATURE_DIM];
    let cpu = &game.cycles[1];
    let (hx, hy) = cpu.head;

    // 0..4 open neighbour one-hot
    let dirs = [
        Direction::Up,
        Direction::Down,
        Direction::Left,
        Direction::Right,
    ];
    for (i, &d) in dirs.iter().enumerate() {
        vector[i] = if free_step(game, hx, hy, d) { 1.0 } else { 0.0 };
    }

    // 4..8 wall distance normalised by arena diagonal
    let diag = ((game.width as f32).hypot(game.height as f32)).max(1.0);
    let walls = wall_distance(game, hx, hy);
    for i in 0..4 {
        vector[4 + i] = (walls[i] / diag).min(1.0);
    }

    // 8..12 nearest own-trail distance per direction (binned to 6)
    let trail = nearest_trail_distance(game, hx, hy, 6.0);
    for i in 0..4 {
        vector[8 + i] = trail[i] / 6.0;
    }

    // 12..16 nearest food distance per direction (binned to 6)
    let food = nearest_food_distance(game, hx, hy, 6.0);
    for i in 0..4 {
        vector[12 + i] = food[i] / 6.0;
    }

    // 16..20 player head distance per direction (binned to 6)
    let player = directional_player_distance(game, hx, hy, 6.0);
    for i in 0..4 {
        vector[16 + i] = player[i] / 6.0;
    }

    // 20..24 current travel direction one-hot
    vector[20 + dir_index(cpu.direction)] = 1.0;

    // 24 phase depth
    vector[24] = (game.frame_count as f32 / 200.0).min(1.0);

    // L2-normalise so cosine is 1 − dot, like rps-ai.
    let mut norm = vector.iter().map(|value| value * value).sum::<f32>();
    norm = norm.sqrt();
    if norm > 0.0 {
        let inv = 1.0 / norm;
        for value in &mut vector {
            *value *= inv;
        }
    }
    // silence the unused-parameter warning on `brain` — the encoder is pure
    // but kept signature-compatible with rps-ai's context builder which takes history.
    let _ = brain;
    vector
}

/// Encode the **player's** local situation — the input to the opponent model.
///
/// Layout (fixed width → cosine is meaningful):
///   0..4    player open-neighbour one-hot {Up,Down,Left,Right}
///   4..8    distance to player's own trail per direction (binned 0..6)
///   8..12   distance from player head toward nearest food per direction
///  12      player→CPU proximity (binned 0..12, inverted: near = high)
///  13..29  4×4 direction-transition matrix (see below); 29..32 zero-padded.
///
/// Phase depth is intentionally omitted here because the player's *intent*
/// does not depend on the clock — it depends on topology. We rely on the
/// global `WormGame::frame_count` implicitly via episode `seq` for recency.
/// Encode a player-centric situation vector. The player's recent direction
/// history (from `player_tail`) is encoded as a 4×4 transition matrix in slots
/// 13..29: (prev_dir → curr_dir), capturing corner behaviour. This is the
/// order-matters analogue of rps-ai's `bg` bigram block.
pub fn encode_player_context(
    game: &WormGame,
    tail: &VecDeque<Direction>,
) -> [f32; PLAYER_FEATURE_DIM] {
    let mut vector = [0.0f32; PLAYER_FEATURE_DIM];
    let player = &game.cycles[0];
    let (hx, hy) = player.head;
    let heading = player.prev_direction;

    // Everything below is expressed RELATIVE to the player's heading wherever
    // it describes a choice, because a habit is heading-relative: "breaks left
    // when cornered" is one pattern here and four unrelated ones in compass
    // space. The absolute blocks that remain are the ones where absoluteness is
    // the point (which way they like to travel, where the walls are).
    let turn_dirs = [
        Turn::Straight.apply(heading),
        Turn::Left.apply(heading),
        Turn::Right.apply(heading),
    ];

    // 0..3 CAN I GO THIS WAY — separates a forced break from a chosen one.
    for (i, &d) in turn_dirs.iter().enumerate() {
        vector[i] = if free_step(game, hx, hy, d) { 1.0 } else { 0.0 };
    }

    // 3..6 RUNWAY per option: how far before something stops me.
    // Lets the model express "turns when the wall is three cells away", which
    // a single global turn prior can never represent.
    let trail = nearest_trail_distance(game, hx, hy, 6.0);
    for (i, &d) in turn_dirs.iter().enumerate() {
        vector[3 + i] = trail[dir_index(d)] / 6.0;
    }

    // 6..9 FOOD bearing per option — "breaks toward food unless threatened".
    //
    // The cap is 24, not 6. At a 6-cell cap on a 120x38 board with at most
    // five morsels, the measured value was the cap on essentially every frame
    // — three constant slots eating 17% of the vector's energy while carrying
    // zero information. A feature that never varies is worse than no feature:
    // it dilutes every real one.
    let food = nearest_food_distance(game, hx, hy, 24.0);
    for (i, &d) in turn_dirs.iter().enumerate() {
        vector[6 + i] = food[dir_index(d)] / 24.0;
    }

    // 9..13 WHERE IS THE CPU, in the player's own frame: ahead, behind, to
    // their left, to their right. "Turns away when the CPU is behind them" is
    // a real habit and was previously inexpressible — the old vector had a
    // single undirected proximity scalar.
    let (cx, cy) = game.cycles[1].head;
    let ox = cx as f32 - hx as f32;
    let oy = cy as f32 - hy as f32;
    let (fx, fy) = heading.as_delta();
    let (rx, ry) = right_turn(heading).as_delta();
    let ahead = ox * fx as f32 + oy * fy as f32;
    let rightward = ox * rx as f32 + oy * ry as f32;
    let bearing = if ahead.abs() >= rightward.abs() {
        if ahead >= 0.0 { 0 } else { 1 }
    } else if rightward >= 0.0 { 2 } else { 3 };
    vector[9 + bearing] = 1.0;

    // 13 CPU PROXIMITY — how much pressure they are under.
    let manhattan = ox.abs() + oy.abs();
    vector[13] = ((12.0 - manhattan).max(0.0)) / 12.0;

    // 14 IS THE CPU CLOSING? Radial relative velocity — the rate the gap is
    // actually shrinking, in cells/frame along the line between the heads.
    // The first version projected only the CPU's velocity and divided by a
    // constant, which made it distance-scaled alignment: approaching at one
    // cell/frame read as strong from twelve cells and weak from one, and
    // matched-velocity motion at constant separation read as "closing".
    let (cfx, cfy) = game.cycles[1].direction.as_delta();
    let (pfx, pfy) = heading.as_delta();
    let (rvx, rvy) = (cfx as f32 - pfx as f32, cfy as f32 - pfy as f32);
    let dist = (ox * ox + oy * oy).sqrt().max(1.0);
    let closing = -((ox * rvx + oy * rvy) / dist); // + = gap shrinking
    vector[14] = (closing / 2.0).clamp(-1.0, 1.0) * 0.5 + 0.5;

    // 15..19 PLAYER HEADING one-hot. Keeps absolute compass habits learnable
    // ("this player likes going Up") now that everything else went relative —
    // dropping it would trade one blindness for another.
    vector[15 + dir_index(heading)] = 1.0;

    // 19 HOW BOXED IN ARE THEY — arena configuration as they experience it.
    let total = (game.width as f32) * (game.height as f32);
    vector[19] = (count_open_space(game, hx, hy) / total).min(1.0);

    // 20 OWN LENGTH — a long player plays differently from a short one, and
    // it is the quantity that decides whether they can afford a tight turn.
    vector[20] = ((player.positions.len() as f32) / 60.0).min(1.0);

    // 21 SPEED. The game accelerates as food is eaten, and a human at 35ms
    // per cell is a different opponent from the same human at 115ms — they
    // stop reacting and start running pre-planned routes.
    vector[21] = ((115.0 - game.frame_delay().as_millis() as f32) / 80.0).clamp(0.0, 1.0);

    // 22 SUDDEN-DEATH PRESSURE — the arena is closing in.
    let max_level = game.sudden_death_max_level().max(1) as f32;
    vector[22] = (game.shrink_level as f32 / max_level).min(1.0);

    // 23 WHAT ARE THEY HOLDING — not merely whether.
    //
    // The human's HUD shows what the CPU is carrying, so a CPU blind to what
    // the HUMAN carries is starved of information its opponent has, and will
    // be outsmarted for it. People also play measurably differently armed:
    // bolder with a laser, more evasive with nothing.
    vector[23] = match player.held_powerup {
        None => 0.0,
        Some(crate::game::PowerUpKind::Laser) => 1.0 / 3.0,
        Some(crate::game::PowerUpKind::TriShot) => 2.0 / 3.0,
        Some(crate::game::PowerUpKind::Bomb) => 1.0,
    };

    // 24 IS THERE A MINE NEAR THEM — disguised as food, so their reaction to
    // one is exactly the "do they take bait" habit worth learning.
    let mine_near = game
        .bombs
        .iter()
        .map(|b| (b.x as f32 - hx as f32).abs().max((b.y as f32 - hy as f32).abs()))
        .fold(f32::INFINITY, f32::min);
    vector[24] = if mine_near.is_finite() {
        ((12.0 - mine_near).max(0.0)) / 12.0
    } else {
        0.0
    };

    // 25..28 RECENT TURN MIX, and 28..31 the LAST turn.
    //
    // Replaces a 4x4 matrix over absolute directions, which smeared one
    // relative habit across sixteen cells and — despite its comment — never
    // actually carried order, since it was a bag of pair counts. Turn space
    // needs a ninth of the room to say more, and the explicit last-turn
    // one-hot is what lets an alternator ("left, right, left") be recognised
    // at all.
    let tail_vec: Vec<Direction> = tail.iter().copied().collect();
    let mut last_turn = None;
    let mut pairs = 0.0f32;
    for w in tail_vec.windows(2) {
        if let Some(t) = Turn::from_dirs(w[0], w[1]) {
            vector[25 + turn_index(t)] += 1.0;
            pairs += 1.0;
            last_turn = Some(t);
        }
    }
    // Normalise the mix by its own mass. Raw counts reached 3.0 while every
    // other slot was <= 1.0, so after L2 this one block carried ~47% of the
    // vector's energy and cosine retrieval was mostly comparing tail lengths.
    if pairs > 0.0 {
        for i in 25..28 {
            vector[i] /= pairs;
        }
    }
    if let Some(t) = last_turn {
        vector[28 + turn_index(t)] = 1.0;
    }

    // L2-normalise
    let mut norm = vector.iter().map(|value| value * value).sum::<f32>();
    norm = norm.sqrt();
    if norm > 0.0 {
        let inv = 1.0 / norm;
        for value in &mut vector {
            *value *= inv;
        }
    }
    vector
}

fn nearest_trail_distance(game: &WormGame, hx: u16, hy: u16, max_range: f32) -> [f32; 4] {
    let dirs = [
        Direction::Up,
        Direction::Down,
        Direction::Left,
        Direction::Right,
    ];
    let mut out = [max_range; 4];
    for (i, d) in dirs.iter().enumerate() {
        let (dx, dy) = d.as_delta();
        let mut x = hx as i16;
        let mut y = hy as i16;
        let mut dist = 0.0;
        loop {
            x += dx;
            y += dy;
            if x < 0 || y < 0 || x as u16 >= game.width || y as u16 >= game.height {
                break;
            }
            let cell = game.grid[y as usize][x as usize];
            // Only solid obstacles count — holes and power-ups are passable,
            // so they must not read as "trail" for the distance feature.
            if matches!(cell, CellType::Wall | CellType::Player | CellType::CPU) {
                out[i] = dist;
                break;
            }
            dist += 1.0;
            if dist >= max_range {
                break;
            }
        }
    }
    out
}

/* ----------------------------- recall + vote ----------------------------- */

#[derive(Clone, Debug)]
pub struct Recalled {
    pub surviving_dir: Direction,
    pub seq: u32,
    pub distance: f32,
    /// Episode outcome (survival frames + food). 0 = the move died instantly.
    pub reward: f32,
}

/// Exact cosine k-NN scan (rps-ai `store.recall`).
pub fn recall(brain: &CpuBrain, query: &[f32; CPU_FEATURE_DIM], k: usize) -> Vec<Recalled> {
    let mut all: Vec<Recalled> = brain
        .episodes
        .iter()
        .map(|e| {
            let dot: f32 = e.vector.iter().zip(query.iter()).map(|(a, b)| a * b).sum();
            let distance = 1.0 - dot;
            Recalled {
                surviving_dir: e.surviving_dir,
                seq: e.seq,
                distance: distance.max(0.0),
                reward: e.reward,
            }
        })
        .collect();
    all.sort_by(|a, b| {
        a.distance
            .partial_cmp(&b.distance)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    all.truncate(k);
    all
}

/* ---------------------- Player-Model: recall + vote ---------------------- */

/// k-NN reasoning result for the opponent model.
#[derive(Debug)]
pub struct PlayerAggregate {
    pub distribution: [f32; 4],
    pub confidence: f32,
    pub margin: f32,
    pub support: f32,
    pub maturity: f32,
    pub prior_weight: f32,
    pub predicted_dir: Direction,
}

struct PlayerRec {
    next_dir: Direction,
    seq: u32,
    distance: f32,
}

fn recall_player(
    brain: &PlayerBrain,
    query: &[f32; PLAYER_FEATURE_DIM],
    k: usize,
) -> Vec<PlayerRec> {
    let mut all: Vec<PlayerRec> = brain
        .episodes
        .iter()
        .map(|e| {
            let dot: f32 = e.vector.iter().zip(query.iter()).map(|(a, b)| a * b).sum();
            PlayerRec {
                next_dir: e.next_dir,
                seq: e.seq,
                distance: (1.0 - dot).max(0.0),
            }
        })
        .collect();
    all.sort_by(|a, b| {
        a.distance
            .partial_cmp(&b.distance)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    all.truncate(k);
    all
}

fn argmax(distribution: &[f32; 4]) -> Direction {
    let mut best = 0;
    let mut best_val = distribution[0];
    for (i, &value) in distribution.iter().enumerate().skip(1) {
        if value > best_val {
            best_val = value;
            best = i;
        }
    }
    index_dir(best)
}

/// How much a recalled episode's label agrees with what the player has been
/// doing lately.
///
/// This used to count how often the episode's LABEL appeared anywhere in the
/// recent tail and call it a "trailing match". That is a label-frequency
/// bonus, not a sequence match: it duplicates the absolute prior, and it
/// rewards the answer before any contextual evidence has been weighed —
/// doubling the weight of whichever direction the player happens to use most.
///
/// Now it compares against the MOST RECENT move only, which is the one claim
/// a single stored label can actually support ("they just went this way, and
/// this memory says they go this way"). Weaker, and honest.
fn trailing_match_dir(a: &VecDeque<Direction>, ep: &PlayerRec) -> f32 {
    match a.back() {
        Some(&last) if last == ep.next_dir => 1.0,
        _ => 0.0,
    }
}

fn aggregate_player(
    brain: &PlayerBrain,
    recalled: &[PlayerRec],
    current_seq: u32,
    memory_size: usize,
    tail: &VecDeque<Direction>,
) -> PlayerAggregate {
    let prior = brain.prior_distribution();
    let prior_strength = brain.prior_strength();
    let maturity = (memory_size as f32 / COLD_START_EPISODES as f32).min(1.0);
    let cold = memory_size < COLD_START_EPISODES || recalled.is_empty();

    if cold {
        return PlayerAggregate {
            distribution: prior,
            confidence: 0.0,
            margin: 0.0,
            support: 0.0,
            maturity,
            prior_weight: 1.0,
            predicted_dir: argmax(&prior),
        };
    }

    let weights: Vec<f32> = recalled
        .iter()
        .map(|ep| {
            let proximity = 1.0 / (EPS + ep.distance * ep.distance);
            let age = (current_seq as i64 - ep.seq as i64).max(0) as u32;
            let recency = (-(age as f32 / DECAY_TAU)).exp();
            let trail = trailing_match_dir(tail, ep);
            proximity * recency * (1.0 + MATCH_BONUS * trail)
        })
        .collect();

    let total_weight: f32 = weights.iter().sum();
    if total_weight <= 0.0 {
        return PlayerAggregate {
            distribution: prior,
            confidence: 0.0,
            margin: 0.0,
            support: 0.0,
            maturity,
            prior_weight: 1.0,
            predicted_dir: argmax(&prior),
        };
    }

    let mut memory_vote = [0.0f32; 4];
    for (i, ep) in recalled.iter().enumerate() {
        memory_vote[dir_index(ep.next_dir)] += weights[i] / total_weight;
    }

    let sum_squares: f32 = weights.iter().map(|w| w * w).sum();
    let effective_n = (total_weight * total_weight) / sum_squares;
    let support = (effective_n / SUPPORT_TARGET).min(1.0);

    let mut sorted = memory_vote;
    sorted.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    let margin = if sorted[0] + sorted[1] > 0.0 {
        (sorted[0] - sorted[1]) / (sorted[0] + sorted[1])
    } else {
        0.0
    };
    let memory_confidence = margin * support * maturity;
    let w = ((1.0 - memory_confidence) * prior_strength).min(1.0);
    let distribution: [f32; 4] = [
        (1.0 - w) * memory_vote[0] + w * prior[0],
        (1.0 - w) * memory_vote[1] + w * prior[1],
        (1.0 - w) * memory_vote[2] + w * prior[2],
        (1.0 - w) * memory_vote[3] + w * prior[3],
    ];

    PlayerAggregate {
        distribution,
        confidence: memory_confidence,
        margin,
        support,
        maturity,
        prior_weight: w,
        predicted_dir: argmax(&distribution),
    }
}

/// Public entry point: predict the player's next direction.
pub fn predict_player_move(
    game: &WormGame,
    brain: &CpuBrain,
    tail: &VecDeque<Direction>,
) -> PlayerAggregate {
    let memory_size = brain.opp_brain.episodes.len();
    let context = encode_player_context(game, &brain.player_tail);

    if memory_size < COLD_START_EPISODES {
        return aggregate_player(
            &brain.opp_brain,
            &[],
            brain.opp_brain.seq,
            memory_size,
            tail,
        );
    }

    let recalled = recall_player(&brain.opp_brain, &context, RECALL_K.min(memory_size));
    if recalled.is_empty() {
        return aggregate_player(
            &brain.opp_brain,
            &[],
            brain.opp_brain.seq,
            memory_size,
            tail,
        );
    }

    aggregate_player(
        &brain.opp_brain,
        &recalled,
        brain.opp_brain.seq,
        memory_size,
        tail,
    )
}

/// The faithful `aggregate` from predict.ts, ported to directions.
pub fn aggregate(
    brain: &CpuBrain,
    recalled: &[Recalled],
    current_seq: u32,
    memory_size: usize,
    tail: &VecDeque<Direction>,
) -> CpuAggregate {
    let distribution = [0.25f32; 4];
    let maturity = (memory_size as f32 / COLD_START_EPISODES as f32).min(1.0);
    let cold = memory_size < COLD_START_EPISODES || recalled.is_empty();

    if cold {
        return CpuAggregate {
            distribution,
            confidence: 0.0,
            margin: 0.0,
            support: 0.0,
            maturity,
            prior_weight: 0.0,
        };
    }

    let weights: Vec<f32> = recalled
        .iter()
        .map(|ep| {
            let proximity = 1.0 / (EPS + ep.distance * ep.distance);
            let age = (current_seq as i64 - ep.seq as i64).max(0) as u32;
            let recency = (-((age as f32) / DECAY_TAU)).exp();
            let trail = trailing_match(tail, ep);
            // Crash episodes (reward 0, recorded at death) must not vote FOR
            // repeating the move that died — only surviving episodes count.
            let survived = if ep.reward > 0.0 { 1.0 } else { 0.0 };
            proximity * recency * survived * (1.0 + MATCH_BONUS * trail)
        })
        .collect();

    let total_weight: f32 = weights.iter().sum();
    if total_weight <= 0.0 {
        return CpuAggregate {
            distribution,
            confidence: 0.0,
            margin: 0.0,
            support: 0.0,
            maturity,
            prior_weight: 0.0,
        };
    }

    let mut memory_vote = [0.0f32; 4];
    for (i, ep) in recalled.iter().enumerate() {
        memory_vote[dir_index(ep.surviving_dir)] += weights[i] / total_weight;
    }

    let sum_squares: f32 = weights.iter().map(|w| w * w).sum();
    let effective_n = (total_weight * total_weight) / sum_squares;
    let support = (effective_n / SUPPORT_TARGET).min(1.0);

    let mut sorted = memory_vote;
    sorted.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    let margin = if sorted[0] + sorted[1] > 0.0 {
        (sorted[0] - sorted[1]) / (sorted[0] + sorted[1])
    } else {
        0.0
    };
    let memory_confidence = margin * support * maturity;

    let prior = brain.prior_distribution();
    let prior_strength = brain.prior_strength();
    // Blend gate — identical to rps-ai.
    let w = ((1.0 - memory_confidence) * prior_strength).min(1.0);
    let distribution: [f32; 4] = [
        (1.0 - w) * memory_vote[0] + w * prior[0],
        (1.0 - w) * memory_vote[1] + w * prior[1],
        (1.0 - w) * memory_vote[2] + w * prior[2],
        (1.0 - w) * memory_vote[3] + w * prior[3],
    ];

    CpuAggregate {
        distribution,
        confidence: memory_confidence,
        margin,
        support,
        maturity,
        prior_weight: w,
    }
}

/// Trailing-match bonus: fraction of the query tail that agrees with the
/// recalled direction, rightmost-aligned, in [0,1] (rps-ai `trailingMatchScore`).
fn trailing_match(a: &VecDeque<Direction>, ep: &Recalled) -> f32 {
    let span = a.len().clamp(1, 4);
    let matches = a.iter().filter(|d| **d == ep.surviving_dir).count();
    (matches as f32 / span as f32).min(1.0)
}

/* --------------------------- move scoring --------------------------- */

/* ------------------------------ decide procedure ------------------------------ */

/// Whether cell (x,y) is threatened by a live projectile.
fn cell_threatened_by_projectile(game: &WormGame, x: u16, y: u16) -> bool {
    for p in &game.projectiles {
        // The CPU's own bolts cannot hit it (advance_projectiles excludes the
        // firer), so they are not threats — without this the CPU abandoned its
        // line to dodge its own tri-shot for up to 7 frames after every shot
        // (mirrors the owner exclusion in cell_threatened_by_bomb).
        if p.from == 1 {
            continue;
        }
        // A projectile threatens a cell if it will reach it within steps_left.
        let dx = x as i16 - p.x as i16;
        let dy = y as i16 - p.y as i16;
        // Check if the cell is on the projectile's path.
        if p.dx != 0 && p.dy != 0 {
            // Diagonal bolt: must be on the diagonal path.
            if dx.abs() != dy.abs() {
                continue;
            }
            if (p.dx > 0 && dx < 0) || (p.dx < 0 && dx > 0) {
                continue;
            }
            if (p.dy > 0 && dy < 0) || (p.dy < 0 && dy > 0) {
                continue;
            }
            let steps = dx.unsigned_abs().max(dy.unsigned_abs());
            if steps <= p.steps_left as u16 {
                return true;
            }
        } else if p.dx != 0 {
            // Horizontal bolt.
            if dy != 0 {
                continue;
            }
            if (p.dx > 0 && dx < 0) || (p.dx < 0 && dx > 0) {
                continue;
            }
            let steps = dx.unsigned_abs();
            if steps <= p.steps_left as u16 {
                return true;
            }
        } else if p.dy != 0 {
            // Vertical bolt.
            if dx != 0 {
                continue;
            }
            if (p.dy > 0 && dy < 0) || (p.dy < 0 && dy > 0) {
                continue;
            }
            let steps = dy.unsigned_abs();
            if steps <= p.steps_left as u16 {
                return true;
            }
        }
    }
    false
}

/// Is cell (x,y) unsafe because of an enemy mine?
///
/// A mine has no countdown to read — it fires when you enter its trigger ring
/// — so the question is not "is it about to go off" but "would stepping here
/// set it off, or leave me inside the blast when something else does".
///
/// Two zones, and the distinction matters: the TRIGGER ring is entered at your
/// own peril, while the arms are only lethal at the moment of detonation and
/// are perfectly safe to cross beforehand. Treating the whole cross as
/// untouchable would wall the CPU out of most of the board.
///
/// Note the trigger radius is small (2), which is what makes the CPU's cheap
/// one-step threat check SUFFICIENT for the first time: you cannot route
/// around a 441-cell region one step at a time, but "don't step in the ring"
/// needs no pathfinding at all.
fn cell_threatened_by_bomb(game: &WormGame, x: u16, y: u16, frames_ahead: u8) -> bool {
    let arm = crate::game::BOMB_RADIUS_CELLS as i32;
    let trigger = crate::game::MINE_TRIGGER_CELLS as i32;
    for b in &game.bombs {
        // Our own mines cannot kill us — detonate() excludes the owner — so
        // dodging them was the AI avoiding blasts that were harmless to it.
        if b.owner == 1 {
            continue;
        }
        let cheb = (x as i32 - b.x as i32)
            .abs()
            .max((y as i32 - b.y as i32).abs());

        // Live mine: entering the ring IS the detonation.
        if b.armed_in == 0 && cheb <= trigger {
            return true;
        }
        // Still arming: the ring is genuinely safe to cross until it goes
        // live. Only flag it if it will arm before we could be out again.
        if b.armed_in > 0
            && cheb <= trigger
            && b.armed_in <= frames_ahead as u32 + (trigger + 1 - cheb) as u32
        {
            return true;
        }
        // Failsafe fuse: if it times out on its own, anything left inside the
        // cross dies. Escapability, as before.
        if crate::game::in_blast(b.x as i32, b.y as i32, x as i32, y as i32, arm) {
            let moves_to_clear = (arm + 1 - cheb).max(1) as u32;
            let frames_left = b.fuse.saturating_sub(frames_ahead as u32);
            if moves_to_clear > frames_left {
                return true;
            }
        }
    }
    false
}

/// Iterative multi-frame player prediction: step the player's position
/// forward, turning at obstacles with the ensemble's predicted direction.
/// Returns a vector of (x, y) positions for frames 1..max_frames.
/// One brain, one place: corner turns use the same ensemble prediction the
/// hunt layers use (the old version ran a second, divergent k-NN recall here).
fn predict_player_positions_iterative(
    game: &WormGame,
    predicted_dir: Direction,
    max_frames: usize,
) -> Vec<(u16, u16)> {
    let mut positions = Vec::with_capacity(max_frames);
    let mut px = game.cycles[0].head.0 as i16;
    let mut py = game.cycles[0].head.1 as i16;
    let mut pdir = game.cycles[0].direction;

    // Occupancy for prediction: any cell the player could physically occupy.
    let open = |x: i16, y: i16| -> bool {
        x >= 0
            && y >= 0
            && x < game.width as i16
            && y < game.height as i16
            && matches!(
                game.grid[y as usize][x as usize],
                crate::game::CellType::Empty
                    | crate::game::CellType::Food
                    | crate::game::CellType::Hole
                    | crate::game::CellType::PowerUp
            )
    };

    for _ in 0..max_frames {
        // Check if the player will hit a wall/trail in their current direction.
        let (ddx, ddy) = pdir.as_delta();
        let blocked = !open(px + ddx, py + ddy);

        if blocked {
            // Corner: follow the ensemble's prediction when it leads anywhere
            // occupiable; otherwise fall back to THIS PLAYER'S learned
            // handedness.
            //
            // This used to assume `right_turn` unconditionally — "the
            // canonical wall-follower turn". Against a player the CPU had just
            // learned breaks LEFT, every projected corner went the wrong way,
            // and the intercept layers drove to the wrong side of the board.
            // The better the read, the harder it committed to the wrong place:
            // measured, a CPU that remembered the player won 87% where one
            // that could not learn won 97%. Learning was making it worse, and
            // this line was why.
            let (pdx, pdy) = predicted_dir.as_delta();
            if open(px + pdx, py + pdy) {
                pdir = predicted_dir;
            } else {
                let prior = game.cpu_brain.opp_brain.turn_prior();
                let prefer_left = prior[turn_index(Turn::Left)] >= prior[turn_index(Turn::Right)];
                let (first, second) = if prefer_left {
                    (left_turn(pdir), right_turn(pdir))
                } else {
                    (right_turn(pdir), left_turn(pdir))
                };
                let (fdx, fdy) = first.as_delta();
                pdir = if open(px + fdx, py + fdy) { first } else { second };
            }
        }

        let (dx, dy) = pdir.as_delta();
        // Advance only into a physically occupiable cell; otherwise hold
        // position. The old code clamped coordinates into [2, w-3], which
        // pushed predicted positions INSIDE the ring-2 arena wall and
        // poisoned intercept targeting with unreachable cells.
        let nx = px + dx;
        let ny = py + dy;
        if open(nx, ny) {
            px = nx;
            py = ny;
        }
        positions.push((px as u16, py as u16));
    }
    positions
}

fn right_turn(dir: Direction) -> Direction {
    match dir {
        Direction::Up => Direction::Right,
        Direction::Right => Direction::Down,
        Direction::Down => Direction::Left,
        Direction::Left => Direction::Up,
    }
}

fn left_turn(dir: Direction) -> Direction {
    match dir {
        Direction::Up => Direction::Left,
        Direction::Left => Direction::Down,
        Direction::Down => Direction::Right,
        Direction::Right => Direction::Up,
    }
}

/* ============================ The rps-ai ensemble ============================
 *
 * The REAL rps-ai mechanism (src/model.py), faithfully ported: six+ specialist
 * models, each a falsifiable assumption about the player. Every model predicts
 * every frame and every prediction is recorded (counterfactual recording) —
 * then scored next frame against what the player actually did with QUADRATIC
 * recency weights (frame j counts j², so a figured-out model craters fast and
 * the ensemble rotates away from it). The argmax-score model's prediction is
 * the one that drives play. Scores are per-game (reset on restart, like
 * rps-ai's per-game record); the k-NN memory beneath persists as the corpus.
 */

pub const ENSEMBLE_MODELS: usize = 7;
/// Short names for the HUD brain panel, in model order.
pub const MODEL_NAMES: [&str; ENSEMBLE_MODELS] = ["rep", "pat", "frq", "due", "wlR", "wlL", "knn"];
/// Score bonus for the sophisticated model once warm (rps-ai's +0.15).
const KNN_SCORE_BONUS: f32 = 0.15;

/// Live per-model state. `num`/`den` accumulate ±j² / j² per game;
/// `hits`/`total` are plain per-game counters for the HUD hit-rates.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Ensemble {
    pub num: [f32; ENSEMBLE_MODELS],
    pub den: [f32; ENSEMBLE_MODELS],
    pub hits: [u32; ENSEMBLE_MODELS],
    pub total: [u32; ENSEMBLE_MODELS],
    /// Each model's prediction for the upcoming frame (scored next frame).
    pub pending: [Option<Direction>; ENSEMBLE_MODELS],
    /// Index of the model whose prediction currently drives play.
    pub active: usize,
    /// Rolling hit-rate of the active model — the confidence the hunt
    /// layers gate on (rps-ai: trust the model that's earning its keep).
    pub confidence: f32,
    /// The active model's prediction for the player's next direction.
    pub predicted_dir: Option<Direction>,
}

impl Default for Ensemble {
    fn default() -> Self {
        Self {
            num: [0.0; ENSEMBLE_MODELS],
            den: [0.0; ENSEMBLE_MODELS],
            hits: [0; ENSEMBLE_MODELS],
            total: [0; ENSEMBLE_MODELS],
            pending: [None; ENSEMBLE_MODELS],
            active: 0,
            confidence: 0.0,
            predicted_dir: None,
        }
    }
}

impl Ensemble {
    /// Per-game reset (rps-ai wipes its record each game; the k-NN memory
    /// below persists). Clears scores, hit-rates and pending predictions.
    pub fn reset_scores(&mut self) {
        *self = Self::default();
    }

    /// Model's quadratic score in [-1, 1] (0.0 when never scored).
    pub fn score(&self, model: usize) -> f32 {
        if self.den[model] <= 0.0 {
            0.0
        } else {
            self.num[model] / self.den[model]
        }
    }

    /// Score the pending predictions against the player's actual move.
    /// `frame_idx` is the in-game frame index — its square is the recency
    /// weight (rps-ai's j²).
    pub fn score_frame(&mut self, actual: Direction, frame_idx: u32) {
        let w = (frame_idx as f32).max(1.0);
        let w2 = w * w;
        for i in 0..ENSEMBLE_MODELS {
            if let Some(pred) = self.pending[i].take() {
                let hit = pred == actual;
                self.num[i] += if hit { w2 } else { -w2 };
                self.den[i] += w2;
                self.total[i] += 1;
                if hit {
                    self.hits[i] += 1;
                }
            }
        }
    }
}

/* --------------------------- the seven assumptions --------------------------- */

/// M0 `rep`: the player repeats on a streak, otherwise turns (right bias).
/// (rps-ai model0: beats-last on repeat-streak, loses-to-last otherwise.)
fn m_repeat(tail: &VecDeque<Direction>) -> Option<Direction> {
    let last = *tail.back()?;
    let n = tail.len();
    if n >= 3 {
        let repeats = (1..=2).filter(|&i| tail[n - i] == tail[n - i - 1]).count();
        if repeats >= 2 {
            return Some(last);
        }
    }
    Some(right_turn(last))
}

/// M1 `pat`: the player's transition vector (straight/left/right) follows a
/// pattern — predict the modal recent transition applied to the last move.
/// (rps-ai model1: mode of the last four transition vectors.)
fn m_pattern(tail: &VecDeque<Direction>) -> Option<Direction> {
    if tail.len() < 2 {
        return None;
    }
    let vec_of = |a: Direction, b: Direction| -> i8 {
        if a == b {
            0 // straight
        } else if b == right_turn(a) {
            1 // right
        } else if b == left_turn(a) {
            -1 // left
        } else {
            2 // 180 — impossible under game rules
        }
    };
    let n = tail.len();
    let start = n.saturating_sub(4).max(1);
    let mut counts = [0usize; 3]; // [-1, 0, +1]
    for i in start..n {
        let v = vec_of(tail[i - 1], tail[i]);
        if v != 2 {
            counts[(v + 1) as usize] += 1;
        }
    }
    let mode = counts
        .iter()
        .enumerate()
        .max_by_key(|(_, &c)| c)
        .map(|(i, _)| i as i8 - 1)
        .unwrap_or(0);
    let last = *tail.back().unwrap();
    Some(match mode {
        0 => last,
        1 => right_turn(last),
        _ => left_turn(last),
    })
}

/// M2 `frq`: the player favours one direction — predict their most frequent
/// recent move. (rps-ai model2.)
fn m_frequent(tail: &VecDeque<Direction>) -> Option<Direction> {
    if tail.is_empty() {
        return None;
    }
    let mut counts = [0usize; 4];
    for &d in tail {
        counts[dir_index(d)] += 1;
    }
    let best = counts
        .iter()
        .enumerate()
        .max_by_key(|(_, &c)| c)
        .map(|(i, _)| i)
        .unwrap_or(3);
    Some(index_dir(best))
}

/// M3 `due`: the player rotates to whatever they've used least — predict the
/// least frequent recent move (longest-unseen wins ties). (rps-ai model3.)
fn m_due(tail: &VecDeque<Direction>) -> Option<Direction> {
    if tail.is_empty() {
        return None;
    }
    let mut counts = [0usize; 4];
    let mut last_seen = [usize::MAX; 4];
    for (i, &d) in tail.iter().enumerate() {
        counts[dir_index(d)] += 1;
        last_seen[dir_index(d)] = i;
    }
    let best = (0..4)
        // Tie-break: smaller last_seen = longest unseen, per the doc above.
        // (The old key inverted this and picked the MOST recently seen.)
        .min_by_key(|&i| (counts[i], last_seen[i]))
        .unwrap_or(3);
    Some(index_dir(best))
}

/// M4/M5 `wlR`/`wlL`: the player is a wall-follower — straight while the
/// road ahead is open, then turn (right-handed and left-handed variants).
/// TRON has a chirality rps doesn't, so the assumption comes in both hands.
fn m_wall(game: &WormGame, right_handed: bool) -> Option<Direction> {
    let p = &game.cycles[0];
    let dir = p.direction;
    if free_step(game, p.head.0, p.head.1, dir) {
        Some(dir)
    } else if right_handed {
        Some(right_turn(dir))
    } else {
        Some(left_turn(dir))
    }
}

/// M6 `knn`: the sophisticated model — k-NN opponent memory over the
/// player-centric context. Cold (fewer than COLD_START_EPISODES) it abstains,
/// exactly like rps-ai's NN/DT needing 5–7 rounds of history.
fn m_knn(game: &WormGame, brain: &CpuBrain) -> Option<Direction> {
    if brain.opp_brain.episodes.len() < COLD_START_EPISODES {
        return None;
    }
    Some(predict_player_move(game, brain, &brain.player_tail).predicted_dir)
}

/// Compute every model's prediction for the next frame, pick the driver by
/// quadratic score (knn gets its sophistication bonus once warm), and report
/// the driver's rolling hit-rate as the ensemble confidence.
/// (rps-ai `computer_choice`.)
pub fn compute_ensemble(
    game: &WormGame,
    brain: &CpuBrain,
) -> ([Option<Direction>; ENSEMBLE_MODELS], usize, f32) {
    let tail = &brain.player_tail;
    let pending = [
        m_repeat(tail),
        m_pattern(tail),
        m_frequent(tail),
        m_due(tail),
        m_wall(game, true),
        m_wall(game, false),
        m_knn(game, brain),
    ];

    let e = &brain.ensemble;
    let mut best = usize::MAX;
    let mut best_score = f32::NEG_INFINITY;
    for i in 0..ENSEMBLE_MODELS {
        if e.den[i] <= 0.0 {
            continue; // never scored — not eligible yet
        }
        let s = ensemble_rank_score(brain, i);
        if s > best_score {
            best_score = s;
            best = i;
        }
    }
    // First frame (no scores yet): rps-ai forces model 0.
    let active = if best == usize::MAX { 0 } else { best };
    let confidence = if e.total[active] > 0 {
        e.hits[active] as f32 / e.total[active] as f32
    } else {
        0.0
    };
    (pending, active, confidence)
}

/// Effective score used by ensemble selection. The UI exports this alongside
/// the raw quadratic score so a warm k-NN bonus is never an invisible tie-break.
pub fn ensemble_rank_score(brain: &CpuBrain, model: usize) -> f32 {
    let mut score = brain.ensemble.score(model);
    if model == ENSEMBLE_MODELS - 1 && brain.opp_brain.episodes.len() >= COLD_START_EPISODES {
        score += KNN_SCORE_BONUS;
    }
    score
}

/// Heuristic for when the CPU should fire a held power-up.
/// Fires when the player is in range for a kill:
///   - Laser: player is in line of fire (same row/col, no walls between)
///   - TriShot: player sits on one of the three bolt rays
///   - Bomb: the player's projected path crosses the spot we would mine
pub fn should_fire(game: &mut WormGame, who: usize) -> bool {
    let kind = match game.cycles[who].held_powerup {
        Some(k) => k,
        None => return false,
    };
    let opp = 1 - who;
    if !game.cycles[opp].alive {
        return false;
    }
    let (hx, hy) = game.cycles[who].head;
    let (ox, oy) = game.cycles[opp].head;

    match kind {
        crate::game::PowerUpKind::Laser => {
            // The beam ricochets off arena walls, so the player needn't share
            // a row/col — fire when the (possibly bounced) beam path reaches
            // the player's head. The telegraph draws this exact path.
            let (dx, dy) = game.cycles[who].direction.as_delta();
            let beam = beam_cells(game, hx, hy, dx, dy);
            beam.contains(&(ox, oy))
        }
        crate::game::PowerUpKind::TriShot => {
            // Bolts occupy exactly three rays — straight ahead and the two
            // forward diagonals — so alignment, not distance, decides whether
            // a shot can land.
            //
            // The old test was `manhattan <= TRI_SHOT_RANGE && forward`, an
            // ARC. Most of the arc is unhittable, and now that bolts fly until
            // a wall a distance gate would also refuse exactly the long
            // straight shots that make the unbounded bolt worth having.
            let (dx, dy) = game.cycles[who].direction.as_delta();
            let fdx = ox as i16 - hx as i16;
            let fdy = oy as i16 - hy as i16;
            let forward = dx * fdx + dy * fdy > 0;
            let on_ray = fdx == 0 || fdy == 0 || fdx.abs() == fdy.abs();
            forward && on_ray
        }
        crate::game::PowerUpKind::Bomb => {
            // A mine is PLACED, not aimed, so proximity is the wrong question.
            // "Player within blast radius" read as "throw it at them" and was
            // the single worst use of a power-up in the game: it planted at
            // the radius edge, where the target simply walked out.
            //
            // Two gates. Don't wall ourselves in — we need room to leave our
            // own trigger ring. And plant where they are actually going.
            if legal_directions(game, &game.cycles[who]).len() < 2 {
                return false;
            }
            let path = predict_player_positions_iterative(game, game.cycles[0].direction, 12);
            let reach = (crate::game::MINE_TRIGGER_CELLS + 1) as i32;
            path.iter().any(|&(px, py)| {
                (px as i32 - hx as i32)
                    .abs()
                    .max((py as i32 - hy as i32).abs())
                    <= reach
            })
        }
    }
}

/// Beam path with arena-wall ricochet — mirror of game.rs beam_cells (the
/// CPU's aim, the telegraph, and the actual shot must trace the same path).
fn beam_cells(game: &WormGame, hx: u16, hy: u16, dx: i16, dy: i16) -> Vec<(u16, u16)> {
    let mut out = Vec::new();
    let mut x = hx as i16;
    let mut y = hy as i16;
    let mut rdx = dx;
    let mut rdy = dy;
    let mut bounces = 0;
    loop {
        x += rdx;
        y += rdy;
        if x < 0 || y < 0 || x >= game.width as i16 || y >= game.height as i16 {
            break;
        }
        let (ux, uy) = (x as u16, y as u16);
        if game.grid[uy as usize][ux as usize] == crate::game::CellType::Wall {
            if bounces < 4 && game.is_arena_wall(ux, uy) {
                if (ux == 2 || ux == game.width - 3) && rdx != 0 {
                    rdx = -rdx;
                }
                if (uy == 2 || uy == game.height - 3) && rdy != 0 {
                    rdy = -rdy;
                }
                bounces += 1;
                continue;
            }
            break;
        }
        out.push((ux, uy));
    }
    out
}

/// Faithful to rps-ai's `think` + `decide`: memory-driven read,
/// confidence-gated, blended with a base-rate prior, resolved by
/// deterministic argmax (the 5% explore lives only in the close-evasion
/// branch).
pub fn cpu_decide(game: &mut WormGame) -> Direction {
    let mut decision_forecast = None;
    let mut decision_projection = None;
    macro_rules! choose {
        ($direction:expr, $reason:expr) => {{
            let chosen = $direction;
            let trace = CpuDecisionTrace {
                frame: game.frame_count,
                heading: chosen,
                reason: $reason,
                forecast: decision_forecast,
                projection: decision_projection.clone(),
            };
            game.cpu_telemetry.decision = Some(trace.clone());
            game.round_last_cpu_decision = Some(trace);
            return chosen;
        }};
    }

    let legal = legal_directions(game, &game.cycles[1]);
    if legal.is_empty() {
        choose!(game.cycles[1].direction, CpuDecisionReason::NoLegalMove);
    }
    if legal.len() == 1 {
        choose!(legal[0], CpuDecisionReason::ForcedMove);
    }

    let memory_size = game.cpu_brain.episodes.len();

    // Cold start / low memory: use a simple wall-follower heuristic (same as
    // the naive benchmark opponent) until the memory has enough data to drive
    // decisions. This guarantees the adaptive CPU is at least as good as the
    // baseline during the warm-up phase.
    if memory_size < COLD_START_EPISODES {
        // Still subject to sudden death — see `evacuate_ring`.
        let head = game.cycles[1].head;
        let warm = wall_follow_decide(game, &game.cycles[1]);
        choose!(
            evacuate_ring(game, head, warm, &legal),
            CpuDecisionReason::WarmingUp
        );
    }

    // Memory-driven: wall-follow base + defensive avoidance + adjacent food.
    // The wall-follow pattern is the survival strategy. The ensemble's
    // opponent prediction (refreshed at frame end) drives the hunt layers.

    // --- Opponent Model Prediction (the rps-ai ensemble, refreshed at frame end) ---
    // The forecast for the frame the player has NOT yet chosen.
    //
    // This used to read `cpu_telemetry.scored` — the forecast for the frame
    // already in progress, whose answer is `cycles[0].direction` by the time
    // this runs. Steering on it meant the "prediction" was a restatement of an
    // observable, and no amount of improving the model could have changed a
    // decision. `next_forecast` is now produced before this call (see the
    // ordering note in `WormGame::update`) and genuinely targets t+1.
    decision_forecast = game
        .cpu_telemetry
        .next_forecast
        .or_else(|| game.cpu_telemetry.scored.map(|scored| scored.forecast));
    let player_pred_dir = decision_forecast
        .and_then(|forecast| forecast.predicted)
        .unwrap_or(game.cycles[0].direction);
    // Confidence gated by OBSERVATION COUNT, per Johanson & Bowling's
    // data-biased response result (AISTATS 2009): counter-strategies that
    // trust a model after a single observation measured WORSE than not
    // modelling at all, while a 0-10 linear ramp — zero trust at zero
    // observations, full trust from ten — stayed robust regardless of data
    // quantity. The ensemble's raw confidence is high from frame one (it is
    // mostly scored on easy frames), so ungated it opens every hunt gate
    // before the CPU has actually seen this player choose. n here is the
    // count of genuine left/right choices observed — the same quantity the
    // turn prior stands on.
    //
    // Explainable as: "it doesn't act on a read until it has watched you make
    // about ten real choices."
    let read_conf = (game.cpu_brain.opp_brain.turn_observations() / 10.0).min(1.0);
    let player_pred_conf = decision_forecast
        .map(|forecast| forecast.confidence)
        .unwrap_or(0.0)
        * read_conf;
    let cpu = &game.cycles[1];
    let (cx, cy) = cpu.head;

    // Iterative multi-frame prediction: predicts direction changes at corners.
    let predicted_positions = predict_player_positions_iterative(game, player_pred_dir, 5);
    if decision_forecast.is_some() {
        decision_projection = Some(PlayerProjection {
            direction: player_pred_dir,
            path: predicted_positions.clone(),
        });
    }

    // --- THREAT AVOIDANCE: dodge projectiles and bombs ---
    // Check each legal direction for threats. If the wall-follow direction is
    // threatened, find the safest alternative.
    let wall_dir = wall_follow_decide(game, &game.cycles[1]);
    let mut threatened_dirs = Vec::new();
    for &d in &legal {
        let (ddx, ddy) = d.as_delta();
        let nx = (cx as i16 + ddx).max(0).min((game.width - 1) as i16) as u16;
        let ny = (cy as i16 + ddy).max(0).min((game.height - 1) as i16) as u16;
        if cell_threatened_by_projectile(game, nx, ny) || cell_threatened_by_bomb(game, nx, ny, 3) {
            threatened_dirs.push(d);
        }
    }

    // If wall-follow is threatened, find a safe alternative immediately.
    if threatened_dirs.contains(&wall_dir) {
        let safe_dirs: Vec<&Direction> = legal
            .iter()
            .filter(|d| !threatened_dirs.contains(d))
            .collect();
        if !safe_dirs.is_empty() {
            // Pick the safe direction closest to wall-follow (minimises deviation).
            let mut best_dir = *safe_dirs[0];
            let mut best_score = f32::NEG_INFINITY;
            for &d in &safe_dirs {
                let (ddx, ddy) = d.as_delta();
                let nx = (cx as i16 + ddx).max(0).min((game.width - 1) as i16) as u16;
                let ny = (cy as i16 + ddy).max(0).min((game.height - 1) as i16) as u16;
                let open = count_open_space(game, nx, ny);
                let norm_open = open / (game.width as f32 * game.height as f32);
                let score = norm_open * 1000.0;
                if score > best_score {
                    best_score = score;
                    best_dir = *d;
                }
            }
            choose!(best_dir, CpuDecisionReason::ThreatDodge);
        }
    }

    // Candidate directions for every layer below: legal minus threatened.
    // The threat gate used to protect ONLY wall-follow — the food/intercept/
    // defensive layers could then return a direction with a projectile or
    // bomb blast landing on it. Fall back to full legal when everything is
    // threatened (a dodge is impossible; let the layers pick the least-bad).
    //
    // A cell on a sudden-death ring that is about to seal is also excluded:
    // `close_ring` kills any head standing on the ring it closes, and nothing
    // in this file previously knew sudden death existed.
    let safe_legal: Vec<Direction> = legal
        .iter()
        .copied()
        .filter(|d| !threatened_dirs.contains(d) && !ring_doomed_step(game, (cx, cy), *d))
        .collect();
    // Never empty the candidate set — when every move is threatened or doomed
    // the layers below still have to pick the least-bad one.
    let ring_safe_legal: Vec<Direction> = legal
        .iter()
        .copied()
        .filter(|d| !ring_doomed_step(game, (cx, cy), *d))
        .collect();
    let candidates: &[Direction] = if !safe_legal.is_empty() {
        &safe_legal
    } else if !ring_safe_legal.is_empty() {
        &ring_safe_legal
    } else {
        &legal
    };

    // Survival floor for every deviation from wall-follow: the destination
    // must keep at least this much of the arena reachable (absolute), and at
    // least half of what wall-follow keeps (relative). Prevents hunt dives
    // into pockets behind our own trail — the kamikaze move.
    let total_cells = game.width as f32 * game.height as f32;
    let (wdx, wdy) = wall_dir.as_delta();
    let wx = (cx as i16 + wdx).max(0).min((game.width - 1) as i16) as u16;
    let wy = (cy as i16 + wdy).max(0).min((game.height - 1) as i16) as u16;
    let wall_open = count_open_space(game, wx, wy) / total_cells;
    // Absolute, LENGTH-RELATIVE survival floor, in cells.
    //
    // This replaced an arena FRACTION (0.12 of the board, or half of whatever
    // wall-follow kept, whichever was larger), which was wrong at both ends of
    // a round: early game `wall_open` is ~0.80
    // so the floor became ~0.40 of the board — demanding ~1824 reachable cells
    // of a nine-cell snake, which suppressed every safe food run — while late
    // game, once total reachable space fell under ~547 cells, the floor could
    // no longer be met by ANY move and all deviation layers switched off at
    // once, blinding the CPU exactly when the arena got interesting.
    //
    // The quantity that actually decides survival is not "how much of the
    // arena" but "can I still outrun my own body", so the floor scales with
    // length. `escape_floor_cells` is the single source of truth.
    // Survival floor — never scaled by difficulty.
    let escape_cells = escape_floor_cells(game, 1);
    // Hunt floor — what an OPTIONAL aggressive deviation must clear. Shrinks
    // as the CPU's read of this player improves, so a well-read player faces a
    // CPU that commits to intercepts it would otherwise decline.
    let hunt_cells = hunt_floor_cells(game, 1, game.read_rate);

    // --- DEFENSIVE: avoid predicted player when very close ---
    // SURVIVAL BEFORE HUNTING: this used to run AFTER the intercept layers,
    // so a confident hunt could steer us head-on into the player and the
    // dodge never ran. Now it outranks food and intercepts alike.
    if player_pred_conf >= 0.4 {
        let mut min_dist = i16::MAX;
        for &(px, py) in &predicted_positions {
            let dist = (cx as i16 - px as i16).abs() + (cy as i16 - py as i16).abs();
            min_dist = min_dist.min(dist);
        }
        if min_dist <= 2 {
            let mut best_dir = wall_dir;
            let mut best_score = f32::NEG_INFINITY;
            for &d in candidates {
                let (ddx, ddy) = d.as_delta();
                let nx = (cx as i16 + ddx).max(0).min((game.width - 1) as i16) as u16;
                let ny = (cy as i16 + ddy).max(0).min((game.height - 1) as i16) as u16;
                let mut dmin = i16::MAX;
                for &(px, py) in &predicted_positions {
                    let dd = (nx as i16 - px as i16).abs() + (ny as i16 - py as i16).abs();
                    dmin = dmin.min(dd);
                }
                let wall_bonus = if d == wall_dir { 3.0 } else { 0.0 };
                let score = dmin as f32 + wall_bonus;
                if score > best_score {
                    best_score = score;
                    best_dir = d;
                }
            }
            if game.rng_f32(0.0, 1.0) < EXPLORE_RATE {
                // Draw from the threat-filtered candidates, not raw legal —
                // exploration must not step onto a bolt path or blast zone
                // the dodge logic just steered around.
                choose!(
                    candidates[(game.rng_f32(0.0, candidates.len() as f32) as usize)
                        .min(candidates.len() - 1)],
                    CpuDecisionReason::CloseEvasion
                );
            }
            choose!(best_dir, CpuDecisionReason::CloseEvasion);
        }
    }

    // --- ITEMS: grab food / power-ups on or near our path ---
    // Only deviate for items already near our path — we don't abandon the
    // perimeter. Three tiers:
    //   1. Food or power-up directly adjacent (1 cell) — grab it.
    //   2. Food up to 3 cells ahead along the wall-follow axis — keep going.
    //   3. BFS pathfinding to the nearest food or power-up (when the
    //      destination has enough open space to not trap us).
    if !game.food_items.is_empty() || !game.powerups.is_empty() {
        // Tier 1: adjacent food OR power-up in a candidate direction. The CPU
        // used to walk straight past power-ups; anything collectible one step
        // away is worth the deviation. Grid-based check (candidates are
        // already passable-verified, so bombs are out).
        for &d in candidates {
            let (ddx, ddy) = d.as_delta();
            let nx = (cx as i16 + ddx).max(0).min((game.width - 1) as i16) as u16;
            let ny = (cy as i16 + ddy).max(0).min((game.height - 1) as i16) as u16;
            if matches!(
                game.grid[ny as usize][nx as usize],
                CellType::Food | CellType::PowerUp
            ) {
                choose!(d, CpuDecisionReason::ItemPickup);
            }
        }

        // Tier 2: food up to 3 cells ahead along the wall-follow axis.
        // Safe because wall-follow already goes that direction — we're just
        // confirming the food is on the path we're taking anyway.
        if free_step(game, cx, cy, wall_dir) {
            let mut nearest: Option<f32> = None;
            for &(fx, fy, _) in &game.food_items {
                let on_axis = match wall_dir {
                    Direction::Up | Direction::Down => fx == cx,
                    Direction::Left | Direction::Right => fy == cy,
                };
                if !on_axis {
                    continue;
                }
                let dist = ((fx as i16 - cx as i16).abs() + (fy as i16 - cy as i16).abs()) as f32;
                if !(1.0..=3.0).contains(&dist) {
                    continue;
                }
                if nearest.is_none() || dist < nearest.unwrap() {
                    nearest = Some(dist);
                }
            }
            if nearest.is_some() {
                choose!(wall_dir, CpuDecisionReason::ItemPath);
            }
        }

        // Tier 3: BFS pathfinding to nearest safe food.
        // Only used when the food isn't on the wall-follow path and the
        // destination has enough open space to not trap us.
        if let Some((food_dir, food_open)) = bfs_nearest_food_dir(game, cx, cy, candidates) {
            // Only take the BFS path if the destination clears the same
            // survival floor every other wall-follow deviation obeys (the
            // old hard-coded 10% undercut open_floor on both axes).
            //
            // The route is taken even when it coincides with wall-follow.
            // It previously bailed on `food_dir != wall_dir` — a HUD-honesty
            // guard (don't label a move "ItemPath" when wall-follow would make
            // it anyway) that silently cost the food itself: measured, 98.6%
            // of the frames where the CPU moved AWAY from reachable food were
            // frames where the food lay in the wall-follow direction, so the
            // layer declined and the k-NN below wandered off instead.
            // Honesty is preserved by labelling the coincidence, not by
            // forfeiting the food.
            if food_open >= escape_cells {
                let reason = if food_dir == wall_dir {
                    CpuDecisionReason::WallFollow
                } else {
                    CpuDecisionReason::ItemPath
                };
                choose!(food_dir, reason);
            }
        }
    }

    // --- CHOKEPOINT INTERCEPT: cut across to corners against wall-followers ---
    // When the player is a wall-follower (confidence high, direction stable),
    // predict which corner they'll reach and cut across to lay a trail barrier.
    // This works even when the player is >10 cells away (standard intercept range).
    if player_pred_conf >= 0.5 {
        // Predict the corner the player will reach next.
        // Wall-followers turn right at corners. We predict their path to the next corner.
        let corner_target = predict_next_corner(game, &game.cycles[0], player_pred_dir);

        if let Some((corner_x, corner_y)) = corner_target {
            let dist_to_corner =
                ((cx as i16 - corner_x as i16).abs() + (cy as i16 - corner_y as i16).abs()) as f32;

            // Only intercept if the corner is reachable (within ~20 cells)
            // and we're not too close to the player (avoid head-on).
            if (5.0..=25.0).contains(&dist_to_corner) {
                let mut best_dir = wall_dir;
                let mut best_score = f32::NEG_INFINITY;
                for &d in candidates {
                    let (ddx, ddy) = d.as_delta();
                    let nx = (cx as i16 + ddx).max(0).min((game.width - 1) as i16) as u16;
                    let ny = (cy as i16 + ddy).max(0).min((game.height - 1) as i16) as u16;

                    // Distance from new position to corner (lower = closer).
                    let corner_dist = ((nx as i16 - corner_x as i16).abs()
                        + (ny as i16 - corner_y as i16).abs())
                        as f32;

                    // Open space from destination (higher = safer).
                    let open = count_open_space(game, nx, ny);
                    let norm_open = open / (game.width as f32 * game.height as f32);

                    // Hunt floor: never dive into a pocket for a hunt, but
                    // spend margin in proportion to how well we read them.
                    if open < hunt_cells {
                        continue;
                    }

                    // Score: prefer closer to corner + more open space.
                    // Wall-follow gets a bonus so we don't abandon the wall
                    // for a marginal intercept.
                    let wall_bonus = if d == wall_dir { 1.0 } else { 0.0 };
                    let score =
                        (20.0 - corner_dist) * (0.5 + 2.5 * game.read_rate) + norm_open * 3.0 + wall_bonus;

                    if score > best_score {
                        best_score = score;
                        best_dir = d;
                    }
                }
                // Only take the chokepoint intercept if it's meaningfully better.
                if best_dir != wall_dir && best_score > 5.0 {
                    choose!(best_dir, CpuDecisionReason::CornerIntercept);
                }
            }
        }
    }

    // --- INTERCEPT: position to create a trail barrier across the player's path ---
    // When the prediction is confident and the player is within intercept range,
    // move toward the player's predicted future position. The CPU passes through
    // it, leaving a trail the player crashes into. Against wall-followers this
    // triggers at the corners where both cycles converge; against chasers it
    // triggers constantly because the player is always approaching.
    if player_pred_conf >= 0.6 {
        // Target: where the player will be in 2-5 frames (from iterative prediction).
        let mut best_intercept: Option<(u16, u16, f32)> = None;
        for (i, &(px, py)) in predicted_positions.iter().enumerate() {
            let frames_ahead = (i + 1) as f32; // 1, 2, 3, 4, 5
            let dist = ((cx as i16 - px as i16).abs() + (cy as i16 - py as i16).abs()) as f32;
            // Score: closer target + fewer frames ahead = easier intercept.
            let score = 20.0 - dist - frames_ahead * 2.0;
            if best_intercept.is_none() || score > best_intercept.unwrap().2 {
                best_intercept = Some((px, py, score));
            }
        }

        if let Some((target_px, target_py, _)) = best_intercept {
            let dist_to_target = ((cx as i16 - target_px as i16).abs()
                + (cy as i16 - target_py as i16).abs()) as u16;

            // Intercept range: 2-10 cells. Too close risks head-on,
            // too far means we can't reach it in time.
            if (2..=10).contains(&dist_to_target) {
                let mut best_dir = wall_dir;
                let mut best_score = f32::NEG_INFINITY;
                for &d in candidates {
                    let (ddx, ddy) = d.as_delta();
                    let nx = (cx as i16 + ddx).max(0).min((game.width - 1) as i16) as u16;
                    let ny = (cy as i16 + ddy).max(0).min((game.height - 1) as i16) as u16;

                    // Distance from new position to intercept point (lower = closer).
                    let intercept_dist = ((nx as i16 - target_px as i16).abs()
                        + (ny as i16 - target_py as i16).abs())
                        as f32;

                    // Open space from destination (higher = safer).
                    let open = count_open_space(game, nx, ny);
                    let norm_open = open / (game.width as f32 * game.height as f32);

                    // Hunt floor: never dive into a pocket for a hunt, but
                    // spend margin in proportion to how well we read them.
                    if open < hunt_cells {
                        continue;
                    }

                    // Score: prefer closer to intercept + more open space.
                    // Wall-follow gets a bonus so we don't abandon the wall
                    // for a marginal intercept.
                    let wall_bonus = if d == wall_dir { 1.0 } else { 0.0 };
                    let score =
                        (15.0 - intercept_dist) * (0.6 + 3.0 * game.read_rate) + norm_open * 3.0 + wall_bonus;

                    if score > best_score {
                        best_score = score;
                        best_dir = d;
                    }
                }
                // Only take the intercept if it's meaningfully better than wall-follow.
                if best_dir != wall_dir && best_score > 5.0 {
                    choose!(best_dir, CpuDecisionReason::DirectIntercept);
                }
            }
        }
    }

    // --- SELF-MEMORY VOTE: ask the CPU's own survival episodes ---
    // rps-ai's core loop: every decision → encode situation → recall similar
    // pasts → vote → act. We kept the survival floor (wall-follow) but let the
    // k-NN memory cast the deciding vote when it is confident enough: recall
    // episodes whose situation is near ours, weight by proximity × recency ×
    // trailing-match, blend with the prior, then vote with the legal favourite.
    // The deviation gate below is the "memory modifies survival, never replaces
    // it" rule the defensive/intercept layers above follow: the vote fires only
    // when it is confident AND its destination is at least as open as
    // wall-follow's, so a noisy sample can't trade a safe wall for a dead pocket.
    if memory_size >= COLD_START_EPISODES {
        let obs = encode_situation(game, &game.cpu_brain);
        let recalled = recall(&game.cpu_brain, &obs, RECALL_K.min(memory_size));
        if !recalled.is_empty() {
            let agg = aggregate(
                &game.cpu_brain,
                &recalled,
                game.cpu_brain.cpu_seq,
                memory_size,
                &game.cpu_brain.player_tail,
            );
            // The vote is over all 4 directions; restrict it to legal ones.
            let mut legal_dist = [0.0f32; 4];
            let mut total = 0.0f32;
            for &d in candidates {
                let w = agg.distribution[dir_index(d)].max(0.0);
                legal_dist[dir_index(d)] = w;
                total += w;
            }
            if total > 0.0 {
                // Vote with the favourite (argmax over the legal-restricted
                // distribution) — deterministic. The deterministic base policy
                // (intercept/defensive/food/wall-follow) already earned its
                // wins; the memory only gets to *modify* it, so no temperature
                // or explore noise on top of an optimal base.
                let sampled = candidates
                    .iter()
                    .max_by(|a, b| {
                        legal_dist[dir_index(**a)]
                            .partial_cmp(&legal_dist[dir_index(**b)])
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .copied()
                    .unwrap_or(wall_dir);
                // Deviation gate — same discipline as the intercept/defensive
                // layers above: memory only *modifies* wall-follow, never
                // replaces it. Fire the vote only when the aggregate is
                // confident (margin × support × maturity) AND the sampled
                // destination is at least as open as wall-follow's.
                let (ddx, ddy) = sampled.as_delta();
                let nx = (cx as i16 + ddx).max(0).min((game.width - 1) as i16) as u16;
                let ny = (cy as i16 + ddy).max(0).min((game.height - 1) as i16) as u16;
                let vote_open = count_open_space(game, nx, ny) / total_cells;
                // `sampled != wall_dir` matches the convention of the
                // intercept layers: when the vote merely AGREES with
                // wall-follow, fall through and label it WallFollow — the
                // HUD must not credit memory for moves the base policy
                // produces anyway.
                if sampled != wall_dir
                    && agg.confidence >= SELF_VOTE_MIN_CONFIDENCE
                    && vote_open >= wall_open
                    && vote_open >= MEMORY_VOTE_MIN_OPEN
                {
                    choose!(sampled, CpuDecisionReason::SurvivalMemory);
                }
            }
        }
    }

    // Wall-follow is the base policy, but it is not exempt from the ring.
    // It hugs the inner face of the ring-2 wall, which is precisely the first
    // ring sudden death seals — so the fallthrough that produces most of the
    // CPU's moves is exactly the one that used to walk it into the closing
    // wall. Prefer any candidate that is not about to be sealed.
    choose!(
        evacuate_ring(game, (cx, cy), wall_dir, candidates),
        CpuDecisionReason::WallFollow
    );
}

/// Predict which corner a cycle will reach next given their current direction.
/// Returns (corner_x, corner_y) or None if no clear corner pattern.
fn predict_next_corner(
    game: &WormGame,
    cycle: &LightCycle,
    predicted_dir: Direction,
) -> Option<(u16, u16)> {
    let (hx, hy) = cycle.head;
    let (dx, dy) = predicted_dir.as_delta();

    // Find the next wall intersection in the predicted direction.
    let mut x = hx as i16;
    let mut y = hy as i16;
    let mut steps = 0;
    let max_steps = 30; // Cap the search

    loop {
        x += dx;
        y += dy;
        steps += 1;
        if steps > max_steps {
            return None;
        }
        if x < 2 || y < 2 || x >= game.width as i16 - 2 || y >= game.height as i16 - 2 {
            // Hit the arena wall — this is the corner.
            return Some((x as u16, y as u16));
        }
        if game.grid[y as usize][x as usize] == crate::game::CellType::Wall {
            return Some((x as u16, y as u16));
        }
    }
}

/// Simple right-hand wall follower — the same strategy the naive benchmark
/// opponent uses. Used during cold start so the adaptive CPU is never worse
/// than the baseline.
pub fn wall_follow_decide(game: &WormGame, cpu: &LightCycle) -> Direction {
    let head = cpu.head;
    let current_dir = cpu.direction;

    let right_map = [
        (Direction::Up, Direction::Right),
        (Direction::Right, Direction::Down),
        (Direction::Down, Direction::Left),
        (Direction::Left, Direction::Up),
    ];
    let left_map = [
        (Direction::Up, Direction::Left),
        (Direction::Left, Direction::Down),
        (Direction::Down, Direction::Right),
        (Direction::Right, Direction::Up),
    ];
    let back_map = [
        (Direction::Up, Direction::Down),
        (Direction::Down, Direction::Up),
        (Direction::Left, Direction::Right),
        (Direction::Right, Direction::Left),
    ];

    let right_dir = right_map
        .iter()
        .find(|(d, _)| *d == current_dir)
        .map(|(_, r)| *r)
        .unwrap_or(current_dir);
    let left_dir = left_map
        .iter()
        .find(|(d, _)| *d == current_dir)
        .map(|(_, l)| *l)
        .unwrap_or(current_dir);
    let back_dir = back_map
        .iter()
        .find(|(d, _)| *d == current_dir)
        .map(|(_, b)| *b)
        .unwrap_or(current_dir);

    // Use the same legality predicate as physics (passable via free_step):
    // the old Empty-only check refused to step onto food, punched holes and
    // power-ups, and its coordinate clamping aliased out-of-bounds steps to
    // the head's own cell.
    for dir in [right_dir, current_dir, left_dir, back_dir] {
        if free_step(game, head.0, head.1, dir) {
            return dir;
        }
    }
    current_dir
}

/// Legal directions: no 180° reversal, in-bounds and free.
pub fn legal_directions(game: &WormGame, cpu: &LightCycle) -> Vec<Direction> {
    let dirs = [
        Direction::Up,
        Direction::Down,
        Direction::Left,
        Direction::Right,
    ];
    dirs.iter()
        .copied()
        .filter(|&d| {
            !matches!(
                (cpu.direction, d),
                (Direction::Up, Direction::Down)
                    | (Direction::Down, Direction::Up)
                    | (Direction::Left, Direction::Right)
                    | (Direction::Right, Direction::Left)
            )
        })
        .filter(|&d| free_step(game, cpu.head.0, cpu.head.1, d))
        .collect()
}

/* ------------------------------ episode recording ------------------------------ */

/// Record one CPU move outcome. Faithful to rps-ai `store.remember`: learn only
/// from what happened. reward = survival frames + food value eaten. Better
/// outcomes get re-stored so the vote naturally over-weights moves that lasted.
pub fn record_episode(
    brain: &mut CpuBrain,
    vector: [f32; CPU_FEATURE_DIM],
    dir: Direction,
    survived_frames: u32,
    food_value: u8,
) {
    let reward = survived_frames as f32 + (food_value as f32) * 10.0;
    // Long survivals get double-stored so the vote over-weights moves that
    // lasted (rps-ai re-stores good outcomes). Written explicitly: the old
    // `(x/20).clamp(1,2)` was constant-1 because the frame counter feeding it
    // was reset every frame; the counter is fixed, and this is the intent.
    let copies = if survived_frames >= 40 { 2 } else { 1 };
    for _ in 0..copies {
        brain.remember(vector, dir, reward);
    }
}

/// Record an opponent observation: the player's context before it moved, and
/// the direction it took next. This is the core learning signal for the
/// opponent model — a direct analog of rps-ai storing `nextHumanMove`.
/// Keep one in this many NON-decision frames.
///
/// Not zero: dropping routine frames entirely would be case-control bias — the
/// corpus would teach the CPU that turning is far more common than it is, and
/// the k-NN would over-predict turns everywhere. A thinned sample keeps the
/// base rate honest while leaving room for the frames that matter.
const STRAIGHT_KEEP_EVERY: u32 = 64;

/// Record what the player did, preferring frames where they actually chose.
///
/// `decision` marks a frame where the player had a real alternative. Roughly
/// 95% of frames are none — continuing down an open corridor — and storing
/// them all meant the 4000-episode corpus filled by round THREE with
/// interchangeable straight frames, evicting the rare decisions that carry the
/// human. Capacity was being spent almost entirely on the least informative
/// thing on the board.
pub fn record_player_episode(
    brain: &mut CpuBrain,
    context: [f32; PLAYER_FEATURE_DIM],
    player_next_dir: Direction,
    decision: bool,
) {
    // The absolute base-rate tally sees EVERY move — it is a base rate, and
    // thinning it would bias it.
    brain.opp_brain.observe(player_next_dir);

    // DECISION-FOCUSED RETENTION, deliberately paced by retained rows.
    //
    // `seq` advances only when a row is retained, so this keeps every decision
    // frame and roughly one routine "anchor" row per twelve retained ones —
    // the corpus ends up ~90% decision frames. That is intentional, and it
    // was arrived at the hard way: an "honest" clock counting OBSERVED frames
    // was tried at both 1-in-12 (routine-majority ~6:1) and 1-in-64 (class
    // parity), and BOTH collapsed the measured read to zero lift while wins
    // held. The k-NN needs a decision-dominated corpus to emit turn
    // predictions confident enough to ever disagree with the trivial
    // always-straight baseline — and those disagreements are where all the
    // evidence of reading a player lives. The routine anchors stay as a
    // small calibration trickle, not as ballast.
    if decision || brain.opp_brain.seq % STRAIGHT_KEEP_EVERY == 0 {
        brain.opp_brain.remember(context, player_next_dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A forecast the player cannot execute is an abstention, and it abstains
    /// on exactly the frames that decide anything. Masking converts it into a
    /// real guess drawn from their habit — measured, this alone took lift from
    /// 0% to 69% against an opponent with a left-turn signature.
    #[test]
    fn an_impossible_forecast_falls_back_to_the_turn_habit() {
        // Travelling Up, and they habitually break LEFT when forced.
        // Left-of-Up is Left; right-of-Up is Right.
        let turn_prior = [0.10, 0.80, 0.10]; // Straight, Left, Right
        let legal = [Direction::Left, Direction::Right];

        assert_eq!(
            mask_to_legal(Some(Direction::Up), &legal, Direction::Up, &turn_prior),
            Some(Direction::Left),
            "an unreachable prediction must become their habitual TURN"
        );
        assert_eq!(
            mask_to_legal(None, &legal, Direction::Up, &turn_prior),
            Some(Direction::Left),
            "so must no prediction at all"
        );
    }

    /// The point of relative turns: the SAME habit must produce a different
    /// compass direction under a different heading. An absolute prior cannot
    /// do this, which is why a left-breaking player was unlearnable.
    #[test]
    fn the_turn_habit_rotates_with_the_heading() {
        let turn_prior = [0.10, 0.80, 0.10]; // habitually breaks Left

        // Heading Up: left is Left.
        assert_eq!(
            mask_to_legal(None, &[Direction::Left, Direction::Right], Direction::Up, &turn_prior),
            Some(Direction::Left)
        );
        // Heading Right: left is Up. Same habit, different compass answer.
        assert_eq!(
            mask_to_legal(None, &[Direction::Up, Direction::Down], Direction::Right, &turn_prior),
            Some(Direction::Up)
        );
        // Heading Down: left is Right.
        assert_eq!(
            mask_to_legal(None, &[Direction::Left, Direction::Right], Direction::Down, &turn_prior),
            Some(Direction::Right)
        );
    }

    /// Masking must never override a forecast the player CAN execute — that
    /// would replace the model's read with a base-rate guess.
    #[test]
    fn a_possible_forecast_is_left_alone() {
        let turn_prior = [0.10, 0.80, 0.10];
        // Straight is available, so this is a free choice, not a forced turn —
        // the model's read is the best information available and stands.
        let legal = [Direction::Up, Direction::Left, Direction::Right];
        assert_eq!(
            mask_to_legal(Some(Direction::Right), &legal, Direction::Up, &turn_prior),
            Some(Direction::Right),
            "a legal prediction stands even when the habit disagrees"
        );
    }

    /// At a FORCED turn the absolute models have measured NEGATIVE skill —
    /// they read a left-breaking persona at 38% against a 50% baseline — so
    /// their opinion is discarded in favour of the relative prior, even when
    /// what they named is technically legal.
    #[test]
    fn a_forced_turn_overrides_the_model_with_the_habit() {
        let turn_prior = [0.10, 0.80, 0.10]; // habitually breaks Left
        let legal = [Direction::Left, Direction::Right]; // Up (straight) blocked

        assert_eq!(
            mask_to_legal(Some(Direction::Right), &legal, Direction::Up, &turn_prior),
            Some(Direction::Left),
            "a forced turn must be answered by the habit, not by the model"
        );
    }

    /// Boxed in with nowhere to go: pass the forecast through rather than
    /// inventing a move, so the caller sees the model's actual opinion.
    #[test]
    fn masking_with_no_legal_moves_is_a_passthrough() {
        let turn_prior = [1.0 / 3.0; TURNS];
        assert_eq!(
            mask_to_legal(Some(Direction::Up), &[], Direction::Up, &turn_prior),
            Some(Direction::Up)
        );
    }

    /// The whole point of the metric: a model that only ever predicts the
    /// commonest turn must score ZERO lift, however high its raw hit rate.
    /// Measured against a real opponent this is not hypothetical — the CPU
    /// scores 98.3% and so does "always straight", so its true lift is 0.
    /// A CPU that only ever predicts the player's usual move must be
    /// indistinguishable from the trivial baseline, however high its raw hit
    /// rate. It agrees with the baseline on essentially every frame, so there
    /// is no evidence either way — and the metric must say so rather than
    /// print a 98% and call it a read.
    #[test]
    fn predicting_the_usual_thing_proves_nothing() {
        let mut r = ReadRate::default();
        // Player goes straight 80% of the time. CPU always guesses straight —
        // exactly what the baseline does, so they hit and miss together.
        for _ in 0..80 {
            r.record(3, Turn::Straight, true);
        }
        for _ in 0..20 {
            r.record(3, Turn::Left, false);
        }

        assert!(r.rate() > 0.79, "raw rate looks respectable: {}", r.rate());
        assert!(
            r.discordant() <= 1,
            "a copy of the baseline disagrees with it essentially never"
        );
        assert!(!r.is_ready(), "and therefore never accumulates evidence");
        assert!(!r.is_significant());
    }

    /// Catching the turns the baseline misses is the only thing that counts —
    /// and it is exactly what shows up as discordant pairs.
    #[test]
    fn calling_the_turns_is_what_earns_significance() {
        let mut r = ReadRate::default();
        for _ in 0..80 {
            r.record(3, Turn::Straight, true); // both right, no evidence
        }
        for _ in 0..20 {
            r.record(3, Turn::Left, true); // CPU right, baseline wrong
        }

        // 20 turns, plus the very first frame where the baseline had no data
        // yet and therefore no call.
        assert_eq!(r.cpu_only, 21, "every turn called is a point of evidence");
        assert_eq!(r.mode_only, 0);
        assert!(r.is_ready());
        assert!(r.is_significant(), "p = {}", r.p_value());
        assert!(r.p_value() < 0.001);
    }

    /// The falsifiability check: a CPU no better than the baseline must NOT
    /// come out significant, however many frames it is given. A metric that
    /// cannot report failure is not evidence of anything.
    #[test]
    fn a_coin_flipping_cpu_is_never_significant() {
        let mut r = ReadRate::default();
        // Disagreements split evenly — the CPU wins some, the baseline wins
        // just as many. That is what "no better" looks like.
        for i in 0..200 {
            let turn = if i % 3 == 0 { Turn::Left } else { Turn::Straight };
            r.record(3, turn, i % 2 == 0);
        }
        assert!(r.is_ready(), "plenty of disagreements to judge on");
        assert!(
            !r.is_significant(),
            "even split must not read as a read: b={} c={} p={}",
            r.cpu_only,
            r.mode_only,
            r.p_value()
        );
    }

    /// The exact McNemar tail must match hand-computed values, or every claim
    /// downstream of it is decoration.
    #[test]
    fn mcnemar_p_value_is_exact() {
        // 20 disagreements, all won by the CPU: p = 0.5^20.
        let mut all = ReadRate::default();
        all.cpu_only = 20;
        all.mode_only = 0;
        assert!((all.p_value() - 0.5f32.powi(20)).abs() < 1e-9);

        // A dead-even split is the least significant result possible.
        let mut even = ReadRate::default();
        even.cpu_only = 10;
        even.mode_only = 10;
        assert!(even.p_value() > 0.4, "p = {}", even.p_value());

        // No disagreements at all: nothing to conclude.
        assert_eq!(ReadRate::default().p_value(), 1.0);
    }

    /// Uniform chance is reported honestly per decision — never the 1/4 the
    /// UI used to claim, because a reversal is never a legal option.
    #[test]
    fn uniform_chance_never_assumes_four_options() {
        let mut r = ReadRate::default();
        for _ in 0..10 {
            r.record(2, Turn::Straight, true); // two ways out -> 1/2
        }
        for _ in 0..10 {
            r.record(3, Turn::Straight, true); // three ways out -> 1/3
        }
        let expected = (10.0 * 0.5 + 10.0 / 3.0) / 20.0;
        assert!((r.uniform_chance() - expected).abs() < 1e-5);
        assert!(r.uniform_chance() > 0.25, "never the old 25% claim");
    }

    /// A read rate must survive a save/load cycle, or the cross-session curve
    /// the product is built around resets every launch.
    #[test]
    fn read_rate_survives_persistence() {
        let mut brain = CpuBrain::new();
        for _ in 0..40 {
            brain.lifetime_read.record(3, Turn::Left, true);
        }
        let (restored, report) = CpuBrain::from_bytes_report(&brain.to_bytes()).unwrap();
        assert_eq!(restored.lifetime_read.samples, 40);
        assert_eq!(restored.lifetime_read.hits, 40);
        assert!(!report.is_partial());
    }

    /// A blob written before the read-rate section existed must still restore
    /// cleanly — no scary partial-restore banner for a player who lost nothing.
    #[test]
    fn a_brain_without_a_read_rate_section_restores_clean() {
        let mut brain = CpuBrain::new();
        brain.opp_pred_hits = 5;
        brain.opp_pred_total = 9;
        let bytes = brain.to_bytes();
        // Drop the trailing section by rewriting the section count.
        let mut older = bytes.clone();
        let count = u16::from_le_bytes(older[4..6].try_into().unwrap());
        older[4..6].copy_from_slice(&(count - 1).to_le_bytes());

        let (restored, report) = CpuBrain::from_bytes_report(&older).unwrap();
        assert!(report.ensemble_kept);
        assert_eq!(report.sections_skipped, 0, "a missing section is not a skip");
        assert_eq!(restored.lifetime_read.samples, 0);
        assert_eq!((restored.opp_pred_hits, restored.opp_pred_total), (5, 9));
    }

    /// A poisoned brain must degrade to a usable one, not to a silently
    /// useless one. NaN is the dangerous case precisely because it does NOT
    /// crash: it propagates through `prior_distribution`, makes every
    /// comparison false, and leaves an opponent that has quietly stopped
    /// thinking with nothing reported.
    #[test]
    fn a_poisoned_brain_is_sanitized_on_load() {
        let mut brain = CpuBrain::new();
        brain.remember([f32::NAN; CPU_FEATURE_DIM], Direction::Up, 1.0);
        brain.remember([0.5; CPU_FEATURE_DIM], Direction::Down, 1.0);
        brain
            .opp_brain
            .remember([f32::INFINITY; PLAYER_FEATURE_DIM], Direction::Left);
        brain.tally = [f32::NAN; 4];
        brain.tail_len = usize::MAX;
        brain.opp_pred_hits = 900;
        brain.opp_pred_total = 10;
        brain.ensemble.active = 999;

        let (clean, report) = CpuBrain::from_bytes_report(&brain.to_bytes()).unwrap();

        assert_eq!(clean.episodes.len(), 1, "the NaN episode is dropped");
        assert!(report.cpu_episodes_dropped >= 1);
        assert!(clean.opp_brain.episodes.is_empty(), "the Inf episode is dropped");
        assert!(
            clean.prior_distribution().iter().all(|p| p.is_finite()),
            "a poisoned tally must not yield a NaN prior"
        );
        assert!((1..=16).contains(&clean.tail_len));
        assert!(clean.opp_pred_accuracy() <= 1.0, "accuracy cannot exceed 100%");
        assert!(clean.ensemble.active < ENSEMBLE_MODELS);
    }

    /// A mine has no countdown to read: entering its trigger ring IS the
    /// detonation. So the threat question is not "is it about to go off" but
    /// "would stepping here set it off".
    #[test]
    fn an_armed_mine_makes_its_trigger_ring_unsafe() {
        let mut game = WormGame::with_size(120, 38);
        let t = crate::game::MINE_TRIGGER_CELLS as u16;
        game.bombs.push(crate::game::Bomb {
            x: 60,
            y: 20,
            fuse: crate::game::BOMB_FUSE_FRAMES,
            armed_in: 0,
            disguise: 5,
            owner: 0,
        });

        assert!(
            cell_threatened_by_bomb(&game, 60, 20, 0),
            "standing on an armed mine is not survivable"
        );
        assert!(
            cell_threatened_by_bomb(&game, 60 + t, 20, 0),
            "the edge of the trigger ring still trips it"
        );
        assert!(
            !cell_threatened_by_bomb(&game, 60 + t + 1, 20, 0),
            "one cell outside the ring is safe to stand — the arms are only \
             lethal at the moment of detonation, and treating the whole cross \
             as untouchable would wall the CPU out of the board"
        );
    }

    /// The arming window is a real dash-through, so the CPU must be able to
    /// use it rather than treating a fresh mine as instantly lethal.
    #[test]
    fn an_arming_mine_can_still_be_crossed() {
        let mut game = WormGame::with_size(120, 38);
        game.bombs.push(crate::game::Bomb {
            x: 60,
            y: 20,
            fuse: crate::game::BOMB_FUSE_FRAMES,
            armed_in: crate::game::MINE_ARM_FRAMES,
            disguise: 5,
            owner: 0,
        });
        assert!(
            !cell_threatened_by_bomb(&game, 60, 20, 0),
            "an inert mine is genuinely crossable while it arms"
        );

        game.bombs[0].armed_in = 1;
        assert!(
            cell_threatened_by_bomb(&game, 60, 20, 0),
            "about to arm with us inside the ring is a trap, not a window"
        );
    }

    /// Our own mines cannot kill us, so dodging them was the AI avoiding
    /// blasts that were physically harmless to it.
    #[test]
    fn our_own_mine_is_never_a_threat() {
        let mut game = WormGame::with_size(120, 38);
        game.bombs.push(crate::game::Bomb {
            x: 60,
            y: 20,
            fuse: 1,
            armed_in: 0,
            disguise: 5,
            owner: 1,
        });
        assert!(!cell_threatened_by_bomb(&game, 60, 20, 0));
    }

    /// Dying must never make the fatal direction *more* attractive. The k-NN
    /// vote already zero-weights crash episodes; the direction prior did not,
    /// and the prior is what carries the decision when memory confidence is
    /// low — exactly the situation a recent death describes.
    #[test]
    fn death_does_not_reinforce_the_fatal_direction() {
        let mut brain = CpuBrain::new();
        // Die going Right, from a clean prior.
        brain.remember([0.0; CPU_FEATURE_DIM], Direction::Right, 0.0);

        assert_eq!(
            brain.tally[dir_index(Direction::Right)],
            0.0,
            "a crash episode must earn the fatal direction no credit at all"
        );
    }

    /// The sharper statement: dying going one way must leave that way strictly
    /// less favoured than surviving going the same way.
    #[test]
    fn dying_is_worth_strictly_less_than_surviving() {
        let mut died = CpuBrain::new();
        died.remember([0.0; CPU_FEATURE_DIM], Direction::Right, 0.0);

        let mut lived = CpuBrain::new();
        lived.remember([0.0; CPU_FEATURE_DIM], Direction::Right, 0.0001);

        let i = dir_index(Direction::Right);
        assert!(
            died.prior_distribution()[i] < lived.prior_distribution()[i],
            "even a barely-rewarded survival must outrank a death"
        );
    }

    /// The complement: a genuinely rewarded move still earns its credit, so
    /// the fix cannot be mistaken for "the prior stopped learning".
    #[test]
    fn survival_still_reinforces_its_direction() {
        let mut brain = CpuBrain::new();
        let before = brain.prior_distribution()[dir_index(Direction::Left)];
        brain.remember([0.0; CPU_FEATURE_DIM], Direction::Left, 12.0);
        let after = brain.prior_distribution()[dir_index(Direction::Left)];
        assert!(after > before, "a surviving move must gain prior share");
    }

    /// The survival floor must scale with the cycle's own body, because the
    /// only thing a snake has to outrun is itself. The floor this replaced was
    /// an arena FRACTION, which asked the same 547+ cells of a 3-cell snake as
    /// of a 100-cell one.
    #[test]
    fn escape_floor_scales_with_length_not_arena() {
        let mut game = WormGame::with_size(120, 38);

        game.cycles[1].positions = vec![(10, 10); 3];
        let short = escape_floor_cells(&game, 1);

        game.cycles[1].positions = vec![(10, 10); 40];
        let long = escape_floor_cells(&game, 1);

        assert!(
            long > short,
            "a longer cycle must demand more room ({long} vs {short})"
        );
        assert_eq!(short, 3.0 * ESCAPE_LENGTH_MULTIPLE + ESCAPE_MARGIN_CELLS);

        // The old arena-fraction floor demanded at least 0.12 * 120 * 38 = 547
        // cells regardless of length, which a short cycle can never justify.
        let old_absolute_minimum = 0.12 * 120.0 * 38.0;
        assert!(
            short < old_absolute_minimum,
            "a 3-cell cycle must not be held to a whole-arena floor"
        );
    }

    /// Food already swallowed is body the cycle does not have yet. Ignoring
    /// `pending_growth` is the classic snake-AI death: enter a 12-cell pocket
    /// immediately after eating a 9 and the tail stops retracting behind you.
    #[test]
    fn escape_floor_counts_owed_growth() {
        let mut game = WormGame::with_size(120, 38);
        game.cycles[1].positions = vec![(10, 10); 5];

        game.cycles[1].pending_growth = 0;
        let lean = escape_floor_cells(&game, 1);

        game.cycles[1].pending_growth = 9;
        let owed = escape_floor_cells(&game, 1);

        assert_eq!(
            owed - lean,
            9.0 * ESCAPE_LENGTH_MULTIPLE,
            "owed growth must count toward the body we have to outrun"
        );
    }

    /// A brain saved by a pre-WRM2 build must still load. This is the actual
    /// upgrade path for every player who already has a corpus in IndexedDB —
    /// if it regresses, they all silently meet a brand-new opponent.
    #[test]
    fn legacy_wrm1_brain_still_loads() {
        let mut original = CpuBrain::new();
        original.remember([0.5; CPU_FEATURE_DIM], Direction::Right, 2.0);
        original.opp_brain.remember([0.25; PLAYER_FEATURE_DIM], Direction::Down);
        original.opp_pred_hits = 17;
        original.opp_pred_total = 40;

        // Exactly what the old to_bytes emitted: magic + one bincode blob.
        let mut legacy = BRAIN_MAGIC_V1.to_le_bytes().to_vec();
        legacy.extend(bincode::serialize(&original).unwrap());

        let (restored, report) =
            CpuBrain::from_bytes_report(&legacy).expect("legacy corpora must keep loading");

        assert_eq!(report.format, 1);
        assert!(!report.is_partial(), "a readable WRM1 blob loses nothing");
        assert_eq!(restored.episodes.len(), 1);
        assert_eq!(restored.opp_brain.episodes.len(), 1);
        assert_eq!((restored.opp_pred_hits, restored.opp_pred_total), (17, 40));
    }

    /// Loading legacy and re-saving must produce the new format, so the WRM1
    /// read path drains as players return rather than living forever.
    #[test]
    fn legacy_brain_is_upgraded_on_next_save() {
        let mut original = CpuBrain::new();
        original.opp_pred_hits = 3;
        original.opp_pred_total = 9;
        let mut legacy = BRAIN_MAGIC_V1.to_le_bytes().to_vec();
        legacy.extend(bincode::serialize(&original).unwrap());

        let restored = CpuBrain::from_bytes(&legacy).unwrap();
        let resaved = restored.to_bytes();

        assert_eq!(
            u32::from_le_bytes(resaved[0..4].try_into().unwrap()),
            BRAIN_MAGIC_V2
        );
        let (again, report) = CpuBrain::from_bytes_report(&resaved).unwrap();
        assert_eq!(report.format, 2);
        assert_eq!((again.opp_pred_hits, again.opp_pred_total), (3, 9));
    }

    #[test]
    fn tri_shot_needs_forward_arc() {
        let mut game = WormGame::with_size(120, 38);
        game.cycles[1].head = (30, 20);
        game.cycles[1].direction = Direction::Right;
        game.cycles[1].held_powerup = Some(crate::game::PowerUpKind::TriShot);
        game.cycles[0].head = (25, 20); // behind the firing direction
        assert!(
            !should_fire(&mut game, 1),
            "bolts travel forward — a target behind the head is unhittable"
        );
        game.cycles[0].head = (35, 20); // in front
        assert!(should_fire(&mut game, 1));
    }

    #[test]
    fn dead_episodes_do_not_vote() {
        let brain = CpuBrain::new();
        let recalled = vec![
            Recalled {
                surviving_dir: Direction::Up,
                seq: 5,
                distance: 0.05,
                reward: 0.0,
            },
            Recalled {
                surviving_dir: Direction::Up,
                seq: 6,
                distance: 0.05,
                reward: 0.0,
            },
            Recalled {
                surviving_dir: Direction::Down,
                seq: 7,
                distance: 0.05,
                reward: 4.0,
            },
        ];
        let agg = aggregate(&brain, &recalled, 10, 200, &VecDeque::new());
        assert!(
            agg.distribution[dir_index(Direction::Down)]
                > agg.distribution[dir_index(Direction::Up)],
            "two instant-death Up episodes must not outvote one surviving Down"
        );
    }

    #[test]
    fn own_bolts_are_not_threats() {
        let mut game = WormGame::with_size(120, 38);
        game.projectiles.push(crate::game::Projectile {
            x: 20,
            y: 20,
            dx: 1,
            dy: 0,
            steps_left: 7,
            from: 1,
        });
        assert!(
            !cell_threatened_by_projectile(&game, 24, 20),
            "a CPU-fired bolt cannot hit the CPU and must not register as a threat"
        );
        game.projectiles[0].from = 0;
        assert!(
            cell_threatened_by_projectile(&game, 24, 20),
            "the same bolt fired by the player is a real threat"
        );
    }

    #[test]
    fn prior_is_uniform_when_empty() {
        let brain = CpuBrain::new();
        let p = brain.prior_distribution();
        for v in &p {
            assert!((v - 0.25).abs() < 1e-6);
        }
        assert!(brain.prior_strength() < 1e-6);
    }

    #[test]
    fn cold_start_confidence_is_zero() {
        let brain = CpuBrain::new();
        let agg = aggregate(&brain, &[], 0, 0, &VecDeque::new());
        assert_eq!(agg.confidence, 0.0);
        assert_eq!(agg.distribution, [0.25, 0.25, 0.25, 0.25]);
    }

    #[test]
    fn seq_not_size_ages_recency() {
        // Fill past the cap, ensure cpu_seq keeps climbing while episodes cap.
        let mut brain = CpuBrain::new();
        let v = encode_situation_stub();
        for _ in 0..MAX_EPISODES + 50 {
            brain.remember(v, Direction::Up, 1.0);
        }
        assert_eq!(brain.episodes.len(), MAX_EPISODES);
        assert!(brain.cpu_seq > MAX_EPISODES as u32);
    }

    fn encode_situation_stub() -> [f32; CPU_FEATURE_DIM] {
        let mut v = [0.0f32; CPU_FEATURE_DIM];
        v[0] = 1.0;
        let mut norm = v.iter().map(|value| value * value).sum::<f32>();
        norm = norm.sqrt();
        if norm > 0.0 {
            for value in &mut v {
                *value /= norm;
            }
        }
        v
    }

    /* ----------------------------- Spike 1 ----------------------------- */

    fn encode_player_context_stub() -> [f32; PLAYER_FEATURE_DIM] {
        let mut v = [0.0f32; PLAYER_FEATURE_DIM];
        v[0] = 1.0;
        let mut norm = v.iter().map(|value| value * value).sum::<f32>();
        norm = norm.sqrt();
        if norm > 0.0 {
            for value in &mut v {
                *value /= norm;
            }
        }
        v
    }

    #[test]
    fn spike_1_player_brain_predicts_pattern() {
        // Spike 1 (refactored): Validate the integrated `CpuBrain.opp_brain`
        // can learn a deterministic player sequence.
        // Sequence: Up -> Right -> Down -> Left (repeating).
        let pattern = [
            Direction::Up,
            Direction::Right,
            Direction::Down,
            Direction::Left,
        ];
        let mut brain = CpuBrain::new();
        let tail = VecDeque::new();

        // Feed 20 cycles of the pattern to build memory.
        for _ in 0..20 {
            for i in 0..pattern.len() {
                let _last = pattern[i];
                let next = pattern[(i + 1) % pattern.len()];
                let ctx = encode_player_context_stub(); // Stub: one-hot for 'last' in real use
                record_player_episode(&mut brain, ctx, next, true);
            }
        }

        // The prediction will default to prior because our stub is always the same,
        // so this test validates that the *infrastructure* (record_player_episode,
        // predict_player_move, CpuAggregate) compiles and runs without panic.
        // A true pattern test requires a game state, which is covered in Spike 2.
        let agg = predict_player_move(&crate::WormGame::with_size(120, 38), &brain, &tail);
        assert!((0.0..=1.0).contains(&agg.confidence));
    }

    /* ----------------------------- Spike 2 ----------------------------- */

    #[test]
    fn spike_2_transition_features_encode_corner_patterns() {
        // Corner patterns must still be encoded — but in TURN space now.
        //
        // This used to assert a 4x4 matrix over ABSOLUTE directions at slots
        // 13..29. That representation smeared one relative habit across
        // sixteen cells (a left-break reads as Right->Up, Up->Left, Left->Down
        // or Down->Right depending purely on which way the player happened to
        // be facing) and, despite its name, carried no order at all — it was a
        // bag of pair counts. The vector now records the turn mix and the most
        // recent turn directly.
        let game = WormGame::with_size(120, 38);
        let mut tail: VecDeque<Direction> = VecDeque::new();

        // Right -> Up -> Left -> Down -> Right. Every step is the SAME turn,
        // which is exactly the point: one habit, one feature.
        for d in [
            Direction::Right,
            Direction::Up,
            Direction::Left,
            Direction::Down,
            Direction::Right,
        ] {
            tail.push_back(d);
        }

        let ctx = encode_player_context(&game, &tail);
        let turn = Turn::from_dirs(Direction::Right, Direction::Up)
            .expect("Right -> Up is a legal quarter turn");

        assert!(
            ctx[25 + turn_index(turn)] > 0.0,
            "the repeated turn must register in the recent-turn mix"
        );
        assert!(
            ctx[28 + turn_index(turn)] > 0.0,
            "the most recent turn must be marked, so an alternating player is \
             distinguishable from a consistent one"
        );
        // And the other two turn classes must stay empty — a consistent
        // turner must not look like a mixed one.
        for other in [Turn::Straight, Turn::Left, Turn::Right] {
            if other != turn {
                assert_eq!(
                    ctx[28 + turn_index(other)],
                    0.0,
                    "only the last turn is marked"
                );
            }
        }
    }

    /* ---------------------- regression: encoder fixes ---------------------- */

    /// Regression: wall_distance must stop at the ring-2 arena wall, not count
    /// through it into the outer corridor (it never consulted the grid).
    #[test]
    fn wall_distance_stops_at_arena_wall() {
        let game = WormGame::with_size(120, 38);
        assert!(game.has_corridor(), "test assumes a corridor arena");
        let (hx, hy) = (5u16, 10u16);
        let walls = wall_distance(&game, hx, hy);
        // Left: free cells x=4,3 then wall at x=2 -> distance 2.
        assert_eq!(walls[2], 2.0, "left wall distance must stop at ring 2");
        // Right: free cells x=6..(w-4) then wall at x=w-3.
        let expect_right = (game.width - 3 - hx - 1) as f32;
        assert_eq!(walls[3], expect_right);
        // Up: free cells y=9..3 then wall at y=2.
        assert_eq!(walls[0], (hy - 3) as f32);
        // Down: free cells y=11..(h-4) then wall at y=h-3.
        let expect_down = (game.height - 3 - hy - 1) as f32;
        assert_eq!(walls[1], expect_down);
    }

    /// Regression: food/player directly behind or perpendicular must read as
    /// `cap`, not 0 ("right here"); off-axis targets must pay the
    /// perpendicular Manhattan offset.
    #[test]
    fn projection_distances_respect_half_plane() {
        let mut game = WormGame::with_size(120, 38);
        let (hx, hy) = (20u16, 20u16);
        // Food 3 cells to the LEFT: ahead for Left (3), behind for Right (cap),
        // perpendicular for Up/Down (cap — old code read these as 0).
        game.food_items = vec![(hx - 3, hy, 1)];
        let food = nearest_food_distance(&game, hx, hy, 6.0);
        assert_eq!(food, [6.0, 6.0, 3.0, 6.0]);
        // Food 2 ahead and 4 to the side for Right: along 2 + perp 4 = 6.
        game.food_items = vec![(hx + 2, hy - 4, 1)];
        let food = nearest_food_distance(&game, hx, hy, 8.0);
        assert_eq!(food[3], 6.0);
        // Player 4 cells to the left of the CPU.
        game.cycles[0].head = (hx - 4, hy);
        let player = directional_player_distance(&game, hx, hy, 6.0);
        assert_eq!(player, [6.0, 6.0, 4.0, 6.0]);
    }

    /// Regression: free_step must agree with physics (passable) — a punched
    /// Hole is steppable, a live bomb cell is fatal.
    #[test]
    fn free_step_matches_passable_semantics() {
        let mut game = WormGame::with_size(120, 38);
        let (hx, hy) = (20u16, 20u16);
        game.grid[hy as usize][(hx + 1) as usize] = CellType::Hole;
        assert!(
            free_step(&game, hx, hy, Direction::Right),
            "holes are passable"
        );
        game.grid[hy as usize][(hx + 1) as usize] = CellType::Empty;
        game.bombs.push(crate::game::Bomb {
            x: hx + 1,
            y: hy,
            fuse: 99,
            disguise: 5,
            armed_in: 0,
            owner: 0,
        });
        assert!(
            !free_step(&game, hx, hy, Direction::Right),
            "bomb cells are fatal"
        );
    }

    /// Regression: count_open_space counts the whole reachable interior with
    /// no 2000-cell cap.
    #[test]
    fn count_open_space_has_no_artificial_cap() {
        let game = WormGame::with_size(120, 38);
        let (cx, cy) = (game.width / 2, game.height / 2);
        let count = count_open_space(&game, cx, cy);
        // Expected: every Empty|Food cell inside the ring-2 arena wall is
        // reachable from the centre of a fresh board (corridor unreachable).
        let mut expected = 0usize;
        for y in 3..(game.height - 3) {
            for x in 3..(game.width - 3) {
                if matches!(
                    game.grid[y as usize][x as usize],
                    CellType::Empty | CellType::Food
                ) {
                    expected += 1;
                }
            }
        }
        assert_eq!(count, expected as f32);
        if expected > 2000 {
            assert!(count > 2000.0, "old cap would truncate this board");
        }
    }

    /// Regression: the CPU's own bombs cannot kill it (detonate owner
    /// exclusion), so they must not register as threats.
    #[test]
    fn own_bomb_is_not_a_threat() {
        let mut game = WormGame::with_size(120, 38);
        let (hx, hy) = (20u16, 20u16);
        game.bombs.push(crate::game::Bomb {
            x: hx,
            y: hy,
            fuse: 1,
            disguise: 5,
            armed_in: 0,
            owner: 1,
        });
        assert!(
            !cell_threatened_by_bomb(&game, hx, hy, 3),
            "own bomb is harmless to the CPU"
        );
        game.bombs.clear();
        game.bombs.push(crate::game::Bomb {
            x: hx,
            y: hy,
            fuse: 1,
            disguise: 5,
            armed_in: 0,
            owner: 0,
        });
        assert!(
            cell_threatened_by_bomb(&game, hx, hy, 3),
            "player bomb is a threat"
        );
    }
}
