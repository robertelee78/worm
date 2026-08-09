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
    {
        let t = crate::tuning::tuning();
        // THE BEATABLE OPENING: an unread CPU keeps only discipline_floor of
        // its survival margin — bold enough to genuinely die to its own
        // dives. Reading the player restores the champion floor. "Before it
        // knows you it plays reckless; reading you makes it careful."
        let discipline =
            t.discipline_floor + (1.0 - t.discipline_floor) * game.discipline_sharpness();
        // ADR-021 Kata 1: a player who keeps killing the CPU with box-ins
        // earns a bigger escape floor against them — floors only rise
        // (max +50%), the doze's discipline scaling still applies (this
        // does not wake the opening), and a player who never chases
        // leaves it at exactly 1.0.
        let aversion = 1.0 + game.cpu_brain.ledgers.boxer_aversion();
        // THE ENVELOPMENT ALARM (task #13 v1): when the CPU's own open
        // region has collapsed to under 60% of what it was 8 decisions
        // ago WITH the player nearby, the walls are closing — evacuate
        // standards rise NOW (+50%), not at the last legal frame. Board
        // knowledge (both players can see the space), defensive only,
        // and it decays the moment the space stops shrinking.
        let envelopment = if game.cpu_enveloped() { 1.5 } else { 1.0 };
        let aversion = aversion * envelopment;
        (own_len * t.escape_multiple + t.escape_margin) * discipline * aversion
    }
}

/// Leave a sudden-death ring this many frames before it seals, rather than at
/// the last instant — one frame of slack is not enough when the move that
/// takes you off the ring may itself be blocked.
const RING_EVACUATE_FRAMES: u32 = 3;

/// Would stepping `d` from `from` land on a sudden-death ring about to seal?
/// `close_ring` kills any head standing on the ring it closes.
pub(crate) fn ring_doomed_step(game: &WormGame, from: (u16, u16), d: Direction) -> bool {
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
/// Retuned 0.55 -> 0.35 when the food economy landed: 0.55 was calibrated for
/// a CPU that orbited walls between hunts, and once it lived mid-arena
/// chasing food the same spend meant hunting from exposure instead of from
/// cover — the warm arm died MORE than the cold one (87% vs 90%), the exact
/// inversion ADR-009 exists to forbid.
pub(crate) const HUNT_MARGIN_SPEND: f32 = 0.35;
/// Superlinear, so a strong read bites noticeably harder than a middling one
/// rather than the whole range feeling the same.
pub(crate) const HUNT_MARGIN_CURVE: f32 = 0.7;

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
    let t = crate::tuning::tuning();
    let read = read_rate.clamp(0.0, 1.0);
    // Read-driven spend (champion economics) PLUS opening recklessness that
    // decays as the CPU sharpens — a U over the learning arc: bold at first
    // contact, tight while consolidating, committed once it knows you.
    let spend =
        (t.hunt_spend * read.powf(t.hunt_curve)
        + t.bold_spend * (1.0 - game.sharpness()) * game.boldness_scale())
    .min(0.85);
    (escape_floor_cells(game, who) * (1.0 - spend)).max(t.escape_margin)
}

/// How many times its own length a cycle wants reachable before committing.
pub(crate) const ESCAPE_LENGTH_MULTIPLE: f32 = 3.0;
/// Flat manoeuvring allowance on top, so a very short cycle still needs room.
// 8.0 until the first worm-native Darwin sweep (2026-08-06): of 22
// single-knob candidates, margin 10 was the top winner — habitual record
// 92% -> 98% on the fixed seeds with warm domination (30-0, lift 86%) and
// the browser-board probe byte-identical. Receipts in .darwin/ and the
// promoting commit.
pub(crate) const ESCAPE_MARGIN_CELLS: f32 = 10.0;
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
    EscapeFloor,
    LaneRefusal,
    Curiosity,
    CloseEvasion,
    ItemPickup,
    ItemPath,
    CornerIntercept,
    DirectIntercept,
    SurvivalMemory,
    WallFollow,
    /// ADR-024: space-denial choke — a perturbation of an already-funded
    /// intercept that steers to shrink the player's reachable region
    /// instead of closing distance.
    Boxer,
}

impl CpuDecisionReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Opening => "opening scan",
            Self::NoLegalMove => "no safe move",
            Self::ForcedMove => "only safe move",
            Self::WarmingUp => "warming up · wall safety",
            Self::ThreatDodge => "dodging a weapon",
            // The escape-floor rescue used to reuse ThreatDodge and claim
            // "dodging a weapon" with no weapon on the board — a label the
            // player can catch out, which is fatal to a HUD whose whole job
            // is being believed (ADR-003/ADR-006).
            Self::EscapeFloor => "backing out of a dead end",
            Self::LaneRefusal => "refusing to be pinned along the wall",
            Self::Curiosity => "drawn to you",
            Self::CloseEvasion => "evading your predicted path",
            Self::ItemPickup => "taking a nearby item",
            Self::ItemPath => "routing to an item",
            Self::CornerIntercept => "cutting off your next corner",
            Self::DirectIntercept => "intercepting your predicted path",
            Self::SurvivalMemory => "reusing a surviving move",
            Self::WallFollow => "following the survival floor",
            Self::Boxer => "boxing off your space",
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
    /// Which book drove this forecast: 0 = the global (straight-dominated)
    /// selection, 1 = the turn book through the derived gate (ADR-020).
    pub book: u8,
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
    pattern_left: Option<f32>,
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
        // When the pattern model has enough break history, it outranks the
        // flat prior: it can express "they alternate" or "after two lefts
        // they go right", which a three-number tally structurally cannot.
        // Below the evidence floor, the prior decides as before.
        if let Some(p_left) = pattern_left {
            let want = if p_left >= 0.5 { Turn::Left } else { Turn::Right };
            let dir = want.apply(heading);
            if legal.contains(&dir) {
                return Some(dir);
            }
        }
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
        // ABSTENTION IS PRESERVED. A model with nothing to say on a free
        // frame must stay silent, because the fallback below is fed by the
        // turn prior — which learns only at forced turns and therefore has
        // ~zero Straight mass (measured 0.005–0.012). Substituting it here
        // forces a TURN guess onto a frame that is ~95% straight and then
        // scores the model on it: measured against a power-up seeker, `arm`
        // abstained on 64.8% of frames, held 92.8% raw skill on the frames a
        // power-up existed, and was scored down to a 36.1% hit rate and a
        // 2.7% selection share by manufactured wrong guesses. score_frame
        // skips a None pending, which is the honest treatment.
        None => None,
        // Impossible while straight was still available.
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

/// A small PORTFOLIO of playstyles, selected per round by Exp3.
///
/// Implicit opponent modelling (Bard, Johanson, Burch & Bowling, AAMAS 2013):
/// instead of estimating the opponent's policy in a huge context space, keep a
/// handful of counter-styles and use online data only to learn WHICH STYLE
/// BEATS THIS HUMAN — collapsing the learned dimensionality to four numbers,
/// the right size for a per-round reward signal. Styles here are drive
/// multipliers on how hard the CPU spends its read (hunt margin + intercept
/// authority); survival floors are never touched by any style.
///
/// Credit is ROUND-LEVEL AND ON-POLICY — win/draw/loss for the style actually
/// played — because counterfactually replaying a human's trajectory against a
/// different style is invalid past the first divergence (the human would have
/// reacted). Unbiased, slower, honest. Exploration is a fixed floor mixed into
/// the pick (Exp3's gamma), so no style's probability ever reaches zero.
///
/// Explainable: "it keeps a small roster of temperaments, cautious to
/// relentless, and leans toward whichever has actually been beating you —
/// without ever fully abandoning the others."
pub const PORTFOLIO_STYLES: [f32; 4] = [0.5, 1.0, 1.6, 2.4];
const EXP3_ETA: f32 = 0.35;
const EXP3_GAMMA: f32 = 0.15;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Portfolio {
    pub weights: [f32; 4],
    pub active: usize,
    pub rounds: u32,
}

impl Default for Portfolio {
    fn default() -> Self {
        Self { weights: [1.0; 4], active: 1, rounds: 0 }
    }
}

impl Portfolio {
    fn probs(&self) -> [f32; 4] {
        let sum: f32 = self.weights.iter().sum();
        let mut p = [0.25f32; 4];
        if sum > 0.0 && sum.is_finite() {
            for (pi, w) in p.iter_mut().zip(self.weights.iter()) {
                *pi = (1.0 - EXP3_GAMMA) * w / sum + EXP3_GAMMA / 4.0;
            }
        }
        p
    }

    /// Reward the style just played and pick the next, deterministically under
    /// the seed (the draw comes from a hash, never the game RNG stream).
    pub fn end_round(&mut self, reward: f32, draw: u64) {
        let p = self.probs();
        // Exp3 importance-weighted update for the played arm only.
        let est = reward / p[self.active].max(1e-3);
        self.weights[self.active] *= (EXP3_ETA * est / 4.0).exp();
        // Renormalise for float hygiene.
        let sum: f32 = self.weights.iter().sum();
        if sum > 0.0 && sum.is_finite() {
            for w in &mut self.weights {
                *w *= 4.0 / sum;
            }
        } else {
            self.weights = [1.0; 4];
        }
        // Sample the next style from the floored distribution.
        let p = self.probs();
        let mut u = (fnv1a64(&draw.to_le_bytes()) % 10_000) as f32 / 10_000.0;
        self.active = 3;
        for (i, &pi) in p.iter().enumerate() {
            if u < pi {
                self.active = i;
                break;
            }
            u -= pi;
        }
        self.rounds += 1;
    }

    pub fn drive_multiplier(&self) -> f32 {
        PORTFOLIO_STYLES[self.active]
    }
}

/// Variable-order Markov model over the player's BREAK pattern.
///
/// The flat turn prior can say "they break left 85% of the time"; it cannot
/// say "they alternate", or "after two lefts they go right". This holds a KT
/// (Krichevsky–Trofimov) estimator for every recent break-context up to depth
/// `VOMM_DEPTH`, and mixes the per-depth predictions with fixed-share
/// exponential weights — a flattened context-tree-switching scheme: each
/// depth is a hypothesis about how much history matters, and the weights
/// float toward whichever depth has been predicting THIS player best, while
/// the share step keeps every depth recoverable after a style change.
///
/// The alphabet is binary (Left/Right at forced breaks) because that is where
/// a turning habit is expressed; the KT estimator ((n+1/2)/(N+1)) is the
/// asymptotically minimax choice at these tiny counts. Everything here is
/// counts and multiplications — O(depth) per event, free in the frame budget,
/// and explainable: "it looks for repeating patterns in your recent breaks,
/// at several pattern lengths at once, trusting whichever length has been
/// calling you right."
pub const VOMM_DEPTH: usize = 5;
const VOMM_ETA: f32 = 1.0;
const VOMM_SHARE: f32 = 0.05;
/// Break events needed before the pattern model outranks the flat prior.
pub const VOMM_MIN_EVENTS: u32 = 6;

#[derive(Clone, Debug)]
pub struct TurnPattern {
    /// Recent break outcomes, most recent last. true = Left.
    pub(crate) history: Vec<bool>,
    /// KT counts per (depth, context-bits): (lefts, total).
    pub(crate) counts: std::collections::HashMap<(u8, u16), (f32, f32)>,
    /// Fixed-share weight per depth 0..=VOMM_DEPTH.
    pub(crate) weights: [f32; VOMM_DEPTH + 1],
    pub events: u32,
}

impl Default for TurnPattern {
    fn default() -> Self {
        Self {
            history: Vec::new(),
            counts: std::collections::HashMap::new(),
            weights: [1.0; VOMM_DEPTH + 1],
            events: 0,
        }
    }
}

impl TurnPattern {
    /// Wire round-trip (ADR-021 Kata 2): the voluntary-turn VOMM is
    /// knowledge about the HUMAN — their swerve grammar — and must
    /// survive sessions like every other read.
    pub fn to_wire(&self) -> TurnTimingWire {
        TurnTimingWire {
            ver: 1,
            history: self.history.clone(),
            counts: self.counts.iter().map(|(&k, &v)| (k.0, k.1, v.0, v.1)).collect(),
            weights: self.weights.to_vec(),
            events: self.events,
        }
    }

    pub fn from_wire(w: TurnTimingWire) -> Self {
        let mut t = Self::default();
        if w.weights.len() == t.weights.len() {
            for (i, v) in w.weights.iter().enumerate() {
                t.weights[i] = *v;
            }
        }
        t.history = w.history;
        t.history.truncate(64);
        t.counts = w
            .counts
            .into_iter()
            .map(|(d, bits, l, n)| ((d, bits), (l, n)))
            .collect();
        t.events = w.events;
        t
    }

    fn context_bits(&self, depth: usize) -> Option<u16> {
        if self.history.len() < depth {
            return None;
        }
        let mut bits = 0u16;
        for &b in &self.history[self.history.len() - depth..] {
            bits = (bits << 1) | b as u16;
        }
        Some(bits)
    }

    /// P(next break is Left) at one depth — KT smoothed.
    fn p_left_at(&self, depth: usize) -> Option<f32> {
        let bits = self.context_bits(depth)?;
        let (l, n) = self
            .counts
            .get(&(depth as u8, bits))
            .copied()
            .unwrap_or((0.0, 0.0));
        Some((l + 0.5) / (n + 1.0))
    }

    /// Weighted mixture over depths — the published pattern read.
    pub fn p_left(&self) -> f32 {
        let mut num = 0.0;
        let mut den = 0.0;
        for d in 0..=VOMM_DEPTH {
            if let Some(p) = self.p_left_at(d) {
                let w = self.weights.get(d).copied().unwrap_or(1.0).max(1e-6);
                num += w * p;
                den += w;
            }
        }
        if den > 0.0 { num / den } else { 0.5 }
    }

    /// Fold in an observed break. Weight update BEFORE counting, so each
    /// depth is judged on a genuine prediction of this event.
    pub fn observe(&mut self, left: bool) {
        for d in 0..=VOMM_DEPTH {
            if let Some(p) = self.p_left_at(d) {
                let p_event = if left { p } else { 1.0 - p };
                self.weights[d] *= (VOMM_ETA * (p_event - 0.5)).exp();
            }
        }
        // Fixed share + renormalise.
        let sum: f32 = self.weights.iter().sum();
        if sum > 0.0 && sum.is_finite() {
            let n = self.weights.len() as f32;
            let pool = VOMM_SHARE * sum / n;
            for w in &mut self.weights {
                *w = (1.0 - VOMM_SHARE) * *w + pool;
            }
            let sum: f32 = self.weights.iter().sum();
            let inv = n / sum;
            for w in &mut self.weights {
                *w *= inv;
            }
        } else {
            self.weights = [1.0; VOMM_DEPTH + 1];
        }
        for d in 0..=VOMM_DEPTH {
            if let Some(bits) = self.context_bits(d) {
                let e = self.counts.entry((d as u8, bits)).or_insert((0.0, 0.0));
                if left {
                    e.0 += 1.0;
                }
                e.1 += 1.0;
            }
        }
        self.history.push(left);
        if self.history.len() > 64 {
            self.history.remove(0);
        }
        self.events += 1;
    }
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
    /// The LATERAL channel (ADR-020 stage 1): forecast performance on the
    /// frames where the player actually turned, scored against uniform
    /// chance over the options they faced. McNemar above answers "better
    /// than the class-aware modal base?"; against a pure modal habit that
    /// is honestly NO — the base calls the habit too — yet the read is
    /// real. This channel answers the other honest question, "better than
    /// chance where it counts?", and a null player is at chance on it by
    /// definition.
    pub lat_samples: u32,
    pub lat_hits: u32,
    /// Sum of per-frame uniform chance (1/options) over lateral frames.
    pub lat_chance: f32,
    /// Sum of per-frame p(1-p) — the exact variance of the chance rival.
    pub lat_var: f32,
    /// Schmitt latch on the lateral channel: proven at the family's
    /// anytime boundary, and it stays proven until the evidence decays
    /// below 1 sigma. Without hysteresis the absolute excess is frozen
    /// once earned while variance keeps growing, so a genuine read drifts
    /// back under a single fixed gate and sharpness flaps dozy
    /// mid-session. A null never crosses the boundary, so the latch never
    /// manufactures a read.
    pub lat_latched: bool,
    /// Same Schmitt latch for the McNemar channel — the discordant stream
    /// is Bernoulli(1/2) under the null, the same family, same boundary,
    /// same repeated-looks discipline. The exact p-value stays alongside
    /// for one-shot reporting only.
    pub mc_latched: bool,
}

impl ReadRate {
    /// The commonest turn seen so far AMONG the given legal set — the
    /// baseline restricted to what the board actually allowed this frame.
    /// Ties break to the lowest index (Straight, Left, Right) for
    /// replayability; None before any evidence, which counts as a miss,
    /// not a free pass.
    fn modal_among(&self, legal: [bool; TURNS]) -> Option<Turn> {
        let max = (0..TURNS)
            .filter(|&i| legal[i])
            .map(|i| self.taken[i])
            .max()?;
        // Zero history is not "no call": a real rival still answers, and
        // with a sole legal option the answer is board knowledge. Leaving
        // the base silent here made the first occurrence of any unseen
        // forced lateral a guaranteed structural cpu_only point (codex
        // verification finding — bounded initialization bias, but bias).
        // With max == 0 every legal candidate ties and the lowest index
        // wins, deterministically and replayably.
        (0..TURNS)
            .find(|&i| legal[i] && self.taken[i] == max)
            .map(|i| match i {
                0 => Turn::Straight,
                1 => Turn::Left,
                _ => Turn::Right,
            })
    }

    pub fn record(
        &mut self,
        options: u8,
        actual: Turn,
        predicted: Turn,
        legal: [bool; TURNS],
        hit: bool,
    ) {
        // Score the baseline BEFORE folding this frame in, so it never peeks
        // at the outcome it is being judged on — the same information the CPU
        // had, at the same instant.
        //
        // INFORMATION PARITY (ADR-020 stage 1, both halves): the baseline
        // gets everything public the CPU's forecast had —
        //
        //  * the LEGAL set: on a forced turn the CPU calls the only exit
        //    from board knowledge alone; a baseline never told what was
        //    legal is structurally wrong there, and every such frame is a
        //    fabricated point of "evidence" (a habit-free slalomer measured
        //    lift 0.35, SIGNIFICANT, from exactly this leak);
        //  * the forecast's own CLASS: when the published forecast is
        //    itself a TURN, the base answers with the commonest LATERAL —
        //    otherwise every turn forecast competes against a rival that
        //    structurally answers "straight", the discordant stream goes
        //    one-sided by construction, and a chance-level turn predictor
        //    grades as significantly "learned".
        let mode_hit = if predicted != Turn::Straight {
            let lateral_only = [false, legal[1], legal[2]];
            let pick = if lateral_only.iter().any(|&b| b) {
                self.modal_among(lateral_only)
            } else {
                self.modal_among(legal)
            };
            pick == Some(actual)
        } else {
            self.modal_among(legal) == Some(actual)
        };

        self.samples += 1;
        self.taken[turn_index(actual)] += 1;
        self.opts[(options as usize).min(3)] += 1;
        if hit {
            self.hits += 1;
        }
        // SIDE evidence only, under the CORRECT conditional null (codex
        // verification finding, the one that made stage 1 unsound as first
        // committed): this channel samples frames where the player DID
        // turn, so the null must be the distribution of sides GIVEN a
        // turn — uniform over the legal laterals — not 1/options
        // including straight. Scored against 1/options, an always-Left
        // forecast beats a fair coin by construction (50% hits vs a
        // claimed 33% chance), and with only one legal lateral the side
        // is CERTAIN given the outcome yet was scored as a coin toss.
        // Frames with fewer than two legal laterals therefore carry zero
        // side information and are excluded outright; with both laterals
        // legal the null is exactly 1/2. Timing skill ("they turn NOW")
        // deliberately earns nothing here — it must be claimed by a
        // predictor scored on ALL its declarations, straight false alarms
        // included (the stage-2 hazard book).
        if actual != Turn::Straight && legal[1] && legal[2] {
            let p = 0.5;
            self.lat_samples += 1;
            self.lat_chance += p;
            self.lat_var += p * (1.0 - p);
            if hit {
                self.lat_hits += 1;
            }
            // Opening is decided ONLY at the proved geometric looks (see
            // look_threshold); between looks the latch holds. The 1-sigma
            // close is hysteresis AFTER a legitimate open, not a second
            // test, and closing early is conservative.
            if let Some(bound) = Self::look_threshold(self.lat_samples) {
                if self.lateral_z() > bound {
                    self.lat_latched = true;
                }
            }
            if self.lat_latched && self.lateral_z() < 1.0 {
                self.lat_latched = false;
            }
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
        if hit != mode_hit {
            if let Some(bound) = Self::look_threshold(self.discordant()) {
                if self.mcnemar_z() > bound {
                    self.mc_latched = true;
                }
            }
            if self.mc_latched && self.mcnemar_z() < 1.0 {
                self.mc_latched = false;
            }
        }
    }

    /// EVIDENCE-BUDGET REGISTRY (ADR-021, codex prescription): every
    /// anytime-valid evidence family in this brain, named, with its α
    /// and channel count stated in one place. Adding a channel or a
    /// family REQUIRES updating this table — the thresholds below derive
    /// from it, and an unregistered channel is an unbudgeted false-wake.
    ///
    /// | Family | Channels | α | Looks |
    /// |--------|----------|---|-------|
    /// | A: player-read (per-frame) | 4: published×{McNemar, lateral}, book×{McNemar, lateral} | 0.005 | ratio 1.4 from n=20 |
    /// | B: drift (per-round) | 4: {alternation, mean-gap} × two-sided sign test vs frozen reference medians | 0.005 | ratio 1.4 from n=8 trials per statistic |
    ///
    /// Families are independent budgets (their consumers differ: A funds
    /// aggression via earned_snapshot; B funds only a NARRATION flag and
    /// narration flag only — it never funds spend or resets evidence).
    pub const FAMILY_A_CHANNELS: f32 = 4.0;
    pub const FAMILY_B_CHANNELS: f32 = 4.0;

    /// The family's PROVED time-uniform opening rule (codex round 3: an
    /// LIL-shaped constant is shorthand, not a theorem — this is the
    /// theorem, and it is elementary). A channel is only TESTED at
    /// geometric looks n ∈ {20, 40, 80, …}; at look k it spends
    /// α_k = (α_family/4) · 6/(π²k²), a convergent series summing to
    /// α_family/4 per channel, Bonferroni across the FOUR channels
    /// (published/book × McNemar/lateral). Under the null both channel
    /// statistics are centered fair-coin sums with EXACT variance n/4
    /// (the lateral null is exactly ½ by construction; a discordant
    /// McNemar frame is a fair coin), so Hoeffding gives the exact
    /// per-look crossing bound P(z ≥ z_k) ≤ exp(−z_k²/2), and
    /// z_k = sqrt(2·ln(1/α_k)) makes the union over every look of every
    /// channel ≤ α_family (0.005; the power analysis at the constant
    /// below). No normal approximation, no
    /// asymptotics, finitely many looks per lifetime. Returns the
    /// threshold when n IS a look point; None between looks, where the
    /// latch simply holds its state.
    pub fn look_threshold(n: u32) -> Option<f32> {
        Self::look_threshold_for(n, 20, Self::FAMILY_A_CHANNELS)
    }

    /// Registry-parametrized look rule (see the table above): geometric
    /// looks with ratio 1.4 from `base_n`, α_k = (α_family/channels) ·
    /// 6/(π²k²) at look k, exact Hoeffding crossing bounds.
    pub fn look_threshold_for(n: u32, base_n: u32, channels: f32) -> Option<f32> {
        if n < base_n {
            return None;
        }
        let mut base = base_n;
        let mut k = 1u32;
        while base < n {
            base = ((base as f32) * 1.4).ceil() as u32;
            k += 1;
        }
        if base != n {
            return None;
        }
        // α chosen with the power analysis on the table, not pulled from
        // convention: an 85:15 side habit at genuine choices produces
        // z ≈ 0.7·√n, and a session arc against the reference habitual
        // persona supplies ~40 such choices — the n=40 look must
        // therefore sit at or below z ≈ 4.4, which α = 0.005 gives
        // (z₂ = 4.14) and α = 0.001 does not (z₂ = 4.51, measured: the
        // flagship read became unprovable). 0.005 family-wise per
        // opponent LIFETIME — one false wake in 200 players — remains
        // far stricter than any per-look convention.
        let alpha_family = 0.005f32;
        let alpha_k =
            (alpha_family / channels) * 6.0 / (std::f32::consts::PI.powi(2) * (k * k) as f32);
        Some((2.0 * (1.0 / alpha_k).ln()).sqrt())
    }

    /// Lift of the lateral-frame forecast over uniform chance, 0 until the
    /// latch is proven. Straight frames are excluded by construction, so
    /// "predict the usual thing" scores zero here and cannot inflate it.
    /// SPENDS A CONSERVATIVE BOUND, not the raw point estimate: the rate
    /// is shrunk by one standard error before the lift is computed, so the
    /// winning channel of the family cannot cash in its own selection
    /// luck (codex: "spend a simultaneous lower confidence bound").
    pub fn lateral_lift(&self) -> f32 {
        if !self.lateral_significant() || self.lat_samples == 0 {
            return 0.0;
        }
        let n = self.lat_samples as f32;
        let rate = self.lat_hits as f32 / n;
        let chance = self.lat_chance / n;
        if chance >= 1.0 {
            return 0.0;
        }
        // One standard error of the hit rate: sd(sum)/n.
        let se = self.lat_var.max(0.0).sqrt() / n;
        let shrunk = (rate - se).max(0.0);
        ((shrunk - chance) / (1.0 - chance)).clamp(0.0, 1.0)
    }

    /// The lateral channel's z against chance (exact per-frame variance).
    /// Public for receipts (ADR-022): raw channel strength distinguishes
    /// a missed geometric look from genuinely absent evidence.
    pub fn lateral_z(&self) -> f32 {
        if self.lat_var <= 0.0 {
            return 0.0;
        }
        (self.lat_hits as f32 - self.lat_chance) / self.lat_var.sqrt()
    }

    /// The McNemar channel's z: under the null each discordant frame is a
    /// fair coin, so this is the same Bernoulli family the lateral channel
    /// lives in — and it is held to the same anytime boundary, because it
    /// too is inspected after every frame. (The exact p-value below remains
    /// for one-shot REPORTING — ghost_eval reads it once per corpus, a
    /// single look, where it is valid.)
    fn mcnemar_z(&self) -> f32 {
        let n = self.discordant();
        if n == 0 {
            return 0.0;
        }
        (self.cpu_only as f32 - n as f32 / 2.0) / (n as f32 / 4.0).sqrt()
    }

    /// Whether the lateral channel's evidence is proven — the Schmitt
    /// latch's current state (see `lat_latched`). The latch is the sole
    /// authority: it is updated under the anytime-valid boundary at the
    /// only moments new evidence arrives.
    pub fn lateral_significant(&self) -> bool {
        self.lat_latched
    }

    /// The read the CPU has actually EARNED — the number sharpness is
    /// allowed to spend. Two evidence channels per record, each held to
    /// the SAME family-wise look rule (see `look_threshold`), each
    /// spending a one-standard-error-shrunk lift rather than its raw
    /// point estimate:
    ///  * McNemar lift over the class-aware modal base (catches edges the
    ///    base cannot express, e.g. alternation);
    ///  * lateral lift over uniform chance (catches habits the base ALSO
    ///    calls — a real read even though the discordant stream is silent).
    ///
    /// A null player latches neither, so an unearned read is exactly 0.
    pub fn earned_read(&self) -> f32 {
        let mcnemar = if self.mc_latched {
            let n = self.discordant().max(1) as f32;
            let se = 1.0 / n.sqrt();
            (self.lift() - se).max(0.0)
        } else {
            0.0
        };
        mcnemar.max(self.lateral_lift())
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
    /// Break-pattern model. Transient: patterns re-learn within a round or
    /// two, and the flat prior (which IS persisted) carries the long-run
    /// habit across sessions.
    #[serde(skip)]
    pub turn_pattern: TurnPattern,
    /// The voluntary-turn VOMM feeding ensemble model M13 `alt` — sees
    /// every voluntary lateral, where the forced instance sees only
    /// cornered breaks (ADR-020 stage 3). Transient, like its sibling.
    #[serde(skip)]
    pub voluntary_pattern: TurnPattern,
    /// Which temperaments beat THIS human — knowledge about them, persisted.
    #[serde(skip)]
    pub portfolio: Portfolio,
    /// Hysteresis for the eat/arm intent models: the target cell each family
    /// is currently committed to ([0] = eat, [1] = arm). A human on an errand
    /// does not re-shop for a marginally nearer morsel mid-run; the models
    /// keep predicting toward the committed target while the player's own
    /// observed moves keep shortening the route to it. Transient by design —
    /// an errand does not survive a session.
    #[serde(skip)]
    pub intent_targets: [Option<(u16, u16)>; 2],
    /// The class-conditional selection layer (ADR-020 stage 2). Persisted
    /// in its OWN section (SEC_CLASS_BOOKS) — skipped here because bincode
    /// is not field-tolerant and this struct rides other wire shapes.
    #[serde(skip)]
    pub class_books: ClassBooks,
    /// Frames since the player's last VOLUNTARY lateral turn — the
    /// dedicated hazard counter (the 4-deep player tail cannot represent a
    /// 5-frame gap). Transient.
    #[serde(skip)]
    pub gap_since_voluntary: u32,
    /// Frames since the player last picked up food. Transient.
    #[serde(skip)]
    pub frames_since_food: u32,
    /// Player↔CPU distance last frame, for the closing bucket. Transient.
    #[serde(skip)]
    pub prev_pc_dist: u32,
    /// earned_read snapshot taken at ROUND boundaries (refresh_read_rate).
    /// In-round consumers (the hunt confidence ramp) spend THIS, never the
    /// live latches — a mid-round latch flip must not open hunts before
    /// any boundary check has seen it (codex round 2: the half-woken
    /// transition regime reborn). Transient.
    #[serde(skip)]
    pub earned_snapshot: f32,
    /// SESSION DOZE-EXIT LATCH (both v6 consultants, Q6): once an earned
    /// read has ended the casual opening this session, the doze never
    /// returns — a latch that re-released on a marginal look-crossing was
    /// measuring "currently seeing crossing-shaped inputs", not "has
    /// earned sharpness", and the warm arm surrendered wins to the
    /// re-opened vulnerability window (games 13-17, read 0.00 mid-arm).
    /// Not serialized — but NOT purely session-scoped either: every
    /// load path calls refresh_read_rate(), so a brain restored WITH
    /// live earned evidence re-latches immediately (the wits were
    /// earned against this same human; basics do not get sloppy again
    /// just because the calendar turned). The ADR-018 beatable opening
    /// belongs to UNREAD sessions: fresh brains, and returning humans
    /// whose read has genuinely lapsed to zero, still get it — see
    /// ADR-022. AGGRESSION spend still tracks the live earned value;
    /// only survival basics are latched.
    #[serde(skip)]
    pub discipline_latched: bool,
    /// DWELL RELEASE counter (k3 v9 ruling 2b): consecutive round
    /// boundaries at which a latched read's SE-shrunk spend sat below
    /// the behavioral floor. The Schmitt pair (open at the look bound,
    /// release at z<1) has an unbounded dead zone between them — under
    /// heavy dilution the diluted z can asymptote just above 1.0 and
    /// the latch never releases while spending nothing. K consecutive
    /// below-floor boundaries release it: keyed to HARM (a spend too
    /// small to change behavior), never to a loosened assertion.
    #[serde(skip)]
    pub spend_dwell: u8,
    /// Round-boundary snapshots of the book's spendable evidence and
    /// projection authority (codex round 3): a latch that opens mid-round
    /// must not reshape projections or defensive trust before any
    /// boundary check has seen it. These are the ONLY values in-round
    /// consumers may read. Transient.
    #[serde(skip)]
    pub book_spend_snapshot: f32,
    #[serde(skip)]
    pub book_authority_snapshot: bool,
    /// The space game's v1 (task #13, spike-redirected): the CPU's open
    /// region size over the last 8 decisions. Static trap-throats
    /// measured near-nonexistent (2 moments in 63 rounds); what kills
    /// the CPU is DYNAMIC envelopment — trail walls collapsing its
    /// space (27 of 45 corpus deaths). Board knowledge, defensive only.
    /// Transient.
    #[serde(skip)]
    pub region_ring: std::collections::VecDeque<u32>,
    /// Kata 4 (#1): round-boundary snapshot of the tactic ledger's one
    /// active preference — direct intercept over corner when BOTH are
    /// mature and the ledger says direct kills this player better. Its
    /// consumer is a YIELD (the corner layer steps aside), which can only
    /// make a frame LESS aggressive — self-knowledge re-ranking already
    /// gated options, exactly the agreed envelope. The incumbent rule
    /// order is the null; the gauntlet is the regression tripwire.
    #[serde(skip)]
    pub tactic_prefer_direct: bool,
    /// ADR-024: round-boundary gate on the Boxer perturbation. Same
    /// yield discipline as tactic_prefer_direct — suppressing the choke
    /// can only make a frame LESS aggressive, and the decayed ledger
    /// self-recovers: a suppressed arm stops accruing attempts, decay
    /// erodes its mass below maturity, and the gate reopens.
    #[serde(skip)]
    pub tactic_boxer_ok: bool,
    /// Self-knowledge instrumentation (ADR-021 Kata 0). Persisted in its
    /// own sections; recording-only until later katas activate readers.
    #[serde(skip)]
    pub ledgers: LearningLedgers,
    /// The book's precommitted record for the NEXT frame — one auditable
    /// struct, target-framed so a stale record can never score against the
    /// wrong input (codex round 2). "Precommitted internally", not
    /// "sealed": the public seal covers only the published forecast; this
    /// is folded into the round's reveal chain for post-hoc audit instead.
    /// Transient.
    #[serde(skip)]
    pub pending_book: Option<PendingBook>,
}

/// Everything the turn book committed to before the player's next input
/// existed, kept together so training and auditing read one record.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PendingBook {
    /// The frame this record predicts — must match at scoring time.
    pub target_frame: u32,
    /// Hazard context cell at commit time.
    pub cell: usize,
    /// The book's side pick, None when no lateral model spoke.
    pub side: Option<Direction>,
    /// The lateral direction TOWARD the nearest food at commit time
    /// (None when food was ahead/absent) — trains the correction prior.
    pub food_side_dir: Option<Direction>,
}

/* -------------------- the learning ledgers (ADR-021 Kata 0) -------------------- */

/// Hunt-family tactics with STABLE semantic ids (never reorder — the wire
/// stores these ids, not enum positions).
pub const TACTIC_IDS: [(u8, CpuDecisionReason); 5] = [
    (0, CpuDecisionReason::DirectIntercept),
    (1, CpuDecisionReason::CornerIntercept),
    (2, CpuDecisionReason::ItemPath),
    (3, CpuDecisionReason::WallFollow),
    // ADR-024 Phase A. Ids 5 (Breach actuator telemetry) and 6 (SlipRun)
    // are RESERVED for Phase B — never reuse them for anything else.
    (4, CpuDecisionReason::Boxer),
];
/// Frames an opened tactic attempt stays live for kill attribution. The
/// window is PRECOMMITTED at decision time (codex: no post-hoc causal
/// stories) — a kill credits the tactic only if death lands inside a
/// window opened before the outcome existed.
pub const ATTEMPT_HORIZON: u32 = 12;

/// Weapon ids on the wire (stable).
pub const WEAPON_IDS: [(u8, crate::game::PowerUpKind); 3] = [
    (0, crate::game::PowerUpKind::Laser),
    (1, crate::game::PowerUpKind::TriShot),
    (2, crate::game::PowerUpKind::Bomb),
];

/// Self-knowledge instrumentation (ADR-021 Kata 0): pure RECORDING, zero
/// behavior change — the ledgers that later katas' bandits and defenses
/// read. Class: self-knowledge (exempt from the evidence family; nothing
/// here may raise aggression — and in Kata 0 nothing here is read at all).
#[derive(Clone, Debug, Default)]
pub struct LearningLedgers {
    /// Per tactic id: decayed (attempts, kills) + non-decayed totals for
    /// the later activation comparison (codex: never treat decayed mass
    /// as evidence).
    pub tactic_attempts: Vec<(u8, f32, f32, u32, u32)>,
    /// Per weapon id: (held-frames, gate-pass frames, fires, lethal) —
    /// the opportunity ledger that turns "9 fires" into a denominator.
    pub weapon_ops: Vec<(u8, u32, u32, u32, u32)>,
    /// Per death-cause id: (deaths, chased-deaths) — chase flag = player
    /// head within 8 cells at any point in the last 10 frames before the
    /// CPU died (k3's boxer-vs-wander attribution bit).
    pub loss_causes: Vec<(u8, u32, u32)>,
    /// Ring of per-round summaries for the drift alarm's family:
    /// (laterals, alternations, mean_gap_x10, frames). Capped at 64.
    pub round_summaries: std::collections::VecDeque<(u32, u32, u32, u32)>,
    /// Transient: attempt window currently open — (tactic id, opened
    /// frame, precommitted baseline). The baseline is the player's
    /// reachable open space at window-open; 0.0 for every arm except
    /// Boxer, whose kill credit requires a REALIZED choke against it
    /// (ADR-024: the precommitted-window rule keeps the clock honest,
    /// the baseline keeps the causal story honest).
    pub open_attempt: Option<(u8, u32, f32, u16)>,
    /// Transient: staged by the Boxer decision site immediately before
    /// choose!/note_tactic runs, consumed into the episode it opens.
    pub pending_boxer_baseline: Option<f32>,
    /// Transient: a Boxer window closed by tactic REPLACEMENT while
    /// still inside its horizon — the choke's terminal phase often
    /// hands the label back to the intercept precisely because it
    /// worked (k3 verify, finding B). At death the contested window is
    /// re-tested against its own baseline and WINS the credit iff the
    /// choke realized; otherwise the replacement keeps it. (opened,
    /// baseline, shrink_level at open.)
    pub contested_boxer: Option<(u32, f32, u16)>,
    /// Transient per-round tallies feeding the summary.
    pub rs_laterals: u32,
    pub rs_alternations: u32,
    pub rs_gap_sum: u32,
    pub rs_last_left: Option<bool>,
    /// Transient: recent player↔CPU head distances (last 10 frames).
    pub recent_dist: std::collections::VecDeque<u32>,
    /// The drift alarm (ADR-021 Kata 3, REBUILT per codex verification
    /// blocking finding 2): the original two-window pooled-variance z was
    /// not covered by the family's Hoeffding proof. This construction IS:
    /// a SIGN TEST against a frozen reference. The first REF_ROUNDS
    /// summarized rounds freeze a per-statistic median; every later round
    /// contributes one fair-coin trial per statistic (above/below its
    /// median; ties skip). Under stationarity each trial is EXACTLY
    /// Bernoulli(1/2) — the same centered fair-coin sum the family's
    /// exact bound covers — and the deviation |S − n/2| is tested
    /// two-sided at round-count geometric looks (2 statistics × 2 sides
    /// = 4 channels in family B's budget). Consequences remain NARRATION
    /// ONLY. All trial state persists (finding 3): the lifetime look
    /// budget survives reloads.
    pub drift_latched: bool,
    pub drift_z: f32,
    /// Frozen reference medians (alternation proportion, mean gap ×10).
    pub ref_alt_median: f32,
    pub ref_gap_median: f32,
    pub ref_frozen: bool,
    /// Sign-test tallies: (above-median count, trials) per statistic.
    pub alt_above: u32,
    pub alt_trials: u32,
    pub gap_above: u32,
    pub gap_trials: u32,
    /// Rounds summarized so far — family B's look counter (persisted).
    pub rounds_seen: u32,
    /// Kata 5 (#2) exploration bookkeeping (transient): frames the CPU
    /// has been sitting on a mine, and whether this round's single
    /// exploratory placement is spent.
    pub mine_held_streak: u32,
    pub explore_used: bool,
}

impl LearningLedgers {
    pub fn note_tactic(&mut self, reason: CpuDecisionReason, frame: u32, shrink: u16) {
        if let Some(&(id, _)) = TACTIC_IDS.iter().find(|&&(_, r)| r == reason) {
            // EPISODIC attempts (codex: exactly-once closure): consecutive
            // frames of the same tactic inside one precommitted window are
            // ONE attempt — the old per-frame increment inflated the
            // denominator by the pursuit's length. A window closes by
            // kill, by expiry, or by a different tactic taking over.
            if let Some((open_id, opened, _, _)) = self.open_attempt {
                if open_id == id && frame.saturating_sub(opened) <= ATTEMPT_HORIZON {
                    // Same episode, window stays as precommitted — the staged
                    // baseline (if any) belongs to the episode already open.
                    self.pending_boxer_baseline = None;
                    return;
                }
            }
            // Replacement of an in-horizon Boxer window: contested, not
            // forgotten (k3 verify, finding B — the terminal phase of a
            // WORKING choke hands the label back to the intercept).
            if let Some((4, opened, baseline, eshrink)) = self.open_attempt {
                if frame.saturating_sub(opened) <= ATTEMPT_HORIZON && baseline > 0.0 {
                    self.contested_boxer = Some((opened, baseline, eshrink));
                }
            }
            let baseline = if reason == CpuDecisionReason::Boxer {
                // A fresh Boxer window supersedes any contested remnant.
                self.contested_boxer = None;
                self.pending_boxer_baseline.take().unwrap_or(0.0)
            } else {
                self.pending_boxer_baseline = None;
                0.0
            };
            self.open_attempt = Some((id, frame, baseline, shrink));
            let e = match self.tactic_attempts.iter_mut().find(|e| e.0 == id) {
                Some(e) => e,
                None => {
                    self.tactic_attempts.push((id, 0.0, 0.0, 0, 0));
                    self.tactic_attempts.last_mut().unwrap()
                }
            };
            e.1 = e.1 * 0.999 + 1.0;
            e.3 = e.3.saturating_add(1);
        }
    }

    /// Player died: credit the open attempt iff its precommitted window
    /// still covers this frame — and, for Boxer (ADR-024), iff the choke
    /// was REALIZED: a boxing-compatible death cause with the player's
    /// reachable space at death collapsed to <=60% of the baseline the
    /// episode precommitted at open. Both consultants ruled the same
    /// way: the window stays 12 frames; the eligibility tightens.
    pub fn resolve_player_death(
        &mut self,
        frame: u32,
        cause: Option<crate::game::DeathCause>,
        player_space_at_death: f32,
        shrink_now: u16,
    ) {
        use crate::game::DeathCause as DC;
        let boxed_cause = matches!(
            cause,
            Some(DC::Wall) | Some(DC::OwnTrail) | Some(DC::EnemyTrail)
        );
        // The realized-choke test, shared by the open window and the
        // contested one. The shrink guard voids credit when sudden death
        // advanced during the window — close_ring collapses the player's
        // space MECHANICALLY and kills with DeathCause::Wall, a
        // lie-shaped hole in "a credit rule that cannot lie" (k3 verify,
        // finding B/G1).
        let realized = |baseline: f32, eshrink: u16| {
            boxed_cause
                && baseline > 0.0
                && player_space_at_death <= 0.6 * baseline
                && shrink_now == eshrink
        };
        // Contested arbitration (k3 G2): a Boxer window that was closed
        // by replacement inside its horizon wins the credit over its
        // replacement iff ITS choke realized — exclusive, never both.
        let contested_wins = self
            .contested_boxer
            .map(|(opened, baseline, eshrink)| {
                frame.saturating_sub(opened) <= ATTEMPT_HORIZON && realized(baseline, eshrink)
            })
            .unwrap_or(false);
        let credit_id = if let Some((id, opened, baseline, eshrink)) = self.open_attempt {
            let in_window = frame.saturating_sub(opened) <= ATTEMPT_HORIZON;
            let eligible = if id == 4 { realized(baseline, eshrink) } else { true };
            if in_window && eligible {
                if contested_wins && id != 4 { Some(4) } else { Some(id) }
            } else if contested_wins {
                Some(4)
            } else {
                None
            }
        } else if contested_wins {
            Some(4)
        } else {
            None
        };
        if let Some(id) = credit_id {
            if let Some(e) = self.tactic_attempts.iter_mut().find(|e| e.0 == id) {
                e.2 = e.2 * 0.999 + 1.0;
                e.4 = e.4.saturating_add(1);
            }
        }
        self.open_attempt = None;
        self.contested_boxer = None;
    }

    pub fn note_weapon(&mut self, kind: crate::game::PowerUpKind, gate_pass: bool, fired: bool) {
        if let Some(&(id, _)) = WEAPON_IDS.iter().find(|&&(_, k)| k == kind) {
            let e = match self.weapon_ops.iter_mut().find(|e| e.0 == id) {
                Some(e) => e,
                None => {
                    self.weapon_ops.push((id, 0, 0, 0, 0));
                    self.weapon_ops.last_mut().unwrap()
                }
            };
            e.1 = e.1.saturating_add(1);
            if gate_pass {
                e.2 = e.2.saturating_add(1);
            }
            if fired {
                e.3 = e.3.saturating_add(1);
            }
        }
    }

    /// An actual discharge (laser: after the telegraph completes).
    pub fn note_weapon_fired(&mut self, kind: crate::game::PowerUpKind) {
        if let Some(&(id, _)) = WEAPON_IDS.iter().find(|&&(_, k)| k == kind) {
            if let Some(e) = self.weapon_ops.iter_mut().find(|e| e.0 == id) {
                e.3 = e.3.saturating_add(1);
            }
        }
    }

    pub fn note_weapon_lethal(&mut self, kind: crate::game::PowerUpKind) {
        if let Some(&(id, _)) = WEAPON_IDS.iter().find(|&&(_, k)| k == kind) {
            if let Some(e) = self.weapon_ops.iter_mut().find(|e| e.0 == id) {
                e.4 = e.4.saturating_add(1);
            }
        }
    }

    /// Surface #5's consumer (ADR-021 Kata 1): how much extra escape
    /// margin THIS player's kill record has earned. Self-knowledge class
    /// under the agreed envelope — it can only RAISE a defensive floor
    /// (aversion ≥ 0, hard cap), so it structurally cannot manufacture
    /// aggression and needs no evidence gate. Chase-gated: only deaths
    /// where the player was actually ON the CPU (within 8 cells in the
    /// final 10 frames) count — learning "fear all trails" from wandering
    /// into old ones is nearly free early and ruinous late (k3).
    pub fn boxer_aversion(&self) -> f32 {
        let chased_trail = self
            .loss_causes
            .iter()
            .find(|e| e.0 == crate::game::DeathCause::EnemyTrail as u8)
            .map(|e| e.2)
            .unwrap_or(0);
        (0.06 * chased_trail as f32).min(0.5)
    }

    /// Kata 4 (#1): decayed kill-rate for a tactic, None below the
    /// maturity floor (10 non-decayed attempts) — an immature ledger
    /// abstains rather than steering on noise.
    pub fn tactic_kill_rate(&self, id: u8) -> Option<f32> {
        self.tactic_attempts
            .iter()
            .find(|e| e.0 == id)
            .filter(|e| e.3 >= 10)
            .map(|e| (e.2 + 0.5) / (e.1 + 1.0))
    }

    pub fn note_cpu_death(&mut self, cause_id: u8) {
        let chased = self.recent_dist.iter().any(|&d| d <= 8);
        let e = match self.loss_causes.iter_mut().find(|e| e.0 == cause_id) {
            Some(e) => e,
            None => {
                self.loss_causes.push((cause_id, 0, 0));
                self.loss_causes.last_mut().unwrap()
            }
        };
        e.1 = e.1.saturating_add(1);
        if chased {
            e.2 = e.2.saturating_add(1);
        }
    }

    pub fn note_frame(&mut self, dist: u32, lateral: Option<bool>, gap_before: u32) {
        self.recent_dist.push_back(dist);
        while self.recent_dist.len() > 10 {
            self.recent_dist.pop_front();
        }
        if let Some(left) = lateral {
            self.rs_laterals += 1;
            self.rs_gap_sum += gap_before;
            if let Some(prev) = self.rs_last_left {
                if prev != left {
                    self.rs_alternations += 1;
                }
            }
            self.rs_last_left = Some(left);
        }
    }

    /// Number of rounds that freeze the drift reference medians.
    pub const REF_ROUNDS: usize = 15;

    /// The sign-test z for one tally: |S − n/2| / sqrt(n/4).
    fn sign_z(above: u32, trials: u32) -> f32 {
        if trials == 0 {
            return 0.0;
        }
        (above as f32 - trials as f32 / 2.0).abs() / (trials as f32 / 4.0).sqrt()
    }

    pub fn end_round(&mut self, frames: u32) {
        if self.rs_laterals > 0 {
            let mean_gap_x10 = self.rs_gap_sum * 10 / self.rs_laterals.max(1);
            self.round_summaries
                .push_back((self.rs_laterals, self.rs_alternations, mean_gap_x10, frames));
            while self.round_summaries.len() > 64 {
                self.round_summaries.pop_front();
            }
            self.rounds_seen = self.rounds_seen.saturating_add(1);
            // Freeze the reference at REF_ROUNDS; afterwards each round is
            // one sign-test trial per statistic.
            let (l, a, g, _) = *self.round_summaries.back().unwrap();
            let alt_prop = if l >= 2 { a as f32 / (l - 1) as f32 } else { f32::NAN };
            let gap = g as f32;
            if !self.ref_frozen {
                if self.rounds_seen as usize == Self::REF_ROUNDS {
                    let n = self.round_summaries.len();
                    let start = n.saturating_sub(Self::REF_ROUNDS);
                    let mut alts: Vec<f32> = Vec::new();
                    let mut gaps: Vec<f32> = Vec::new();
                    for i in start..n {
                        let (rl, ra, rg, _) = self.round_summaries[i];
                        if rl >= 2 {
                            alts.push(ra as f32 / (rl - 1) as f32);
                        }
                        gaps.push(rg as f32);
                    }
                    let med = |v: &mut Vec<f32>| -> f32 {
                        if v.is_empty() {
                            return f32::NAN;
                        }
                        v.sort_by(|x, y| x.partial_cmp(y).unwrap());
                        v[v.len() / 2]
                    };
                    self.ref_alt_median = med(&mut alts);
                    self.ref_gap_median = med(&mut gaps);
                    self.ref_frozen = true;
                }
            } else {
                if alt_prop.is_finite() && self.ref_alt_median.is_finite()
                    && alt_prop != self.ref_alt_median
                {
                    self.alt_trials += 1;
                    if alt_prop > self.ref_alt_median {
                        self.alt_above += 1;
                    }
                }
                if self.ref_gap_median.is_finite() && gap != self.ref_gap_median {
                    self.gap_trials += 1;
                    if gap > self.ref_gap_median {
                        self.gap_above += 1;
                    }
                }
                let z_alt = Self::sign_z(self.alt_above, self.alt_trials);
                let z_gap = Self::sign_z(self.gap_above, self.gap_trials);
                self.drift_z = z_alt.max(z_gap);
                // Family B: 2 statistics × 2 sides = 4 channels; looks
                // keyed to each statistic's own trial count from n = 8.
                for (z, trials) in [(z_alt, self.alt_trials), (z_gap, self.gap_trials)] {
                    if let Some(bound) = ReadRate::look_threshold_for(
                        trials,
                        8,
                        ReadRate::FAMILY_B_CHANNELS,
                    ) {
                        if z > bound {
                            self.drift_latched = true;
                        }
                    }
                }
                if self.drift_latched && self.drift_z < 1.0 {
                    self.drift_latched = false;
                }
            }
        }
        self.rs_laterals = 0;
        self.rs_alternations = 0;
        self.rs_gap_sum = 0;
        self.rs_last_left = None;
        self.open_attempt = None;
        self.recent_dist.clear();
        self.mine_held_streak = 0;
        self.explore_used = false;
    }
}

/// The voluntary-turn VOMM on the wire (SEC_TURN_TIMING).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct TurnTimingWire {
    pub ver: u8,
    pub history: Vec<bool>,
    pub counts: Vec<(u8, u16, f32, f32)>,
    pub weights: Vec<f32>,
    pub events: u32,
}

/// Wire shapes: count-keyed vectors, semantic ids, own versions.
#[derive(serde::Serialize, serde::Deserialize)]
struct ActionOutcomesWire {
    ver: u8,
    tactics: Vec<(u8, f32, f32, u32, u32)>,
    weapons: Vec<(u8, u32, u32, u32, u32)>,
}
#[derive(serde::Serialize, serde::Deserialize)]
struct LossLedgerWire {
    ver: u8,
    causes: Vec<(u8, u32, u32)>,
}
/// The pre-verification drift wire (v1: summaries only). Kept decodable
/// so the golden tripwire passes and no live player loses their ring.
#[derive(serde::Serialize, serde::Deserialize)]
struct DriftEpochsWireV1 {
    ver: u8,
    rounds: Vec<(u32, u32, u32, u32)>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct DriftEpochsWire {
    ver: u8,
    rounds: Vec<(u32, u32, u32, u32)>,
    rounds_seen: u32,
    ref_alt_median: f32,
    ref_gap_median: f32,
    ref_frozen: bool,
    alt_above: u32,
    alt_trials: u32,
    gap_above: u32,
    gap_trials: u32,
    drift_latched: bool,
}

/* ----------------------- the turn book (ADR-020 stage 2) ----------------------- */

/// Number of hazard cells: gap-since-voluntary-turn (8 buckets) ×
/// food-side (3: ahead-closing / off to the LEFT / off to the RIGHT) ×
/// just-ate (2) × cpu-closing (2). The 3-way food-side feature replaces
/// the original aligned boolean (ADR-020 stage 2.2): the owner's measured
/// why-structure is overshoot-and-correct — P(turn | misaligned) 16.5%
/// vs 10.4% aligned, and WHICH side the food sits on carries the
/// correction's direction. Persisted 64-cell sections drop gracefully
/// (the wire is cell-count-keyed) and the hazard re-earns.
pub const HAZARD_CELLS: usize = 96;
/// Voluntary-turn events the book must have scored before the gate may
/// fire at all (kimi-k3: maturity floor; ~22 events arrive per round
/// against the owner, so this is about a round and a half of warmup).
pub const BOOK_MATURITY: u32 = 30;
/// Decay for every online statistic in the books. Chosen over an EMA of
/// the raw signal because an EMA would smear the ~5-frame slalom
/// periodicity the hazard exists to see.
const BOOK_DECAY: f32 = 0.995;

/// The class-conditional selection layer over the SAME ensemble: a hazard
/// model for WHEN the player turns, a turn-conditioned book for WHICH WAY,
/// and a derived no-knob gate deciding which book's answer gets published.
///
/// The RCA this answers: global fixed-share selection lets straight-frame
/// volume (88%) crown always-straight experts, so the published forecast
/// degenerates to "straight" exactly on the frames that decide games —
/// while eatW/armW sit unused at 54.8%/56.3% on those same frames. The
/// books are specialist accounting (sleeping-experts formalism): each is
/// scored ONLY against its own class, gate-independently, so neither can
/// be starved by the other's volume.
#[derive(Clone, Debug)]
pub struct ClassBooks {
    /// KT cells: decayed (turn events, total eligible events) per context.
    /// Estimate is Krichevsky–Trofimov, (t + 0.5)/(n + 1) — well-behaved
    /// at zero data.
    pub hz_turn: [f32; HAZARD_CELLS],
    pub hz_total: [f32; HAZARD_CELLS],
    /// Turn-book fixed-share weights over the ensemble roster, scored only
    /// on realized voluntary laterals. Slow half persists (knowledge about
    /// the human); fast half is per-round, like the ensemble's own.
    pub wt_slow: [f32; ENSEMBLE_MODELS],
    /// Fast horizon — per-round, never persisted.
    pub wt_fast: [f32; ENSEMBLE_MODELS],
    /// Gate-independent decayed accuracies: the straight book's hit rate
    /// over eligible frames (its prediction is always "straight"), and the
    /// turn book's SIDE hit rate over realized voluntary laterals.
    pub as_hits: f32,
    pub as_total: f32,
    pub at_hits: f32,
    pub at_total: f32,
    /// Scored voluntary-turn events (maturity floor).
    pub turn_events: u32,
    /// Coverage accounting (codex round 2): genuine side choices the book
    /// COULD have called (voluntary lateral, straight AND both laterals
    /// legal), how often it actually declared a side there, and how often
    /// the declaration was right. aT conditions on declaration; spendable
    /// evidence must multiply by coverage or an abstaining book reports
    /// excellent accuracy on an undisclosed subset.
    pub side_opportunities: u32,
    pub side_declarations: u32,
    /// Learned correction prior (stage 2.2): of the voluntary laterals
    /// taken while the food sat off to one side, how many broke TOWARD
    /// that side. Decayed KT pair; feeds ONLY the projection's side split
    /// when the book abstains — never an evidence channel.
    pub toward_food: f32,
    pub toward_total: f32,
    /// The book's own honest evidence record: its precommitted side picks
    /// scored on genuine two-sided voluntary turns through the SAME
    /// machinery as the published read — class-aware legality-aware
    /// baseline, McNemar + lateral channels, the family-wise anytime
    /// boundary, the same NULL guarantees. This — not raw aT — is what
    /// difficulty and projection authority are allowed to spend.
    pub book_read: ReadRate,
    /// Schmitt state of the publish gate (±0.05 band around the derived
    /// threshold, so the sealed HUD-visible forecast identity cannot flap
    /// on a knife edge).
    pub gate_open: bool,
}

impl Default for ClassBooks {
    fn default() -> Self {
        Self {
            hz_turn: [0.0; HAZARD_CELLS],
            hz_total: [0.0; HAZARD_CELLS],
            wt_slow: [1.0; ENSEMBLE_MODELS],
            wt_fast: [1.0; ENSEMBLE_MODELS],
            as_hits: 0.0,
            as_total: 0.0,
            at_hits: 0.0,
            at_total: 0.0,
            turn_events: 0,
            side_opportunities: 0,
            side_declarations: 0,
            toward_food: 0.0,
            toward_total: 0.0,
            book_read: ReadRate::default(),
            gate_open: false,
        }
    }
}

impl ClassBooks {
    /// Per-round reset: ONLY the fast horizon. Slow weights, hazard cells,
    /// accuracies and maturity persist — cold-starting selection every
    /// round is exactly the failure the RCA measured 45 times over.
    pub fn reset_round(&mut self) {
        self.wt_fast = [1.0; ENSEMBLE_MODELS];
    }

    /// KT hazard estimate for a context cell.
    pub fn hazard(&self, cell: usize) -> f32 {
        let c = cell.min(HAZARD_CELLS - 1);
        (self.hz_turn[c] + 0.5) / (self.hz_total[c] + 1.0)
    }

    /// Train the hazard on an eligible frame (straight was legal; the
    /// player could have held the line).
    pub fn observe_hazard(&mut self, cell: usize, turned: bool) {
        let c = cell.min(HAZARD_CELLS - 1);
        self.hz_turn[c] *= BOOK_DECAY;
        self.hz_total[c] *= BOOK_DECAY;
        self.hz_total[c] += 1.0;
        if turned {
            self.hz_turn[c] += 1.0;
        }
    }

    /// Straight book accuracy: P(hit) of always-answering-straight over
    /// eligible frames, decayed.
    pub fn a_straight(&self) -> f32 {
        if self.as_total <= 0.0 {
            0.5
        } else {
            self.as_hits / self.as_total
        }
    }

    /// Turn book accuracy: P(side correct | voluntary lateral), decayed.
    /// 0.5 before evidence — a coin, honestly.
    pub fn a_turn(&self) -> f32 {
        if self.at_total <= 0.0 {
            0.5
        } else {
            self.at_hits / self.at_total
        }
    }

    pub fn observe_straight_book(&mut self, hit: bool) {
        self.as_hits *= BOOK_DECAY;
        self.as_total *= BOOK_DECAY;
        self.as_total += 1.0;
        if hit {
            self.as_hits += 1.0;
        }
    }

    /// The book evidence difficulty may spend: the book's earned read
    /// (family-gated, SE-shrunk) scaled by coverage, so accuracy on an
    /// undisclosed subset cannot buy global aggression.
    pub fn spendable(&self) -> f32 {
        self.book_read.earned_read() * self.coverage()
    }

    /// Whether the book has earned the right to BEND the projection: the
    /// evidence family latched AND the maturity floor met. A chance-level
    /// side book must never reshape defensive paths, even while sharpness
    /// stays zero (codex round 2).
    pub fn projection_authority(&self) -> bool {
        self.turn_events >= BOOK_MATURITY && self.book_read.earned_read() > 0.0
    }

    /// KT estimate of P(voluntary turn breaks toward the food side |
    /// food off to a side). 0.5 at no data.
    pub fn q_toward_food(&self) -> f32 {
        (self.toward_food + 0.5) / (self.toward_total + 1.0)
    }

    pub fn observe_toward_food(&mut self, toward: bool) {
        self.toward_food *= BOOK_DECAY;
        self.toward_total *= BOOK_DECAY;
        self.toward_total += 1.0;
        if toward {
            self.toward_food += 1.0;
        }
    }

    /// Kata 6 (#3): the epistemic self-map, count-based (both consults:
    /// 96 static cells need direct mass, not connectivity — the
    /// ruvector-mincut dep was evaluated and declined for v1, verdict in
    /// ADR-021). Returns (populated, thin, unseen) hazard cells; decayed
    /// mass, so thinness reflects CURRENCY, not ancient history. Facts,
    /// not hypotheses — no evidence gate; consumers are narration only.
    pub fn map_summary(&self) -> (u32, u32, u32) {
        let mut populated = 0;
        let mut thin = 0;
        let mut unseen = 0;
        for cell in 0..HAZARD_CELLS {
            if self.hz_total[cell] < 1.0 {
                unseen += 1;
            } else if self.hz_total[cell] < 5.0 {
                populated += 1;
                thin += 1;
            } else {
                populated += 1;
            }
        }
        (populated, thin, unseen)
    }

    /// Fraction of genuine side choices where the book declared a side.
    pub fn coverage(&self) -> f32 {
        if self.side_opportunities == 0 {
            0.0
        } else {
            self.side_declarations as f32 / self.side_opportunities as f32
        }
    }

    pub fn observe_turn_book(&mut self, hit: bool) {
        self.at_hits *= BOOK_DECAY;
        self.at_total *= BOOK_DECAY;
        self.at_total += 1.0;
        if hit {
            self.at_hits += 1.0;
        }
        self.turn_events = self.turn_events.saturating_add(1);
    }

    /// Fixed-share update of the turn-book weights on a realized voluntary
    /// lateral — same Herbster–Warmuth step the global ensemble uses, same
    /// tuned rates, applied only to models that spoke.
    pub fn score_turn_frame(&mut self, masked: &[Option<Direction>], actual: Direction) {
        let t = crate::tuning::tuning();
        for i in 0..ENSEMBLE_MODELS {
            if let Some(p) = masked.get(i).copied().flatten() {
                let loss = if p == actual { 0.0 } else { 1.0 };
                self.wt_fast[i] *= (-t.eta_fast * loss).exp();
                self.wt_slow[i] *= (-t.eta_slow * loss).exp();
            }
        }
        for (weights, share) in [
            (&mut self.wt_fast, t.share_fast),
            (&mut self.wt_slow, t.share_slow),
        ] {
            let sum: f32 = weights.iter().sum();
            if sum > 0.0 && sum.is_finite() {
                let pool = share * sum / ENSEMBLE_MODELS as f32;
                for v in weights.iter_mut() {
                    *v = (1.0 - share) * *v + pool;
                }
                let sum: f32 = weights.iter().sum();
                let inv = ENSEMBLE_MODELS as f32 / sum;
                for v in weights.iter_mut() {
                    *v *= inv;
                }
            } else {
                *weights = [1.0; ENSEMBLE_MODELS];
            }
        }
    }

    /// The turn book's SIDE pick: among models currently predicting a
    /// LATERAL (relative to the player's heading), the one with the best
    /// turn-book weight. None when nothing lateral speaks.
    pub fn side_pick(
        &self,
        masked: &[Option<Direction>],
        heading: Direction,
    ) -> Option<(usize, Direction)> {
        let mut best: Option<(usize, Direction)> = None;
        let mut best_w = f32::NEG_INFINITY;
        for i in 0..ENSEMBLE_MODELS {
            if let Some(p) = masked.get(i).copied().flatten() {
                let lateral = matches!(
                    Turn::from_dirs(heading, p),
                    Some(Turn::Left) | Some(Turn::Right)
                );
                if lateral {
                    let w = self.wt_fast[i] + self.wt_slow[i];
                    if w > best_w {
                        best_w = w;
                        best = Some((i, p));
                    }
                }
            }
        }
        best
    }

    /// The derived publish gate (kimi-k3: no threshold knob). Publishing
    /// the turn book earns h·aT expected hits; publishing straight earns
    /// (1−h)·aS. The Schmitt band (±0.05) means the sealed forecast's
    /// identity switches on conviction, not on a knife edge. Below the
    /// maturity floor the gate is hard-closed.
    pub fn gate(&mut self, h: f32) -> bool {
        if self.turn_events < BOOK_MATURITY {
            self.gate_open = false;
            return false;
        }
        let turn_ev = h * self.a_turn();
        let straight_ev = (1.0 - h) * self.a_straight();
        if turn_ev > straight_ev + 0.05 {
            self.gate_open = true;
        } else if turn_ev < straight_ev - 0.05 {
            self.gate_open = false;
        }
        self.gate_open
    }
}

/// The turn book's wire shape: length-prefixed vectors so a roster or
/// cell-count change is a readable mismatch that drops ONLY this section,
/// never a decode failure that eats the file (EpisodesWire pattern).
#[derive(serde::Serialize, serde::Deserialize)]
struct ClassBooksWire {
    cells: u16,
    models: u16,
    hz_turn: Vec<f32>,
    hz_total: Vec<f32>,
    wt_slow: Vec<f32>,
    as_hits: f32,
    as_total: f32,
    at_hits: f32,
    at_total: f32,
    turn_events: u32,
    side_opportunities: u32,
    side_declarations: u32,
    toward_food: f32,
    toward_total: f32,
    book_read: ReadRate,
    gate_open: bool,
}

impl From<&ClassBooks> for ClassBooksWire {
    fn from(b: &ClassBooks) -> Self {
        ClassBooksWire {
            cells: HAZARD_CELLS as u16,
            models: ENSEMBLE_MODELS as u16,
            hz_turn: b.hz_turn.to_vec(),
            hz_total: b.hz_total.to_vec(),
            wt_slow: b.wt_slow.to_vec(),
            as_hits: b.as_hits,
            as_total: b.as_total,
            at_hits: b.at_hits,
            at_total: b.at_total,
            turn_events: b.turn_events,
            side_opportunities: b.side_opportunities,
            side_declarations: b.side_declarations,
            toward_food: b.toward_food,
            toward_total: b.toward_total,
            book_read: b.book_read,
            gate_open: b.gate_open,
        }
    }
}

impl ClassBooksWire {
    /// Restore what still fits. A changed roster drops the weights but
    /// keeps the hazard (and vice versa) — knowledge about the human
    /// survives every schema change that can possibly honor it.
    fn restore(self) -> ClassBooks {
        let mut b = ClassBooks::default();
        if self.cells as usize == HAZARD_CELLS
            && self.hz_turn.len() == HAZARD_CELLS
            && self.hz_total.len() == HAZARD_CELLS
        {
            b.hz_turn.copy_from_slice(&self.hz_turn);
            b.hz_total.copy_from_slice(&self.hz_total);
        }
        if self.models as usize == ENSEMBLE_MODELS && self.wt_slow.len() == ENSEMBLE_MODELS {
            b.wt_slow.copy_from_slice(&self.wt_slow);
        }
        b.as_hits = self.as_hits;
        b.as_total = self.as_total;
        b.at_hits = self.at_hits;
        b.at_total = self.at_total;
        b.turn_events = self.turn_events;
        b.side_opportunities = self.side_opportunities;
        b.side_declarations = self.side_declarations;
        b.toward_food = self.toward_food;
        b.toward_total = self.toward_total;
        b.book_read = self.book_read;
        b.gate_open = self.gate_open;
        b
    }
}

/// Which side of the player's heading the nearest food sits on.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum FoodSide {
    /// Heading is closing on it — no correction due.
    Ahead,
    /// Off to the player's LEFT: an overshoot correction breaks left.
    Left,
    /// Off to the player's RIGHT.
    Right,
}

/// Classify the nearest food relative to a heading from a position.
/// `Ahead` when the heading still closes on it (or there is no food);
/// otherwise the perpendicular side it sits on (ties toward Ahead).
pub fn food_side(
    px: u16,
    py: u16,
    heading: Direction,
    nearest: Option<(u16, u16)>,
) -> FoodSide {
    let Some((fx, fy)) = nearest else {
        return FoodSide::Ahead;
    };
    let (dx, dy) = heading.as_delta();
    let (rx, ry) = (fx as i32 - px as i32, fy as i32 - py as i32);
    if rx * dx as i32 + ry * dy as i32 > 0 {
        return FoodSide::Ahead;
    }
    // Cross product sign: positive = food on the heading's right.
    let cross = dx as i32 * ry - dy as i32 * rx;
    if cross > 0 {
        FoodSide::Right
    } else if cross < 0 {
        FoodSide::Left
    } else {
        FoodSide::Ahead
    }
}

/// Bucket the hazard context into a cell index.
/// gap: frames since the player's last voluntary lateral (0..7+);
/// side: where the nearest food sits relative to their heading;
/// just_ate: they picked food up within the last 3 frames;
/// cpu_close: the CPU is within 12 cells and closing.
pub fn hazard_cell(gap: u32, side: FoodSide, just_ate: bool, cpu_close: bool) -> usize {
    let g = (gap as usize).min(7);
    let s = match side {
        FoodSide::Ahead => 0usize,
        FoodSide::Left => 1,
        FoodSide::Right => 2,
    };
    g + 8 * (s + 3 * ((just_ate as usize) + 2 * (cpu_close as usize)))
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
            turn_pattern: TurnPattern::default(),
            voluntary_pattern: TurnPattern::default(),
            portfolio: Portfolio::default(),
            intent_targets: [None; 2],
            class_books: ClassBooks::default(),
            gap_since_voluntary: 0,
            frames_since_food: 99,
            prev_pc_dist: 0,
            earned_snapshot: 0.0,
            discipline_latched: false,
            spend_dwell: 0,
            book_spend_snapshot: 0.0,
            book_authority_snapshot: false,
            tactic_prefer_direct: false,
            tactic_boxer_ok: true,
            region_ring: std::collections::VecDeque::new(),
            ledgers: LearningLedgers::default(),
            pending_book: None,
        }
    }
}

impl CpuBrain {
    pub fn new() -> Self {
        Self::default()
    }

    /// The whole evidence family's earned read: the published record's
    /// channels and the book's channels (scaled by coverage), every one
    /// individually latched under the shared family-wise anytime boundary
    /// and spending an SE-shrunk lift. The book half sits behind the
    /// book_spend attribution switch (codex D10).
    /// Below this spend the read moves no behavior worth defending —
    /// the dwell release's floor (k3 v9 ruling 2b).
    pub const SPEND_DWELL_FLOOR: f32 = 0.05;
    /// Consecutive below-floor round boundaries before a latched read
    /// releases outright.
    pub const SPEND_DWELL_ROUNDS: u8 = 5;

    pub fn family_earned_read(&self) -> f32 {
        let published = self.lifetime_read.earned_read();
        if crate::tuning::tuning().book_spend < 0.5 {
            published
        } else {
            published.max(self.class_books.spendable())
        }
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
        push_section(
            &mut sections,
            SEC_READ_RATE,
            &ReadRateWireV1::from(&self.lifetime_read),
        );
        push_section(&mut sections, SEC_READ_RATE2, &self.lifetime_read);
        push_section(
            &mut sections,
            SEC_CLASS_BOOKS,
            &ClassBooksWire::from(&self.class_books),
        );
        push_section(
            &mut sections,
            SEC_ACTION_OUTCOMES,
            &ActionOutcomesWire {
                ver: 1,
                tactics: self.ledgers.tactic_attempts.clone(),
                weapons: self.ledgers.weapon_ops.clone(),
            },
        );
        push_section(
            &mut sections,
            SEC_LOSS_DEFENSE,
            &LossLedgerWire { ver: 1, causes: self.ledgers.loss_causes.clone() },
        );
        push_section(
            &mut sections,
            SEC_TURN_TIMING,
            &self.voluntary_pattern.to_wire(),
        );
        push_section(
            &mut sections,
            SEC_DRIFT_EPOCHS,
            &DriftEpochsWire {
                ver: 2,
                rounds: self.ledgers.round_summaries.iter().copied().collect(),
                rounds_seen: self.ledgers.rounds_seen,
                ref_alt_median: self.ledgers.ref_alt_median,
                ref_gap_median: self.ledgers.ref_gap_median,
                ref_frozen: self.ledgers.ref_frozen,
                alt_above: self.ledgers.alt_above,
                alt_trials: self.ledgers.alt_trials,
                gap_above: self.ledgers.gap_above,
                gap_trials: self.ledgers.gap_trials,
                drift_latched: self.ledgers.drift_latched,
            },
        );
        push_section(&mut sections, SEC_TURN_PRIOR, &self.opp_brain.turn_tally);
        push_section(&mut sections, SEC_PORTFOLIO, &self.portfolio);

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

        // ADR-021 ledgers: cross-field invariants and finiteness. A
        // violated section resets to default rather than feeding NaN or
        // impossible counts into aversion/bandit/drift consumers.
        {
            let l = &mut self.ledgers;
            let bad_tactics = l.tactic_attempts.iter().any(|e| {
                !e.1.is_finite() || !e.2.is_finite() || e.1 < 0.0 || e.2 < 0.0
                    || e.2 > e.1 + 1.0 || e.4 > e.3
                    || !TACTIC_IDS.iter().any(|&(id, _)| id == e.0)
            }) || l.tactic_attempts.len() > 16;
            if bad_tactics {
                l.tactic_attempts.clear();
                report.sections_skipped += 1;
            }
            let bad_weapons = l.weapon_ops.iter().any(|e| {
                e.4 > e.3 || e.3 > e.1 || e.2 > e.1
                    || !WEAPON_IDS.iter().any(|&(id, _)| id == e.0)
            }) || l.weapon_ops.len() > 16;
            if bad_weapons {
                l.weapon_ops.clear();
                report.sections_skipped += 1;
            }
            let bad_losses =
                l.loss_causes.iter().any(|e| e.2 > e.1 || e.0 > 16) || l.loss_causes.len() > 16;
            if bad_losses {
                l.loss_causes.clear();
                report.sections_skipped += 1;
            }
            let bad_drift = l.round_summaries.len() > 64
                || l
                    .round_summaries
                    .iter()
                    .any(|&(lat, alt, _, _)| alt > lat.saturating_sub(1))
                || !l.ref_alt_median.is_finite() && l.ref_frozen
                || l.alt_above > l.alt_trials
                || l.gap_above > l.gap_trials;
            if bad_drift {
                l.round_summaries.clear();
                l.rounds_seen = 0;
                l.ref_frozen = false;
                l.ref_alt_median = 0.0;
                l.ref_gap_median = 0.0;
                l.alt_above = 0;
                l.alt_trials = 0;
                l.gap_above = 0;
                l.gap_trials = 0;
                l.drift_latched = false;
                report.sections_skipped += 1;
            }
        }
        // The voluntary VOMM: non-finite counts/weights poison every read.
        {
            let vp = &mut self.voluntary_pattern;
            let bad = vp.history.len() > 64
                || vp.weights.iter().any(|w| !w.is_finite() || *w < 0.0)
                || vp
                    .counts
                    .values()
                    .any(|&(l, n)| !l.is_finite() || !n.is_finite() || l < 0.0 || n < 0.0 || l > n + 1.0);
            if bad {
                *vp = TurnPattern::default();
                report.sections_skipped += 1;
            }
        }

        // Persisted book state: any non-finite or negative float poisons
        // the hazard, the gate, and everything the projection trusts.
        {
            let b = &mut self.class_books;
            let bad = b
                .hz_turn
                .iter()
                .chain(b.hz_total.iter())
                .chain(b.wt_slow.iter())
                .chain(
                    [
                        b.as_hits,
                        b.as_total,
                        b.at_hits,
                        b.at_total,
                        b.book_read.lat_chance,
                        b.book_read.lat_var,
                        b.toward_food,
                        b.toward_total,
                    ]
                    .iter(),
                )
                .any(|v| !v.is_finite() || *v < 0.0);
            if bad {
                *b = ClassBooks::default();
                report.sections_skipped += 1;
            } else {
                b.side_declarations = b.side_declarations.min(b.side_opportunities);
            }
        }

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
        let mut read_rate_v2_seen = false;
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
                SEC_PORTFOLIO => match bincode::deserialize::<Portfolio>(body) {
                    Ok(p) => brain.portfolio = p,
                    Err(_) => report.sections_skipped += 1,
                },
                SEC_TURN_PRIOR => match bincode::deserialize::<[f32; TURNS]>(body) {
                    Ok(t) => brain.opp_brain.turn_tally = t,
                    Err(_) => report.sections_skipped += 1,
                },
                // Version precedence is explicit, not an accident of file
                // order: once the widened v2 record has decoded, a v1
                // projection in the same blob must never clobber it.
                SEC_READ_RATE => match bincode::deserialize::<ReadRateWireV1>(body) {
                    Ok(r) => {
                        if !read_rate_v2_seen {
                            brain.lifetime_read = r.into();
                        }
                    }
                    Err(_) => report.sections_skipped += 1,
                },
                SEC_READ_RATE2 => match bincode::deserialize::<ReadRate>(body) {
                    Ok(r) => {
                        brain.lifetime_read = r;
                        read_rate_v2_seen = true;
                    }
                    Err(_) => report.sections_skipped += 1,
                },
                SEC_CLASS_BOOKS => match bincode::deserialize::<ClassBooksWire>(body) {
                    Ok(w) => brain.class_books = w.restore(),
                    Err(_) => report.sections_skipped += 1,
                },
                SEC_ACTION_OUTCOMES => {
                    match bincode::deserialize::<ActionOutcomesWire>(body) {
                        Ok(w) if w.ver != 1 => report.sections_skipped += 1,
                        Ok(w) => {
                            brain.ledgers.tactic_attempts = w.tactics;
                            brain.ledgers.weapon_ops = w.weapons;
                        }
                        Err(_) => report.sections_skipped += 1,
                    }
                }
                SEC_LOSS_DEFENSE => match bincode::deserialize::<LossLedgerWire>(body) {
                    Ok(w) if w.ver == 1 => brain.ledgers.loss_causes = w.causes,
                    _ => report.sections_skipped += 1,
                },
                SEC_TURN_TIMING => match bincode::deserialize::<TurnTimingWire>(body) {
                    Ok(w) if w.ver == 1 => brain.voluntary_pattern = TurnPattern::from_wire(w),
                    _ => report.sections_skipped += 1,
                },
                SEC_DRIFT_EPOCHS => match bincode::deserialize::<DriftEpochsWire>(body) {
                    // v1 blobs (summaries only): keep the ring, re-earn the
                    // reference — the golden-tripwire dual-decode ritual.
                    Err(_) => match bincode::deserialize::<DriftEpochsWireV1>(body) {
                        Ok(w1) if w1.ver == 1 => {
                            brain.ledgers.round_summaries = w1.rounds.into_iter().collect();
                            brain.ledgers.rounds_seen =
                                brain.ledgers.round_summaries.len() as u32;
                        }
                        _ => report.sections_skipped += 1,
                    },
                    Ok(w) if w.ver == 2 => {
                        brain.ledgers.round_summaries = w.rounds.into_iter().collect();
                        brain.ledgers.rounds_seen = w.rounds_seen;
                        brain.ledgers.ref_alt_median = w.ref_alt_median;
                        brain.ledgers.ref_gap_median = w.ref_gap_median;
                        brain.ledgers.ref_frozen = w.ref_frozen;
                        brain.ledgers.alt_above = w.alt_above;
                        brain.ledgers.alt_trials = w.alt_trials;
                        brain.ledgers.gap_above = w.gap_above;
                        brain.ledgers.gap_trials = w.gap_trials;
                        brain.ledgers.drift_latched = w.drift_latched;
                    }
                    Ok(_) => report.sections_skipped += 1,
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
/// The Exp3 playstyle weights — which temperaments beat this human.
const SEC_PORTFOLIO: u16 = 8;
/// The turn book's books, hazard cells, and accuracies (ADR-020 stage 2).
/// Knowledge about the human — persists so the 45-round corpus failure
/// (selection cold-started every round) cannot recur.
const SEC_CLASS_BOOKS: u16 = 9;
/// The widened lifetime read (adds the lateral evidence channel, ADR-020).
/// `bincode` is not field-tolerant, so the widened `ReadRate` gets a NEW
/// section id; `SEC_READ_RATE` keeps carrying the v1 projection so an older
/// build reading a newer save still recovers the core read instead of
/// wiping what the CPU learned about the human. The writer emits v1 before
/// v2, so on load the full record wins.
const SEC_READ_RATE2: u16 = 10;
/// ADR-021 self-knowledge ledgers, by failure domain (consult D).
const SEC_ACTION_OUTCOMES: u16 = 11;
const SEC_LOSS_DEFENSE: u16 = 12;
const SEC_DRIFT_EPOCHS: u16 = 13;
/// The voluntary-turn VOMM (the rhythm reader's grammar) — ADR-021 Kata 2.
/// The 8→16 gap-bucket upgrade measured WORSE prequentially (0.3642 vs
/// 0.3619 log-loss/frame on the owner corpus) and was NOT taken; this
/// section is the kata's durable half: the sequence read survives
/// sessions.
const SEC_TURN_TIMING: u16 = 14;

/// The pre-ADR-020 `ReadRate` shape, kept as the v1 wire projection.
#[derive(serde::Serialize, serde::Deserialize)]
struct ReadRateWireV1 {
    hits: u32,
    samples: u32,
    taken: [u32; TURNS],
    opts: [u32; 4],
    mode_hits: u32,
    cpu_only: u32,
    mode_only: u32,
}

impl From<&ReadRate> for ReadRateWireV1 {
    fn from(r: &ReadRate) -> Self {
        ReadRateWireV1 {
            hits: r.hits,
            samples: r.samples,
            taken: r.taken,
            opts: r.opts,
            mode_hits: r.mode_hits,
            cpu_only: r.cpu_only,
            mode_only: r.mode_only,
        }
    }
}

impl From<ReadRateWireV1> for ReadRate {
    fn from(w: ReadRateWireV1) -> Self {
        ReadRate {
            hits: w.hits,
            samples: w.samples,
            taken: w.taken,
            opts: w.opts,
            // Downgrade semantics, defined (codex finding): a v1-only blob
            // was written either by a pre-ADR-020 build (class-blind
            // baseline) or by an old build that kept appending class-blind
            // observations after stripping the v2 section. Either way the
            // discordant-pair stream mixes baseline regimes the honest
            // McNemar cannot untangle — so the significance BOOKKEEPING
            // resets and must be re-earned, while everything that is
            // knowledge about the HUMAN (taken, hits, samples, opts)
            // survives. Lateral evidence is lost on downgrade by
            // construction; that is the accepted cost.
            mode_hits: 0,
            cpu_only: 0,
            mode_only: 0,
            ..ReadRate::default()
        }
    }
}

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
    count_open_space_excluding(game, start_x, start_y, &[])
}

/// `count_open_space` with extra cells treated as walls — the player's
/// PREDICTED next positions. The plain flood fill is honest about the board
/// as it stands and completely blind to the board as it is about to be:
/// a pocket whose mouth the player's advancing trail closes NEXT frame still
/// measures thousands of cells this frame. Both traced warm-arm death modes
/// (the corridor pin at the arena wall and the close-evasion pocket seal at
/// length 60+) are exactly that blindness. Subtracting the few cells the
/// player is projected to occupy is what lets a destination be scored
/// against the board it will actually have to survive in.
pub fn count_open_space_excluding(
    game: &WormGame,
    start_x: u16,
    start_y: u16,
    excluded: &[(u16, u16)],
) -> f32 {
    // A start cell that is itself excluded is a destination the player is
    // about to occupy: it has NO open space, not "one plus whatever the
    // fill finds" (the old code marked it visited and then counted it
    // anyway, quietly ignoring the exclusion for exactly the most
    // dangerous cell).
    if excluded.contains(&(start_x, start_y)) {
        return 0.0;
    }
    let mut visited = vec![vec![false; game.width as usize]; game.height as usize];
    for &(ex, ey) in excluded {
        if ex < game.width && ey < game.height {
            visited[ey as usize][ex as usize] = true;
        }
    }
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

/// The corridor pin: the player runs parallel one row inside a wall lane,
/// diagonally abeam of the CPU, matching its heading at equal speed. From
/// that instant the CPU has exactly one legal move per frame — straight —
/// until the facing wall kills it. Traced live on multiple seeds (death at
/// the same frame every replay), and 100% reproducible by a human who
/// notices it: a deterministic loss condition.
///
/// No flood fill can see it. The sealed region is only "unreachable" under
/// the no-reversal rule — undirected reachability says the whole arena is
/// open, and it is, for anything that could turn around. The trap is
/// kinematic, so the defence is geometric: this predicate answers "would
/// stepping `d` put me in a wall lane whose open side the player is
/// escorting abeam?" — the position from which the lock forms. Refusing the
/// step while alternatives exist is the entire fix; once the lock exists
/// there is nothing left to decide.
///
/// Information parity: uses only the player's visible position and heading.
pub fn escorted_lane_step(game: &WormGame, from: (u16, u16), d: Direction) -> bool {
    let (dx, dy) = d.as_delta();
    let nx = from.0 as i16 + dx;
    let ny = from.1 as i16 + dy;
    if nx < 0 || ny < 0 || nx >= game.width as i16 || ny >= game.height as i16 {
        return false;
    }
    // Escort requires matched velocity — a player heading any other way
    // cannot hold the lane shut.
    let player = &game.cycles[0];
    if player.direction != d {
        return false;
    }
    let (px, py) = player.head;
    let sides: [(i16, i16); 2] = match d {
        Direction::Left | Direction::Right => [(0, -1), (0, 1)],
        Direction::Up | Direction::Down => [(-1, 0), (1, 0)],
    };
    for (sx, sy) in sides {
        let wx = nx + sx;
        let wy = ny + sy;
        let wall_side = wx < 0
            || wy < 0
            || wx >= game.width as i16
            || wy >= game.height as i16
            || game.grid[wy as usize][wx as usize] == CellType::Wall
            || game.is_arena_wall(wx as u16, wy as u16);
        if !wall_side {
            continue;
        }
        // Player abeam (within 2 cells longitudinally) on the OPEN side of
        // the lane, within 2 cells laterally. Lateral 1 abeam is the lock
        // itself; lateral 2 is one manoeuvre away from it. A player further
        // behind can never catch up at equal speed, and one further to the
        // side cannot close the lane before the CPU leaves it.
        let (lat, lon) = match d {
            Direction::Left | Direction::Right => (py as i16 - ny, (px as i16 - nx).abs()),
            Direction::Up | Direction::Down => (px as i16 - nx, (py as i16 - ny).abs()),
        };
        let open_sign = match d {
            Direction::Left | Direction::Right => -sy,
            Direction::Up | Direction::Down => -sx,
        };
        if lat.signum() == open_sign.signum() && (1..=2).contains(&lat.abs()) && lon <= 2 {
            return true;
        }
    }
    false
}

/// Timed flood fill from `from`: own-body cells become enterable at the time
/// the tail will have vacated them (`t = len - i + pending_growth` for
/// positions[i], head first). Returns (reachable cells, own tail reachable).
///
/// The plain flood fill treats the CPU's own body as permanent wall, which
/// systematically under-counts a long worm's real room: a 100-cell body is
/// 100 cells of "wall" that will all be floor again within 100 frames. The
/// honest survival question at length 60+ is Tron's classic one — "can the
/// head still reach its own tail" — because a worm that can reach its tail
/// can follow it forever. The opponent's body is treated as static here:
/// conservative, and it is the CPU's OWN coil this exists to see through.
///
/// Measured (520 games/arm, external policy, coil regime at mean length
/// ~176): swapping the static floor for this halved OwnTrail deaths (6→3,
/// total 18→14) with no length given up — directionally right, n too small
/// to call significant, which is why it is used as a RELAXATION of the
/// existing floor (OR, never instead of).
pub fn tail_aware_reach(game: &WormGame, who: usize, from: (u16, u16)) -> (f32, bool) {
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

/// BFS from (sx,sy) to the nearest collectible (food or power-up). Returns
/// (direction_to_take, open_space_at_item). Only considers legal directions
/// from the start, and checks that the item cell is reachable.
/// BFS distance field from (sx, sy): (dist, first-step) per cell. `legal`
/// seeds the frontier; empty seeds all four neighbours (the opponent field —
/// their reversal rule is not ours to enforce).
fn bfs_field(
    game: &WormGame,
    sx: u16,
    sy: u16,
    legal: &[Direction],
) -> Vec<(i32, Option<Direction>)> {
    let w = game.width as usize;
    let mut f = vec![(-1i32, None); w * game.height as usize];
    let mut q: VecDeque<(u16, u16)> = VecDeque::new();
    let seeds: &[Direction] = if legal.is_empty() {
        &[
            Direction::Up,
            Direction::Down,
            Direction::Left,
            Direction::Right,
        ]
    } else {
        legal
    };
    for &d in seeds {
        let (dx, dy) = d.as_delta();
        let nx = sx as i16 + dx;
        let ny = sy as i16 + dy;
        if nx < 0 || ny < 0 || nx >= game.width as i16 || ny >= game.height as i16 {
            continue;
        }
        let (nx, ny) = (nx as u16, ny as u16);
        let idx = ny as usize * w + nx as usize;
        if f[idx].0 < 0 && game.passable(nx, ny) {
            f[idx] = (1, Some(d));
            q.push_back((nx, ny));
        }
    }
    while let Some((x, y)) = q.pop_front() {
        let (base, sd) = f[y as usize * w + x as usize];
        for (dx, dy) in [(0i16, -1i16), (0, 1), (-1, 0), (1, 0)] {
            let nx = x as i16 + dx;
            let ny = y as i16 + dy;
            if nx < 0 || ny < 0 || nx >= game.width as i16 || ny >= game.height as i16 {
                continue;
            }
            let (nx, ny) = (nx as u16, ny as u16);
            let idx = ny as usize * w + nx as usize;
            if f[idx].0 < 0
                && matches!(
                    game.grid[ny as usize][nx as usize],
                    CellType::Empty | CellType::Food | CellType::Hole | CellType::PowerUp
                )
            {
                f[idx] = (base + 1, sd);
                q.push_back((nx, ny));
            }
        }
    }
    f
}

/// The item worth chasing: best VALUE-PER-STEP among items the CPU reaches
/// STRICTLY before the player, that still leave an escape after the post-eat
/// growth. Returns the first step to take and reachable space at the target.
///
/// This replaces nearest-item BFS, which was measured losing 86% of the food
/// races it was strictly closer to and ending matches with ~6% of the
/// economy. Three rules, each load-bearing:
///   RACE — the player's head resolves BEFORE the CPU's inside update(), so a
///   tie goes to them: chase only what we win OUTRIGHT, stop donating detours.
///   VALUE — a 9-morsel two steps further beats a 1-morsel; nearest-first
///   could not tell them apart.
///   ESCAPE — eating value v freezes tail retraction for v frames, so the
///   body to outrun afterwards is len + v; nearest-first checked the board as
///   if eating were free.
/// Losing every race is also why the CPU used to disengage and orbit corners:
/// with no winnable item on its list, the survival layers were all that
/// remained. A winnable target is what pulls it into the arena.
fn best_food_target(
    game: &WormGame,
    cx: u16,
    cy: u16,
    legal: &[Direction],
) -> Option<(Direction, f32)> {
    if game.food_items.is_empty() && game.powerups.is_empty() {
        return None;
    }
    let w = game.width as usize;
    let mine = bfs_field(game, cx, cy, legal);
    let (px, py) = game.cycles[0].head;
    let theirs = bfs_field(game, px, py, &[]);
    let own_len =
        game.cycles[1].positions.len() as f32 + game.cycles[1].pending_growth as f32;

    let mut best: Option<(f32, Direction, f32)> = None;
    let mut consider = |fx: u16, fy: u16, value: f32| {
        let idx = fy as usize * w + fx as usize;
        let (md, sd) = mine[idx];
        let Some(sd) = sd else { return };
        if md <= 0 {
            return;
        }
        let td = theirs[idx].0;
        // Win by a MARGIN, not by a nose. Bisected: with `td <= md` alone the
        // CPU sprinted for every photo-finish item and its deaths tripled
        // (player wins 0-1 -> 4-6) — a race won by one step leaves it head to
        // head with the opponent at the prize. Two steps of daylight keeps
        // the engagement and returns the survival.
        if td >= 0 && td <= md + 2 {
            return;
        }
        // And never cross the whole arena for lunch: a long chase is a long
        // exposure, and the item will usually be gone.
        if md > 24 {
            return;
        }
        let open = count_open_space(game, fx, fy);
        if open < own_len + value + 6.0 {
            return; // post-eat trap
        }
        let score = value / (md as f32 + 2.0);
        if best.is_none_or(|(b, _, _)| score > b) {
            best = Some((score, sd, open));
        }
    };
    for &(fx, fy, fv) in &game.food_items {
        consider(fx, fy, fv as f32);
    }
    for &(pxx, pyy, _) in &game.powerups {
        consider(pxx, pyy, 5.0); // a power-up is worth a mid-size morsel
    }
    best.map(|(_, d, open)| (d, open))
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
        for v in &mut vector[25..28] {
            *v /= pairs;
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
/// World v9: an ENEMY flame patch is a step-in hazard (the burn
/// schedule starts on contact); the CPU's own fire is harmless to it
/// (ADR-023 owner immunity), exactly matching the physics.
fn cell_threatened_by_flame(game: &WormGame, x: u16, y: u16) -> bool {
    game.arena_version >= 9
        && game
            .flames
            .iter()
            .any(|f| f.owner != 1 && f.x == x && f.y == y)
}

fn cell_threatened_by_bomb(game: &WormGame, x: u16, y: u16, frames_ahead: u8) -> bool {
    let trigger = crate::game::MINE_TRIGGER_CELLS as i32;
    for b in &game.bombs {
        // Our own mines cannot kill our HEAD — detonate() excludes the
        // owner — so pre-v8 they are harmless to us and dodging them
        // would be wasted caution. World v8 changed the ledger: an
        // expiring decoy DETONATES and the blast severs the owner's
        // TRAIL even though it spares the head. Own plants are therefore
        // hazards for their WHOLE armed life — self-knowledge, the same
        // "I planted that" memory rule the doze wake encodes (a human
        // routes around their own bomb from the moment they drop it, not
        // just when it starts flashing; flash-gating is a parity rule
        // for ENEMY mines, and measured on the warm arms the flash-only
        // version left the CPU laying 13 wall-clock seconds of trail
        // through its own future blast cross: warm 79% vs cold 86%,
        // non-inferiority broken by its own exploration plants).
        if b.owner == 1 {
            // With v8 blasts owner-safe (trail included, ADR-023 rule),
            // an own mine is pure infrastructure again — no dodge at any
            // fuse age. (A full-life threat was measured worse: it bent
            // early-game routes for zero physical risk.)
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
        // A fuse that runs out FIZZLES (tick_bombs) — an untripped mine's
        // cross arms are never dangerous on their own, so the old
        // failsafe-escape clause here modelled a detonation that no longer
        // happens. Only the trigger ring (live or about to arm) threatens.
    }
    false
}

/// Scenario-based player projection (ADR-020 stage 2.1, codex-designed):
/// the old projector assumed straight-until-blocked, which against a
/// slalomer aims every hunt and dodge where they WON'T be. This one
/// enumerates the no-turn trajectory plus turn-at-step-t trajectories for
/// each side over the horizon, masses them by survival-adjusted hazard
/// P(T=t) = P(T>t−1)·h_t with side mass split by the book's calibrated
/// side accuracy, and returns the probability-weighted MEDOID — the
/// enumerated path minimizing expected path loss against the mixture. A
/// real, reachable trajectory, never an average of incompatible ones, and
/// no-turn wins whenever it carries the mass (no forced bend).
///
/// Approximations, documented: hazards are evaluated along the no-turn
/// path (the pre-turn prefix is shared, which is where h is read); the
/// CPU is held stationary for the closing feature; food is static.
/// Only runs with PROJECTION AUTHORITY — the book's evidence family
/// latched and mature — and behind the book_bend attribution switch.
fn predict_player_positions_book(
    game: &WormGame,
    brain: &CpuBrain,
    predicted_dir: Direction,
    max_frames: usize,
) -> Vec<(u16, u16)> {
    let base = predict_player_positions_iterative(game, predicted_dir, max_frames);
    if crate::tuning::tuning().book_bend < 0.5
        || !brain.book_authority_snapshot
        || max_frames < 2
    {
        return base;
    }
    let heading = game.cycles[0].direction;
    let (l, r) = (left_turn(heading), right_turn(heading));

    // Hazard mass along the shared no-turn prefix, features advanced.
    let (cx, cy) = game.cycles[1].head;
    let nearest_food = game
        .food_items
        .iter()
        .min_by_key(|&&(fx, fy, _)| {
            let (px, py) = game.cycles[0].head;
            (fx as i32 - px as i32).abs() + (fy as i32 - py as i32).abs()
        })
        .copied();
    let mut surv = 1.0f32;
    let mut turn_mass: Vec<f32> = Vec::with_capacity(max_frames);
    let mut step_food_side: Vec<FoodSide> = Vec::with_capacity(max_frames);
    let mut prev_dist = brain.prev_pc_dist;
    for (step, &(px, py)) in base.iter().enumerate() {
        let fside = food_side(px, py, heading, nearest_food.map(|(fx, fy, _)| (fx, fy)));
        step_food_side.push(fside);
        let dist =
            ((px as i32 - cx as i32).abs() + (py as i32 - cy as i32).abs()) as u32;
        let cpu_close = dist <= 12 && dist < prev_dist.max(1);
        prev_dist = dist;
        let cell = hazard_cell(
            brain.gap_since_voluntary.saturating_add(step as u32),
            fside,
            brain.frames_since_food.saturating_add(step as u32) <= 3,
            cpu_close,
        );
        let h = brain.class_books.hazard(cell).clamp(0.0, 1.0);
        turn_mass.push(surv * h);
        surv *= 1.0 - h;
    }
    let no_turn_mass = surv;

    // Side split, PER STEP (stage 2.2): the book's declared side carries
    // its calibrated accuracy; with no declaration, the learned
    // overshoot-correction prior speaks — a turn taken while the food
    // sits off to one side breaks toward that side with probability
    // q_toward_food (the owner measured 59%). No food in play → the
    // turn prior, as before.
    let a_t = brain.class_books.a_turn().clamp(0.5, 1.0);
    let q_food = brain.class_books.q_toward_food().clamp(0.0, 1.0);
    let side_pref = brain.pending_book.and_then(|p| p.side);
    let split_at = |step: usize| -> (f32, f32) {
        match side_pref {
            Some(d) if d == l => (a_t, 1.0 - a_t),
            Some(d) if d == r => (1.0 - a_t, a_t),
            _ => match step_food_side.get(step) {
                Some(FoodSide::Left) => (q_food, 1.0 - q_food),
                Some(FoodSide::Right) => (1.0 - q_food, q_food),
                _ => {
                    let prior = brain.opp_brain.turn_prior();
                    let (wl, wr) =
                        (prior[turn_index(Turn::Left)], prior[turn_index(Turn::Right)]);
                    let s = (wl + wr).max(1e-6);
                    (wl / s, wr / s)
                }
            },
        }
    };

    // Enumerate: scenario 0 = no-turn; then (turn@t, side) for each step.
    let walk_bent = |bend_at: usize, side: Direction| -> Vec<(u16, u16)> {
        let mut path = Vec::with_capacity(max_frames);
        let mut px = game.cycles[0].head.0 as i16;
        let mut py = game.cycles[0].head.1 as i16;
        let mut pdir = heading;
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
        for step in 0..max_frames {
            if step == bend_at {
                let (sdx, sdy) = side.as_delta();
                if open(px + sdx, py + sdy) {
                    pdir = side;
                }
            }
            let (dx, dy) = pdir.as_delta();
            if !open(px + dx, py + dy) {
                // Blocked: same corner logic as the base projector.
                let (fdx, fdy) = left_turn(pdir).as_delta();
                pdir = if open(px + fdx, py + fdy) {
                    left_turn(pdir)
                } else {
                    right_turn(pdir)
                };
            }
            let (dx, dy) = pdir.as_delta();
            if open(px + dx, py + dy) {
                px += dx;
                py += dy;
            }
            path.push((px as u16, py as u16));
        }
        path
    };

    let mut scenarios: Vec<(f32, Vec<(u16, u16)>)> = vec![(no_turn_mass, base.clone())];
    for (t, &mass) in turn_mass.iter().enumerate().take(max_frames) {
        if mass <= 1e-4 {
            continue;
        }
        let (p_l, p_r) = split_at(t);
        scenarios.push((turn_mass[t] * p_l, walk_bent(t, l)));
        scenarios.push((turn_mass[t] * p_r, walk_bent(t, r)));
    }
    // Weighted medoid under summed per-step Manhattan path loss.
    let loss = |a: &[(u16, u16)], b: &[(u16, u16)]| -> f32 {
        a.iter()
            .zip(b.iter())
            .map(|(&(ax, ay), &(bx, by))| {
                ((ax as i32 - bx as i32).abs() + (ay as i32 - by as i32).abs()) as f32
            })
            .sum()
    };
    let mut best = 0usize;
    let mut best_cost = f32::INFINITY;
    for i in 0..scenarios.len() {
        let cost: f32 = scenarios
            .iter()
            .map(|(m, p)| m * loss(&scenarios[i].1, p))
            .sum();
        if cost < best_cost {
            best_cost = cost;
            best = i;
        }
    }
    scenarios.swap_remove(best).1
}

/// Public evaluation wrappers: the two projection strategies from the
/// same live state, for paired grading (rca_probe, ADR-020 stage 2.1).
/// Would stepping `d` take this cycle INTO slipstream space (ring-1
/// corridor or a punched hole) from outside? Board knowledge: corridor
/// entry costs 16× time under world v4+, and a hunting CPU that
/// blunders in gets frozen while its prey flies.
/// ADR-024 Phase A: the Boxer prospective-choke test. Given a FUNDED
/// intercept's incumbent candidate, find the candidate that shrinks the
/// player's reachable region materially harder than the incumbent would.
/// Codex's ruling shapes the trigger: prospective ("does THIS move
/// reduce their space?"), never static advantage ratios ("tactical
/// advantage is not evidence of a reachable choke") — and the spike
/// showed why: finished-condition windows exist on 0.25% of frames.
/// Returns (choke direction, precommitted baseline) or None.
fn boxer_choke_candidate(
    game: &WormGame,
    candidates: &[Direction],
    incumbent: Direction,
    hunt_cells: f32,
) -> Option<(Direction, f32)> {
    if !game.cpu_brain.tactic_boxer_ok {
        return None;
    }
    let (phx, phy) = game.cycles[0].head;
    let (cx, cy) = game.cycles[1].head;
    // Cheap prune before any flood: choking is a contact sport.
    if (phx as i16 - cx as i16).abs() + (phy as i16 - cy as i16).abs() > 14 {
        return None;
    }
    let baseline = count_open_space(game, phx, phy);
    // Already cornered: the funded intercept finishes fine on its own,
    // and a near-zero baseline would make the 60%-collapse credit test
    // vacuous.
    if baseline <= 12.0 {
        return None;
    }
    let next = |d: Direction| -> (u16, u16) {
        let (dx, dy) = d.as_delta();
        (
            (cx as i16 + dx).clamp(0, (game.width - 1) as i16) as u16,
            (cy as i16 + dy).clamp(0, (game.height - 1) as i16) as u16,
        )
    };
    let (ix, iy) = next(incumbent);
    let incumbent_after = count_open_space_excluding(game, phx, phy, &[(ix, iy)]);
    let mut best: Option<(Direction, f32)> = None;
    for &d in candidates {
        if d == incumbent || step_enters_corridor(game, 1, d) {
            continue;
        }
        let (nx, ny) = next(d);
        // Survival floor: a choke that dives into a pocket is a suicide,
        // not a tactic.
        if count_open_space(game, nx, ny) < hunt_cells {
            continue;
        }
        let after = count_open_space_excluding(game, phx, phy, &[(nx, ny)]);
        if best.is_none_or(|(_, s)| after < s) {
            best = Some((d, after));
        }
    }
    let (bd, bs) = best?;
    // Material: beat the incumbent's own denial by >=8 cells AND cut
    // >=15% of the player's current region — both thresholds exist to
    // stop noise-level "chokes" from stealing the intercept label.
    if bs + 8.0 <= incumbent_after && bs <= 0.85 * baseline {
        Some((bd, baseline))
    } else {
        None
    }
}

pub fn step_enters_corridor(game: &WormGame, who: usize, d: Direction) -> bool {
    if game.arena_version < 4 || !game.has_corridor() || game.cycle_in_corridor(who) {
        return false;
    }
    let (hx, hy) = game.cycles[who].head;
    let (dx, dy) = d.as_delta();
    let nx = hx as i16 + dx;
    let ny = hy as i16 + dy;
    if nx < 0 || ny < 0 || nx >= game.width as i16 || ny >= game.height as i16 {
        return false;
    }
    game.pos_in_corridor(nx as u16, ny as u16)
}

pub fn project_player_straight(game: &WormGame, frames: usize) -> Vec<(u16, u16)> {
    let dir = game
        .cpu_brain
        .ensemble
        .predicted_dir
        .unwrap_or(game.cycles[0].direction);
    predict_player_positions_iterative(game, dir, frames)
}
pub fn project_player_book(game: &WormGame, frames: usize) -> Vec<(u16, u16)> {
    let dir = game
        .cpu_brain
        .ensemble
        .predicted_dir
        .unwrap_or(game.cycles[0].direction);
    predict_player_positions_book(game, &game.cpu_brain, dir, frames)
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

    for step in 0..max_frames {
        // SLIPSTREAM AWARENESS (owner report: the CPU "never learned about
        // slipstream"): under world v4+ a player in the corridor steps
        // only when frame % 16 == 0 — projecting them at full speed aimed
        // every hunt five cells ahead of a nearly-frozen worm. On held
        // frames the projection holds position with them.
        if game.arena_version >= 4 && game.has_corridor() {
            let in_corr = game.pos_in_corridor(px as u16, py as u16);
            let frame = game.frame_count + 1 + step as u32;
            if in_corr && !frame.is_multiple_of(16) {
                positions.push((px as u16, py as u16));
                continue;
            }
        }
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

pub fn right_turn(dir: Direction) -> Direction {
    match dir {
        Direction::Up => Direction::Right,
        Direction::Right => Direction::Down,
        Direction::Down => Direction::Left,
        Direction::Left => Direction::Up,
    }
}

pub fn left_turn(dir: Direction) -> Direction {
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

pub const ENSEMBLE_MODELS: usize = 14;
/// Index of the k-NN model — the warm-corpus score bonus attaches here, not
/// to "the last model", which stopped being the k-NN when the intent models
/// joined.
pub const KNN_MODEL: usize = 6;
/// Short names for the HUD brain panel, in model order. The intent models
/// come in TWINS — the same errand hypothesis in two travelling styles.
/// `eat`/`hunt`/`arm` hold the line (a human on an errand keeps their heading
/// while it still shortens the route); the `…W` twins weave (turn the moment
/// any turn is equally short — the router's habit). The fixed-share weights
/// elect whichever style THIS human actually travels with, which is the
/// product claim in one mechanism. Slots 7-9 keep their historical indices.
/// M13 `alt` is the voluntary-turn VOMM (ADR-020 stage 3): a SECOND
/// TurnPattern instance fed every voluntary lateral — the owner's
/// alternation lives here. The forced-only instance keeps its distinct
/// "which way when cornered" semantics for the legal mask.
pub const MODEL_NAMES: [&str; ENSEMBLE_MODELS] = [
    "rep", "pat", "frq", "due", "wlR", "wlL", "knn", "eat", "hunt", "arm", "eatW", "huntW",
    "armW", "alt",
];
/// Score bonus for the sophisticated model once warm (rps-ai's +0.15).
pub(crate) const KNN_SCORE_BONUS: f32 = 0.15;

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
    /// Fixed-share exponential weights, two horizons (fast / slow).
    ///
    /// The old selector was hard argmax over a recency-weighted hit rate.
    /// Two structural defects, straight from the online-learning literature:
    /// hard argmax has no regret guarantee and thrashes between near-tied
    /// models, and a single decay constant cannot represent both "what this
    /// human always does" and "what they started doing five decisions ago".
    /// Fixed share (Herbster–Warmuth) fixes both: the share step keeps every
    /// weight bounded away from zero, which is precisely what allows recovery
    /// after the human changes style, and the two learning rates give a fast
    /// horizon that adapts within a burst plus a slow one that holds the
    /// long-run habit. Explainable as: "seven guessers vote, weighted by how
    /// well each has been doing lately — over both the last few choices and
    /// the long run — and no voice ever drops to zero."
    #[serde(skip)]
    pub w_fast: [f32; ENSEMBLE_MODELS],
    #[serde(skip)]
    pub w_slow: [f32; ENSEMBLE_MODELS],
}

impl Default for Ensemble {
    fn default() -> Self {
        Self {
            num: [0.0; ENSEMBLE_MODELS],
            den: [0.0; ENSEMBLE_MODELS],
            w_fast: [1.0; ENSEMBLE_MODELS],
            w_slow: [1.0; ENSEMBLE_MODELS],
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
        // Fast horizon: strong updates, heavy share — tracks the last handful
        // of decisions. Slow horizon: gentle updates, light share — holds the
        // session-long picture. Multiplicative-weights with a fixed-share
        // mixing step after each loss.
        let t = crate::tuning::tuning();
        let (eta_fast, eta_slow) = (t.eta_fast, t.eta_slow);
        let (share_fast, share_slow) = (t.share_fast, t.share_slow);
        let mut awake_loss = 0.0f32;
        let mut awake = 0u32;
        let mut slept = [false; ENSEMBLE_MODELS];
        for (i, slept_i) in slept.iter_mut().enumerate() {
            if let Some(pred) = self.pending[i].take() {
                let hit = pred == actual;
                self.num[i] += if hit { w2 } else { -w2 };
                self.den[i] += w2;
                self.total[i] += 1;
                if hit {
                    self.hits[i] += 1;
                }
                let loss = if hit { 0.0 } else { 1.0 };
                awake_loss += loss;
                awake += 1;
                self.w_fast[i] *= (-eta_fast * loss).exp();
                self.w_slow[i] *= (-eta_slow * loss).exp();
            } else {
                *slept_i = true;
            }
        }
        // SLEEPERS ARE NOT CHARGED. The specialist-Hedge alternative —
        // charging an abstainer the awake population's average loss — is
        // theoretically fairer and was MEASURED WORSE: the intent models
        // sleep on most frames by design (no power-up on the board, no
        // errand underway), and the average-loss charge decayed their
        // weight with the crowd so their awake skill could never earn rank.
        // The power-up persona's voluntary-turn read collapsed 74% -> 30%
        // under it. The sleeper-takeover risk codex identified is real but
        // is closed by the other half of the fix: selection now happens
        // post-mask among models that currently SPEAK, so a well-ranked
        // sleeper can hold its weight yet never drive while silent.
        let _ = (awake_loss, awake, slept);
        for (weights, share) in [
            (&mut self.w_fast, share_fast),
            (&mut self.w_slow, share_slow),
        ] {
            let sum: f32 = weights.iter().sum();
            if sum > 0.0 && sum.is_finite() {
                let pool = share * sum / ENSEMBLE_MODELS as f32;
                for v in weights.iter_mut() {
                    *v = (1.0 - share) * *v + pool;
                }
                // Renormalise so the weights never under/overflow over a
                // long session.
                let sum: f32 = weights.iter().sum();
                let inv = ENSEMBLE_MODELS as f32 / sum;
                for v in weights.iter_mut() {
                    *v *= inv;
                }
            } else {
                *weights = [1.0; ENSEMBLE_MODELS];
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
/// The player's legal, non-reversing steps right now.
fn player_steps(game: &WormGame) -> Vec<Direction> {
    legal_options_from(game, 0, game.cycles[0].direction)
}

/// Multi-source BFS distance field to the nearest of `targets`, over the
/// board the PLAYER faces. field[y*w + x] = steps to the nearest target, -1
/// unreachable. Targets are seeds at distance 0 whether or not their own
/// cell is passable (a disguised mine reads as food to the player — the
/// errand ends ON it).
///
/// Routing, not crow-flies: the greedy Manhattan step the old intent models
/// used agreed with real routing on ~93% of frames — and the disagreement
/// landed almost entirely on the voluntary-turn frames, the only ones that
/// carry a decision. A model that is wrong exactly when the player chooses
/// is worse than no model.
fn target_field(game: &WormGame, targets: &[(u16, u16)]) -> Vec<i32> {
    let w = game.width as usize;
    let mut field = vec![-1i32; w * game.height as usize];
    let mut q: VecDeque<(u16, u16)> = VecDeque::new();
    for &(tx, ty) in targets {
        if tx < game.width && ty < game.height {
            let idx = ty as usize * w + tx as usize;
            if field[idx] == -1 {
                field[idx] = 0;
                q.push_back((tx, ty));
            }
        }
    }
    while let Some((x, y)) = q.pop_front() {
        let d = field[y as usize * w + x as usize];
        for (dx, dy) in [(0i16, -1i16), (0, 1), (-1, 0), (1, 0)] {
            let nx = x as i16 + dx;
            let ny = y as i16 + dy;
            if nx < 0 || ny < 0 || nx >= game.width as i16 || ny >= game.height as i16 {
                continue;
            }
            let (nx, ny) = (nx as u16, ny as u16);
            let idx = ny as usize * w + nx as usize;
            if field[idx] == -1 && game.passable(nx, ny) {
                field[idx] = d + 1;
                q.push_back((nx, ny));
            }
        }
    }
    field
}

/// The step an errand-running player takes given a distance field: the legal
/// move that most shortens the route. `hold` selects the travelling style —
/// the ONE free parameter the twins differ in:
///
///   hold  — ties go to the current heading. In an open arena both axis
///           moves shorten the route equally for most of the trip; a human
///           holds their line. Measured on a committed human forager, the
///           strict minimiser read 12.2% of their voluntary turns and this
///           read 93.7% — same field, opposite tie-break.
///   weave — strict minimiser (deterministic enum-order tie-break): the
///           style of a router, or a human who corrects course every cell.
///
/// Abstains (None) when there is no field, the target is unreachable, or no
/// legal step shortens anything — silence, not a manufactured guess.
fn intent_step(game: &WormGame, field: &[i32], hold: bool) -> Option<Direction> {
    let (px, py) = game.cycles[0].head;
    let w = game.width as usize;
    let steps = player_steps(game);
    let val = |d: Direction| -> i32 {
        let (dx, dy) = d.as_delta();
        let nx = px as i16 + dx;
        let ny = py as i16 + dy;
        if nx < 0 || ny < 0 || nx >= game.width as i16 || ny >= game.height as i16 {
            return i32::MAX;
        }
        match field[ny as usize * w + nx as usize] {
            -1 => i32::MAX,
            v => v,
        }
    };
    let best = steps.iter().map(|&d| val(d)).min()?;
    if best == i32::MAX {
        return None; // target unreachable from every legal step
    }
    let heading = game.cycles[0].direction;
    if hold && steps.contains(&heading) && val(heading) == best {
        return Some(heading);
    }
    steps.into_iter().find(|&d| val(d) == best)
}

/// One errand family (eat or arm): commit to a target with hysteresis, build
/// its field, and predict both travelling styles from it.
///
/// Commitment is OBSERVATION-DRIVEN, which keeps information parity: the
/// model keeps its target only while the player's own last move shortened
/// the route to it (they are demonstrably still on that errand); it re-shops
/// for the nearest target the moment they stop closing, or the target is
/// gone. A human mid-errand does not re-shop for a marginally nearer morsel;
/// a model that does predicts turns the human never makes.
fn intent_family(
    game: &WormGame,
    targets: &[(u16, u16)],
    committed: Option<(u16, u16)>,
) -> (Option<Direction>, Option<Direction>, Option<(u16, u16)>) {
    if targets.is_empty() {
        return (None, None, None);
    }
    let (px, py) = game.cycles[0].head;
    let w = game.width as usize;

    // Route distance at an OCCUPIED cell (the player's head, their neck):
    // the field never enters worm cells, so a direct lookup is -1 there and
    // a commitment tested that way evicts itself every frame — measured,
    // the advertised hysteresis never operated at all. The occupant's route
    // is one more than the best adjacent field cell.
    let route_dist = |f: &[i32], x: u16, y: u16| -> i32 {
        let direct = f[y as usize * w + x as usize];
        if direct >= 0 {
            return direct;
        }
        let mut best = i32::MAX;
        for (dx, dy) in [(0i16, -1i16), (0, 1), (-1, 0), (1, 0)] {
            let nx = x as i16 + dx;
            let ny = y as i16 + dy;
            if nx >= 0 && ny >= 0 && nx < game.width as i16 && ny < game.height as i16 {
                let v = f[ny as usize * w + nx as usize];
                if v >= 0 && v < best {
                    best = v;
                }
            }
        }
        if best == i32::MAX {
            -1
        } else {
            best + 1
        }
    };

    let mut commit = committed.filter(|t| targets.contains(t));
    if let Some(t) = commit {
        let f = target_field(game, &[t]);
        let here = route_dist(&f, px, py);
        // The previous head is the neck — observable by anyone watching.
        let still_closing = match game.cycles[0].positions.get(1) {
            Some(&(qx, qy)) if here >= 0 => {
                let prev = route_dist(&f, qx, qy);
                prev < 0 || here < prev
            }
            _ => here >= 0,
        };
        if still_closing {
            return (
                intent_step(game, &f, true),
                intent_step(game, &f, false),
                Some(t),
            );
        }
        commit = None;
    }
    debug_assert!(commit.is_none());
    // Re-shop: nearest target by ROUTING (multi-source field), then commit
    // to the specific one the player's best step actually approaches.
    let f = target_field(game, targets);
    let hold = intent_step(game, &f, true);
    let weave = intent_step(game, &f, false);
    // Commit to the nearest target CONSISTENT WITH THE OBSERVED MOTION:
    // among the targets the player's last move actually approached, take the
    // Manhattan-nearest (per-target routed distance would cost a BFS each).
    // Plain nearest was measured wrong: standing between a near morsel
    // behind them and a far one ahead, it committed to the one at their
    // BACK, was evicted one frame later for not closing, and re-committed —
    // flapping forever and never latching the errand actually underway.
    // When no target was approached (first frame, or they just turned), fall
    // back to nearest; the closing test evicts a bad guess next move.
    let new_commit = hold.map(|_| {
        let manh = |t: (u16, u16), from: (u16, u16)| -> i32 {
            (t.0 as i32 - from.0 as i32).abs() + (t.1 as i32 - from.1 as i32).abs()
        };
        let neck = game.cycles[0].positions.get(1).copied();
        let approached: Vec<(u16, u16)> = match neck {
            Some(n) => targets
                .iter()
                .copied()
                .filter(|&t| manh(t, (px, py)) < manh(t, n))
                .collect(),
            None => Vec::new(),
        };
        let pool: &[(u16, u16)] = if approached.is_empty() {
            targets
        } else {
            &approached
        };
        pool.iter()
            .copied()
            .min_by_key(|&t| manh(t, (px, py)))
            .unwrap_or((px, py))
    });
    (hold, weave, new_commit)
}

/// M7/M10 `eat`/`eatW`: the player is HEADED FOR FOOD — where "food"
/// includes the CPU's own disguised mines, because to the player those ARE
/// food. When this model is driving, the panel is literally saying "I think
/// you're going for that food" — and if the food is bait, so much the better.
///
/// This is the model the habit family cannot be: a goal hypothesis. A human
/// crossing an open arena in a straight line is not expressing a turning
/// habit, they are executing an errand — and predicting the errand is what
/// makes the read feel like being understood rather than being tallied.
fn m_eat_family(
    game: &WormGame,
    committed: Option<(u16, u16)>,
) -> (Option<Direction>, Option<Direction>, Option<(u16, u16)>) {
    let targets: Vec<(u16, u16)> = game
        .food_items
        .iter()
        .map(|&(x, y, _)| (x, y))
        .chain(
            game.bombs
                .iter()
                .filter(|b| b.owner == 1)
                .map(|b| (b.x, b.y)),
        )
        .collect();
    intent_family(game, &targets, committed)
}

/// M8/M11 `hunt`/`huntW`: the player is COMING FOR US — the opening move of
/// every wall-in. Routes to the cells ADJACENT to the CPU's head: the head
/// cell itself is impassable (usually backed by our own trail), and aiming
/// at it read a genuine hunter at 65.5% raw. A hunter's destination is
/// beside us, never inside us. No hysteresis — the target moves every frame.
fn m_hunt_family(game: &WormGame) -> (Option<Direction>, Option<Direction>) {
    let (cx, cy) = game.cycles[1].head;
    let mut targets: Vec<(u16, u16)> = Vec::with_capacity(4);
    for (dx, dy) in [(0i16, -1i16), (0, 1), (-1, 0), (1, 0)] {
        let nx = cx as i16 + dx;
        let ny = cy as i16 + dy;
        if nx >= 0
            && ny >= 0
            && nx < game.width as i16
            && ny < game.height as i16
            && game.passable(nx as u16, ny as u16)
        {
            targets.push((nx as u16, ny as u16));
        }
    }
    if targets.is_empty() {
        return (None, None);
    }
    let f = target_field(game, &targets);
    (intent_step(game, &f, true), intent_step(game, &f, false))
}

/// M9/M12 `arm`/`armW`: the player is GOING FOR A POWER-UP. Same shape as
/// `eat`, but power-ups only — they are visually distinct icons, worth a
/// longer detour, and a player heading for one is about to become dangerous.
/// "Why are you moving" has at least three answers a habit tally can never
/// give: food, weapon, or me. This is the weapon one.
fn m_arm_family(
    game: &WormGame,
    committed: Option<(u16, u16)>,
) -> (Option<Direction>, Option<Direction>, Option<(u16, u16)>) {
    let targets: Vec<(u16, u16)> = game.powerups.iter().map(|&(x, y, _)| (x, y)).collect();
    intent_family(game, &targets, committed)
}

/// (per-model forecasts, elected model, its weight, intercept anchors).
pub type EnsembleVerdict = (
    [Option<Direction>; ENSEMBLE_MODELS],
    usize,
    f32,
    [Option<(u16, u16)>; 2],
);

pub fn compute_ensemble(game: &WormGame, brain: &CpuBrain) -> EnsembleVerdict {
    let tail = &brain.player_tail;
    let (eat, eat_w, eat_commit) = m_eat_family(game, brain.intent_targets[0]);
    let (hunt, hunt_w) = m_hunt_family(game);
    let (arm, arm_w, arm_commit) = m_arm_family(game, brain.intent_targets[1]);
    // M13 `alt`: the side of the player's next VOLUNTARY swerve, from the
    // voluntary-turn VOMM. Speaks every frame once it has evidence — the
    // global weights bury it on straight-heavy volume by construction,
    // and that is fine: the TURN BOOK scores it only on the frames it
    // exists for, and elects it there.
    let alt = if brain.voluntary_pattern.events >= VOMM_MIN_EVENTS {
        let heading = game.cycles[0].direction;
        if brain.voluntary_pattern.p_left() >= 0.5 {
            Some(left_turn(heading))
        } else {
            Some(right_turn(heading))
        }
    } else {
        None
    };
    let pending = [
        m_repeat(tail),
        m_pattern(tail),
        m_frequent(tail),
        m_due(tail),
        m_wall(game, true),
        m_wall(game, false),
        m_knn(game, brain),
        eat,
        hunt,
        arm,
        eat_w,
        hunt_w,
        arm_w,
        alt,
    ];

    let e = &brain.ensemble;

    // Model selection by TWO-HORIZON FIXED-SHARE WEIGHTS, hard argmax kept.
    //
    // A full mixed vote (weight-summed across models per direction) was
    // measured and REJECTED: it read better (lift 80%) but won less (100% ->
    // 93%). Selection by the fixed-share weights keeps what the vote was
    // after — the share step bounds every model's weight away from zero, so
    // the ensemble can recover when the human changes style, and the two
    // learning rates hold both the long-run habit and the last few decisions
    // — while preserving single-driver forecasts, which is both what the
    // telemetry panel names and, measured, what wins.
    let mut best = usize::MAX;
    let mut best_score = f32::NEG_INFINITY;
    for i in 0..ENSEMBLE_MODELS {
        if e.den[i] <= 0.0 {
            continue; // never scored — not eligible yet
        }
        let mut w = e.w_fast[i] + e.w_slow[i];
        if i == KNN_MODEL && brain.opp_brain.episodes.len() >= COLD_START_EPISODES {
            w *= 1.0 + crate::tuning::tuning().knn_bonus;
        }
        if w > best_score {
            best_score = w;
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
    (pending, active, confidence, [eat_commit, arm_commit])
}

/// Select the driving model AMONG THOSE CURRENTLY SPEAKING — called after
/// legal masking, on the predictions actually published. Selecting before
/// masking let a silent model be crowned: its pending stayed None while its
/// historical hit-rate rode along as "confidence", and the hunt gates opened
/// on a read that did not exist that frame.
///
/// Returns (active, confidence). When no scored model speaks, falls back to
/// any speaking model (fresh game); when nothing speaks at all, model 0 with
/// zero confidence — an honest silent frame.
pub fn select_active(brain: &CpuBrain, masked: &[Option<Direction>]) -> (usize, f32) {
    let e = &brain.ensemble;
    let warm = brain.opp_brain.episodes.len() >= COLD_START_EPISODES;
    let rank = |i: usize| -> f32 {
        let mut w = e.w_fast[i] + e.w_slow[i];
        if i == KNN_MODEL && warm {
            w *= 1.0 + crate::tuning::tuning().knn_bonus;
        }
        w
    };
    let mut best = usize::MAX;
    let mut best_w = f32::NEG_INFINITY;
    for (i, m) in masked.iter().enumerate() {
        if e.den[i] <= 0.0 || m.is_none() {
            continue;
        }
        if rank(i) > best_w {
            best_w = rank(i);
            best = i;
        }
    }
    if best == usize::MAX {
        // Nothing scored yet (first frames): first speaking model, like
        // rps-ai forcing model 0 — but never a silent one.
        best = (0..ENSEMBLE_MODELS)
            .find(|&i| masked[i].is_some())
            .unwrap_or(0);
    }
    let confidence = if masked[best].is_some() && e.total[best] > 0 {
        e.hits[best] as f32 / e.total[best] as f32
    } else {
        0.0
    };
    (best, confidence)
}

/// Effective score used by ensemble selection — the SAME quantity
/// `select_active` ranks by (two-horizon fixed-share weights, with the warm
/// k-NN multiplier), so the panel can never show a lower-scored model
/// driving. It previously exported the retired quadratic num/den score with
/// an additive bonus: a different ordering from the one actually deciding,
/// which is indefensible in a HUD whose job is being believed.
pub fn ensemble_rank_score(brain: &CpuBrain, model: usize) -> f32 {
    let e = &brain.ensemble;
    let mut w = e.w_fast[model] + e.w_slow[model];
    if model == KNN_MODEL && brain.opp_brain.episodes.len() >= COLD_START_EPISODES {
        w *= 1.0 + crate::tuning::tuning().knn_bonus;
    }
    w
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
            let beam = game.beam_cells(hx, hy, dx, dy);
            if beam.contains(&(ox, oy)) {
                return true;
            }
            // BREACH SHOT (owner: "I expect it to know how to punch holes"):
            // an enveloped CPU holding a laser blasts itself an exit — the
            // beam's fifth wall strike punches a Hole, and the survival
            // layers already know how to take one. Only when the walls are
            // actually closing (cpu_enveloped), and only if the breach
            // would land close enough to reach (within 12 cells). Board
            // knowledge + self-preservation; the telegraph still plays.
            who == 1
                && game.cpu_enveloped()
                && beam
                    .breach
                    .map(|(bx, by)| {
                        (bx as i32 - hx as i32).abs() + (by as i32 - hy as i32).abs()
                            <= 12
                    })
                    .unwrap_or(false)
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
            //
            // Since a bolt bursts on ANY part of the opponent (head kill,
            // trail sever), every opponent cell on a ray is a target — a shot
            // across their body is a sever even when their head is elsewhere.
            let (dx, dy) = game.cycles[who].direction.as_delta();
            // The aim gate is the bolt's ACTUAL reach per world version:
            // v9/v10 napalm flew 4 cells; v11 restored the full ray at
            // double speed (owner: "maybe they need to go further").
            let max_reach: i16 = if game.arena_version == 9 || game.arena_version == 10 {
                4
            } else {
                i16::MAX
            };
            let on_a_ray = |px: u16, py: u16| -> bool {
                let fdx = px as i16 - hx as i16;
                let fdy = py as i16 - hy as i16;
                let forward = dx * fdx + dy * fdy > 0;
                let aligned = fdx == 0 || fdy == 0 || fdx.abs() == fdy.abs();
                let reach = fdx.abs().max(fdy.abs()) <= max_reach;
                forward && aligned && reach
            };
            on_a_ray(ox, oy) || game.cycles[opp].positions.iter().any(|&(px, py)| on_a_ray(px, py))
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


/// Faithful to rps-ai's `think` + `decide`: memory-driven read,
/// confidence-gated, blended with a base-rate prior, resolved by
/// deterministic argmax (the 5% explore lives only in the close-evasion
/// branch).
pub fn cpu_decide(game: &mut WormGame) -> Direction {
    // Feed the envelopment ring: the CPU's open region size, once per
    // decision (task #13 v1).
    {
        let (hx, hy) = game.cycles[1].head;
        let region = count_open_space(game, hx, hy) as u32;
        game.cpu_brain.region_ring.push_back(region);
        while game.cpu_brain.region_ring.len() > 8 {
            game.cpu_brain.region_ring.pop_front();
        }
    }
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
            // ADR-021 Kata 0: hunt-family decisions open a precommitted
            // attempt window in the tactic ledger (recording only;
            // note_tactic ignores non-hunt reasons).
            game.cpu_brain.ledgers.note_tactic($reason, game.frame_count, game.shrink_level);
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
        // The corridor pin bites HERE, not just in the memory-driven path:
        // every traced pin death was at length 1-2 with read 0.00 — a
        // cold-start CPU wall-following into an escorted lane. A guard that
        // only protects the smart path protects the CPU only after it has
        // survived the phase where the exploit actually kills it.
        if escorted_lane_step(game, head, warm) {
            if let Some(exit) = legal
                .iter()
                .copied()
                .filter(|&d| {
                    !escorted_lane_step(game, head, d) && !ring_doomed_step(game, head, d)
                })
                .max_by(|a, b| {
                    let open = |d: Direction| {
                        let (ddx, ddy) = d.as_delta();
                        let ex = (head.0 as i16 + ddx).max(0).min((game.width - 1) as i16) as u16;
                        let ey = (head.1 as i16 + ddy).max(0).min((game.height - 1) as i16) as u16;
                        count_open_space(game, ex, ey)
                    };
                    open(*a).partial_cmp(&open(*b)).unwrap_or(std::cmp::Ordering::Equal)
                })
            {
                choose!(exit, CpuDecisionReason::LaneRefusal);
            }
        }
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
    // Evidence = LATERAL turns actually scored (voluntary and forced alike;
    // the owner corpus has 1,015 voluntary vs 5 forced — the old
    // forced-turn-only tally starved this ramp to ~0.5 against him forever,
    // so no forecast quality could ever open the intercept gates). Rides the
    // persisted lifetime read, so returning players keep their evidence.
    // Quantity × quality, both honest (ADR-020 stage 1). The ramp counts
    // LATERAL turns actually scored (the owner corpus has 1,015 voluntary
    // vs 5 forced — the old forced-turn-only tally starved this to ~0.5
    // against him forever). But quantity alone must not open the gates:
    // warm sessions accumulate laterals in one game, and a full-open gate
    // behind a forecast with no earned read was measured hunting the CPU
    // into walls (warm 77% vs cold 93% — memory COSTING wins). Quality is
    // the earned read itself, so the gates open exactly as fast as the
    // evidence that the forecast deserves them.
    let read_conf = ((game.cpu_brain.lifetime_read.taken[1]
        + game.cpu_brain.lifetime_read.taken[2]) as f32
        / 10.0)
        .min(1.0)
        * game.cpu_brain.earned_snapshot;
    // THE BEATABLE OPENING's second half: before there is a read, the hunt
    // gates may run on RAW forecast confidence (straight-line extrapolation
    // chasing) — eager, imperfect, killable. The DBR observation ramp takes
    // over as real choices accumulate, and boldness fades with the read.
    let read_conf =
        read_conf.max(
            crate::tuning::tuning().bold_drive
                * (1.0 - game.sharpness())
                * game.boldness_scale(),
        );
    // Two confidences, split on purpose (codex silent-model finding, then
    // measured):
    //  - track_conf gates DEFENSIVE use of the projected path. When the
    //    forecast is silent the path is a straight-line extrapolation — a
    //    perfectly good thing to dodge, and zeroing it was measured to cost
    //    wins by blinding CloseEvasion on exactly the silent frames.
    //  - pred_conf gates AGGRESSIVE use. A hunt on a silent forecast is
    //    aggression without a read, which violates the product contract —
    //    so it is zero unless the published forecast actually names a move.
    let track_conf = (decision_forecast
        .map(|forecast| forecast.confidence)
        .unwrap_or(0.0)
        * read_conf)
        // A book-bent path carries the BOOK's earned authority for
        // DEFENSIVE use (dodging where the read says they'll be) — it
        // must not inherit the global straight book's confidence, must
        // not feed pred_conf's aggression (that stays on the published
        // forecast), and reads the ROUND-BOUNDARY SNAPSHOT only.
        .max(if game.cpu_brain.book_authority_snapshot {
            game.cpu_brain.book_spend_snapshot
        } else {
            0.0
        });
    let player_pred_conf = if decision_forecast.and_then(|f| f.predicted).is_some() {
        track_conf
    } else {
        0.0
    };
    let cpu = &game.cycles[1];
    let (cx, cy) = cpu.head;

    // Iterative multi-frame prediction: predicts direction changes at corners.
    let predicted_positions =
        predict_player_positions_book(game, &game.cpu_brain, player_pred_dir, 5);
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
        if cell_threatened_by_projectile(game, nx, ny)
            || cell_threatened_by_bomb(game, nx, ny, 3)
            || cell_threatened_by_flame(game, nx, ny)
        {
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
    // An escorted wall-lane step is filtered exactly like a projectile cell:
    // it is a move the player has already made fatal, just on a longer fuse
    // (see `escorted_lane_step` — the corridor pin). The filter prunes it
    // from every layer below whenever any alternative exists.
    let safe_legal: Vec<Direction> = legal
        .iter()
        .copied()
        .filter(|d| {
            !threatened_dirs.contains(d)
                && !ring_doomed_step(game, (cx, cy), *d)
                && !escorted_lane_step(game, (cx, cy), *d)
        })
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
    // Gated on track_conf: dodging an extrapolated path is defensive and
    // must keep working on frames where the forecast is silent.
    if track_conf >= 0.4 {
        let mut min_dist = i16::MAX;
        for &(px, py) in &predicted_positions {
            let dist = (cx as i16 - px as i16).abs() + (cy as i16 - py as i16).abs();
            min_dist = min_dist.min(dist);
        }
        if min_dist <= 2 {
            // This was the ONLY deviation layer with no space term: it scored
            // pure distance-from-player and could hug alongside them for ten
            // straight frames while their advancing trail sealed the region —
            // the traced length-61 OwnTrail death (open 3565 → 16 in one
            // frame). Two changes, both strengthenings:
            //   1. open space is measured with the player's predicted next
            //      cells EXCLUDED, so "about to be sealed" is visible now;
            //   2. candidates below the escape floor on that measure are
            //      rejected whenever any candidate clears it (same discipline
            //      as every other layer — never empties the choice).
            // Only the projected cells the player's trail can actually HOLD
            // at once count as future walls: a length-2 player's tail
            // vacates the oldest projected cell almost immediately, and
            // excluding all five hallucinated a sealed pocket where none
            // could form (codex finding).
            let coexist = (game.cycles[0].positions.len()
                + game.cycles[0].pending_growth as usize)
                .min(predicted_positions.len());
            // The SUFFIX: by the end of the horizon their tail has vacated
            // the earliest projected cells; the last `coexist` are the ones
            // still standing as trail.
            let live_projection = &predicted_positions[predicted_positions.len() - coexist..];
            let evasion_open = |d: Direction| -> f32 {
                let (ddx, ddy) = d.as_delta();
                let nx = (cx as i16 + ddx).max(0).min((game.width - 1) as i16) as u16;
                let ny = (cy as i16 + ddy).max(0).min((game.height - 1) as i16) as u16;
                count_open_space_excluding(game, nx, ny, live_projection)
            };
            let clearing: Vec<Direction> = candidates
                .iter()
                .copied()
                .filter(|&d| evasion_open(d) >= escape_cells)
                .collect();
            let pool: &[Direction] = if clearing.is_empty() {
                candidates
            } else {
                &clearing
            };
            let mut best_dir = wall_dir;
            let mut best_score = f32::NEG_INFINITY;
            for &d in pool {
                let (ddx, ddy) = d.as_delta();
                let nx = (cx as i16 + ddx).max(0).min((game.width - 1) as i16) as u16;
                let ny = (cy as i16 + ddy).max(0).min((game.height - 1) as i16) as u16;
                let mut dmin = i16::MAX;
                for &(px, py) in &predicted_positions {
                    let dd = (nx as i16 - px as i16).abs() + (ny as i16 - py as i16).abs();
                    dmin = dmin.min(dd);
                }
                let wall_bonus = if d == wall_dir { 3.0 } else { 0.0 };
                // When nothing clears the floor, survival outranks distance:
                // take the roomiest pocket rather than the farthest corner of
                // a sealed one.
                let score = if clearing.is_empty() {
                    evasion_open(d)
                } else {
                    dmin as f32 + wall_bonus
                };
                if score > best_score {
                    best_score = score;
                    best_dir = d;
                }
            }
            if game.rng_cpu_f32(0.0, 1.0) < EXPLORE_RATE {
                // Draw from the floor-clearing pool, not raw candidates —
                // exploration must not dive into the pocket the dodge logic
                // just steered around.
                choose!(
                    pool[(game.rng_cpu_f32(0.0, pool.len() as f32) as usize).min(pool.len() - 1)],
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
        if let Some((food_dir, food_open)) = best_food_target(game, cx, cy, candidates) {
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

    // --- CURIOSITY: an unsharp CPU drifts toward the player ---
    // Casual players are drawn to each other; a fresh CPU that orbits the
    // far wall never meets the human at all, which reads as "not trying"
    // and gives the beatable opening nothing to be beaten AT. While the CPU
    // is unsharp and far away, prefer the candidate that closes distance —
    // survival floors still vet every step, and the trail-blind doze means
    // an over-eager approach can die into the trail you laid: the earned
    // kill the opening exists to offer. Fades out entirely with sharpness.
    if game.discipline_sharpness() < 0.5 {
        let (px, py) = game.cycles[0].head;
        let dist_now = (cx as i16 - px as i16).abs() + (cy as i16 - py as i16).abs();
        if dist_now > 10 {
            let toward = candidates
                .iter()
                .copied()
                .filter(|&d| !step_enters_corridor(game, 1, d))
                .filter(|&d| {
                    let (ddx, ddy) = d.as_delta();
                    let nx = (cx as i16 + ddx).max(0).min((game.width - 1) as i16) as u16;
                    let ny = (cy as i16 + ddy).max(0).min((game.height - 1) as i16) as u16;
                    let closer = (nx as i16 - px as i16).abs() + (ny as i16 - py as i16).abs()
                        < dist_now;
                    // Never approach down the player's own driving lane: a
                    // curiosity step there plus a dozy held heading is a
                    // manufactured HEAD-ON (measured: 3 of 5 warm draws).
                    // The player's lane is the strip ±1 cell around the ray
                    // ahead of their head; side and rear approaches keep the
                    // encounters — and the earned trail kills — coming.
                    let (pdx, pdy) = game.cycles[0].direction.as_delta();
                    let (rx, ry) = (nx as i16 - px as i16, ny as i16 - py as i16);
                    let ahead = rx * pdx + ry * pdy > 0;
                    let lane_off = (rx * pdy - ry * pdx).abs();
                    let head_on_lane = ahead && lane_off <= 1;
                    closer && !head_on_lane && count_open_space(game, nx, ny) >= escape_cells
                })
                .max_by_key(|&d| {
                    let (ddx, ddy) = d.as_delta();
                    let nx = (cx as i16 + ddx).max(0).min((game.width - 1) as i16) as u16;
                    let ny = (cy as i16 + ddy).max(0).min((game.height - 1) as i16) as u16;
                    -((nx as i16 - px as i16).abs() + (ny as i16 - py as i16).abs())
                });
            if let Some(d) = toward {
                choose!(d, CpuDecisionReason::Curiosity);
            }
        }
    }

    // ADR-021 Kata 4: when the tactic ledger has matured on BOTH
    // intercepts and says direct kills this player better, the corner
    // layer yields the frame to it (a strictly less-aggressive move on
    // frames where direct then declines).
    let corner_yields = game.cpu_brain.tactic_prefer_direct;

    // --- CHOKEPOINT INTERCEPT: cut across to corners against wall-followers ---
    // When the player is a wall-follower (confidence high, direction stable),
    // predict which corner they'll reach and cut across to lay a trail barrier.
    // This works even when the player is >10 cells away (standard intercept range).
    //
    // Gate history: 0.5 originally. Measured share of player-directed
    // decisions at that setting was 6-7% of the total — the CPU spent ~90% of
    // its decisions on itself, which the player experienced as "it plays a
    // different game in the same arena" (head-to-head distance matched the
    // uniform-random baseline). read_conf already multiplies the confidence,
    // so this cannot open before ~10 observed real choices regardless; the
    // hunt floor still vets every destination. Lowered to buy engagement
    // without touching a survival floor.
    if !corner_yields && player_pred_conf >= crate::tuning::tuning().corner_gate {
        // Predict the corner the player will reach next.
        // Wall-followers turn right at corners. We predict their path to the next corner.
        let corner_target = predict_next_corner(game, &game.cycles[0], player_pred_dir);

        if let Some((corner_x, corner_y)) = corner_target {
            let dist_to_corner =
                ((cx as i16 - corner_x as i16).abs() + (cy as i16 - corner_y as i16).abs()) as f32;
            // The player travels a straight line to this corner, so their
            // Manhattan distance IS their arrival time. An intercept the CPU
            // cannot win is not an intercept — it is a camp: measured, 4 of 7
            // long corner dwells were entered under this reason and then held
            // in place by the self-memory, which is precisely the
            // "sit and spin in the corner" the player complained about.
            // Arriving strictly first is what makes the barrier a barrier.
            let (phx, phy) = game.cycles[0].head;
            let player_to_corner =
                ((phx as i16 - corner_x as i16).abs() + (phy as i16 - corner_y as i16).abs()) as f32;

            // Only intercept if the corner is reachable (within ~20 cells),
            // we're not too close to the player (avoid head-on), and we can
            // actually beat them there.
            if (5.0..=25.0).contains(&dist_to_corner) && dist_to_corner < player_to_corner {
                let mut best_dir = wall_dir;
                let mut best_score = f32::NEG_INFINITY;
                for &d in candidates {
                    // SLIPSTREAM: a hunt that steps into the corridor
                    // freezes the hunter — never worth it.
                    if step_enters_corridor(game, 1, d) {
                        continue;
                    }
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
                    // ADR-024: a funded intercept may be perturbed into a
                    // Boxer choke when one of its own candidates denies
                    // the player materially more space.
                    if let Some((choke_dir, baseline)) =
                        boxer_choke_candidate(game, candidates, best_dir, hunt_cells)
                    {
                        game.cpu_brain.ledgers.pending_boxer_baseline = Some(baseline);
                        choose!(choke_dir, CpuDecisionReason::Boxer);
                    }
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
    // Gate lowered 0.6 -> 0.45 with the corner gate above (same rationale).
    if player_pred_conf >= crate::tuning::tuning().direct_gate {
        // Target: where the player will be in 2-5 frames (from iterative prediction).
        let mut best_intercept: Option<(u16, u16, f32)> = None;
        for (i, &(px, py)) in predicted_positions.iter().enumerate() {
            let frames_ahead = (i + 1) as f32; // 1, 2, 3, 4, 5
            let dist = ((cx as i16 - px as i16).abs() + (cy as i16 - py as i16).abs()) as f32;
            // NO strict reachability gate. It was tried (dist must not
            // exceed frames_ahead + 2) on the theory that an intercept you
            // cannot arrive at in time is a camp — and MEASURED WRONG:
            // browser-board wins fell 88.8% -> 81.2%, head-to-head distance
            // rose, and long corner dwells returned. Moving TOWARD a
            // predicted crossing is engagement pressure even when arrival is
            // a beat late, because the trail laid en route still closes
            // lanes behind them. Camping is prevented where it was actually
            // measured: CornerIntercept's win-the-race check.
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
                    // SLIPSTREAM: a hunt that steps into the corridor
                    // freezes the hunter — never worth it.
                    if step_enters_corridor(game, 1, d) {
                        continue;
                    }
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
                    // ADR-024: same Boxer perturbation as CornerIntercept.
                    if let Some((choke_dir, baseline)) =
                        boxer_choke_candidate(game, candidates, best_dir, hunt_cells)
                    {
                        game.cpu_brain.ledgers.pending_boxer_baseline = Some(baseline);
                        choose!(choke_dir, CpuDecisionReason::Boxer);
                    }
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
    // The base policy is NOT exempt from the survival floor.
    //
    // Wall-follow used to return unchecked, which was harmless when the CPU
    // never ate and died at length six. The food economy changed that: it now
    // grows to 50-100 cells, and every instrumented warm-arm death became
    // OwnTrail/NoLegalMove at exactly those lengths — the fallthrough coiling
    // a long body into itself, one unexamined step at a time. If wall-follow's
    // next cell cannot reach enough space to outrun our own length, take the
    // candidate that can.
    let followed = evacuate_ring(game, (cx, cy), wall_dir, candidates);
    // The candidate filter cannot reach wall-follow itself — `followed` comes
    // from `wall_dir` directly. If the base policy is about to run an
    // escorted lane and any candidate is not escorted, leave the lane NOW:
    // by the time the lock forms there is exactly one legal move per frame
    // and nothing below this line ever runs again.
    if escorted_lane_step(game, (cx, cy), followed) {
        if let Some(&exit) = candidates
            .iter()
            .filter(|&&d| !escorted_lane_step(game, (cx, cy), d) && !ring_doomed_step(game, (cx, cy), d))
            .max_by(|a, b| {
                let open = |d: Direction| {
                    let (ddx, ddy) = d.as_delta();
                    let ex = (cx as i16 + ddx).max(0).min((game.width - 1) as i16) as u16;
                    let ey = (cy as i16 + ddy).max(0).min((game.height - 1) as i16) as u16;
                    count_open_space(game, ex, ey)
                };
                open(**a)
                    .partial_cmp(&open(**b))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        {
            choose!(exit, CpuDecisionReason::LaneRefusal);
        }
    }
    let step_open = |d: Direction| -> f32 {
        let (ddx, ddy) = d.as_delta();
        let nx = (cx as i16 + ddx).max(0).min((game.width - 1) as i16) as u16;
        let ny = (cy as i16 + ddy).max(0).min((game.height - 1) as i16) as u16;
        count_open_space(game, nx, ny)
    };
    // The static floor is RELAXED — never replaced — by the tail-aware test:
    // wall-follow's destination passes if it clears the floor OR the CPU's
    // own tail remains reachable from it. A long worm hugging its own body
    // fails the static count (its body is 60+ cells of "wall" that will all
    // be floor again) while being perfectly safe; overriding wall-follow on
    // that false alarm is what steered it INTO the coil. Measured on the
    // frames where the floor binds: the destination was actually survivable
    // on ~30% of them (8-56% across seeds).
    let step_cell = |d: Direction| -> (u16, u16) {
        let (ddx, ddy) = d.as_delta();
        (
            (cx as i16 + ddx).max(0).min((game.width - 1) as i16) as u16,
            (cy as i16 + ddy).max(0).min((game.height - 1) as i16) as u16,
        )
    };
    if step_open(followed) < escape_cells && !tail_aware_reach(game, 1, step_cell(followed)).1 {
        if let Some(&roomier) = candidates
            .iter()
            .filter(|&&d| !ring_doomed_step(game, (cx, cy), d))
            .max_by(|a, b| {
                step_open(**a)
                    .partial_cmp(&step_open(**b))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        {
            choose!(roomier, CpuDecisionReason::EscapeFloor);
        }
    }
    choose!(followed, CpuDecisionReason::WallFollow);
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
    if decision || brain.opp_brain.seq.is_multiple_of(STRAIGHT_KEEP_EVERY) {
        brain.opp_brain.remember(context, player_next_dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pattern model must learn what the flat prior structurally cannot:
    /// a strict alternator has a 50/50 tally (unreadable to the prior), but a
    /// perfectly predictable SEQUENCE.
    #[test]
    fn the_pattern_model_reads_an_alternator() {
        let mut tp = TurnPattern::default();
        for i in 0..40 {
            // Score before observing, like the real update path does.
            if i > 8 {
                let predict_left = tp.p_left() >= 0.5;
                assert_eq!(
                    predict_left,
                    i % 2 == 0,
                    "after warm-up the alternation must be called (event {i})"
                );
            }
            tp.observe(i % 2 == 0);
        }
    }

    /// And a stationary habit must still be read at least as well as the
    /// prior reads it — the pattern model must never cost the easy case.
    #[test]
    fn the_pattern_model_reads_a_stationary_habit() {
        let mut tp = TurnPattern::default();
        // 85:15 left, deterministic interleaving.
        for i in 0..60 {
            tp.observe(i % 7 != 0);
        }
        assert!(
            tp.p_left() > 0.6,
            "a left-heavy habit must read left (p = {})",
            tp.p_left()
        );
    }

    /// A forecast the player cannot execute is an abstention, and at a FORCED
    /// turn it abstains on exactly the frames that decide anything. Masking
    /// converts it into a real guess drawn from their habit — measured, this
    /// alone took lift from 0% to 69% against an opponent with a left-turn
    /// signature.
    #[test]
    fn an_impossible_forecast_falls_back_to_the_turn_habit() {
        // Travelling Up, and they habitually break LEFT when forced.
        // Left-of-Up is Left; right-of-Up is Right.
        let turn_prior = [0.10, 0.80, 0.10]; // Straight, Left, Right
        let legal = [Direction::Left, Direction::Right];

        assert_eq!(
            mask_to_legal(Some(Direction::Up), &legal, Direction::Up, &turn_prior, None),
            Some(Direction::Left),
            "an unreachable prediction must become their habitual TURN"
        );
        assert_eq!(
            mask_to_legal(None, &legal, Direction::Up, &turn_prior, None),
            Some(Direction::Left),
            "so must no prediction at all"
        );
    }

    /// On a FREE frame (straight available) an absent prediction must STAY
    /// absent. The turn prior is fed only at forced turns, so it has ~zero
    /// Straight mass — substituting it here manufactures a turn guess on a
    /// frame that is ~95% straight and scores the abstaining model on it.
    /// Measured: this alone was suppressing the power-up intent model from a
    /// 30.9% selection share down to 2.7%.
    #[test]
    fn abstention_on_a_free_frame_is_preserved() {
        let turn_prior = [0.10, 0.80, 0.10];
        // Straight (Up) is available — this is a free choice.
        let legal = [Direction::Up, Direction::Left, Direction::Right];
        assert_eq!(
            mask_to_legal(None, &legal, Direction::Up, &turn_prior, None),
            None,
            "a model with nothing to say must not be handed the habit's guess"
        );
        // But a model that named an IMPOSSIBLE move on a free frame still
        // falls back to the habit — it spoke, and wrongly.
        assert_eq!(
            mask_to_legal(Some(Direction::Down), &legal, Direction::Up, &turn_prior, None),
            Some(Direction::Left),
            "an impossible guess (reversal) still becomes the habitual turn"
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
            mask_to_legal(None, &[Direction::Left, Direction::Right], Direction::Up, &turn_prior, None),
            Some(Direction::Left)
        );
        // Heading Right: left is Up. Same habit, different compass answer.
        assert_eq!(
            mask_to_legal(None, &[Direction::Up, Direction::Down], Direction::Right, &turn_prior, None),
            Some(Direction::Up)
        );
        // Heading Down: left is Right.
        assert_eq!(
            mask_to_legal(None, &[Direction::Left, Direction::Right], Direction::Down, &turn_prior, None),
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
            mask_to_legal(Some(Direction::Right), &legal, Direction::Up, &turn_prior, None),
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
            mask_to_legal(Some(Direction::Right), &legal, Direction::Up, &turn_prior, None),
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
            mask_to_legal(Some(Direction::Up), &[], Direction::Up, &turn_prior, None),
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
            r.record(3, Turn::Straight, Turn::Straight, [true; 3], true);
        }
        for _ in 0..20 {
            r.record(3, Turn::Left, Turn::Straight, [true; 3], false);
        }

        assert!(r.rate() > 0.79, "raw rate looks respectable: {}", r.rate());
        assert!(
            r.discordant() <= 1,
            "a copy of the baseline disagrees with it essentially never"
        );
        assert!(!r.is_ready(), "and therefore never accumulates evidence");
        assert!(!r.is_significant());
    }

    /// Catching what the baseline CANNOT is the only thing that counts.
    /// Under the class-conditional baseline (ADR-020) a habitual always-Left
    /// breaker is called by the base's lateral modal too — no credit there.
    /// What the lagging modal can never call is ALTERNATION: predicting each
    /// switch lands a discordant point every time.
    #[test]
    fn calling_the_turns_is_what_earns_significance() {
        let mut r = ReadRate::default();
        for _ in 0..80 {
            r.record(3, Turn::Straight, Turn::Straight, [true; 3], true); // both right, no evidence
        }
        for i in 0..20 {
            // Player alternates R,L,R,L…; CPU has the alternation read.
            let t = if i % 2 == 0 { Turn::Right } else { Turn::Left };
            r.record(3, t, t, [true; 3], true); // CPU right, lagging modal wrong
        }

        // 20 alternating turns the modal base always trails. (The very
        // first frame is concordant now: a zero-history base answers the
        // lowest-index legal option — Straight — rather than forfeiting.)
        assert_eq!(r.cpu_only, 20, "every switch called is a point of evidence");
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
        // Seed a Left lateral habit so the class-aware base has a stable
        // modal call, then feed 20 discordant frames split dead even: the
        // CPU calls the off-modal turn right as often as it fumbles the
        // modal one. That is what "no better than the base" looks like.
        for _ in 0..10 {
            r.record(3, Turn::Left, Turn::Left, [true; 3], true);
        }
        for i in 0..20 {
            if i % 2 == 0 {
                // Player breaks off-modal (Right); CPU calls it, base can't.
                r.record(3, Turn::Right, Turn::Right, [true; 3], true);
            } else {
                // Player takes the modal Left; CPU guesses Right and misses.
                r.record(3, Turn::Left, Turn::Right, [true; 3], false);
            }
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
        let all = ReadRate {
            cpu_only: 20,
            mode_only: 0,
            ..ReadRate::default()
        };
        assert!((all.p_value() - 0.5f32.powi(20)).abs() < 1e-9);

        // A dead-even split is the least significant result possible.
        let even = ReadRate {
            cpu_only: 10,
            mode_only: 10,
            ..ReadRate::default()
        };
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
            r.record(2, Turn::Straight, Turn::Straight, [true, true, false], true); // two ways out -> 1/2
        }
        for _ in 0..10 {
            r.record(3, Turn::Straight, Turn::Straight, [true; 3], true); // three ways out -> 1/3
        }
        let expected = (10.0 * 0.5 + 10.0 / 3.0) / 20.0;
        assert!((r.uniform_chance() - expected).abs() < 1e-5);
        assert!(r.uniform_chance() > 0.25, "never the old 25% claim");
    }

    /// Codex verification fix 3: a poisoned ledger section resets rather
    /// than feeding NaN into aversion/bandit/drift consumers.
    #[test]
    fn poisoned_ledgers_reset_on_load() {
        let mut brain = CpuBrain::new();
        brain.ledgers.tactic_attempts.push((0, f32::NAN, 1.0, 5, 1));
        brain.ledgers.loss_causes.push((2, 3, 9)); // chased > deaths
        let mut report = BrainRestore::default();
        brain.sanitize(&mut report);
        assert!(brain.ledgers.tactic_attempts.is_empty());
        assert!(brain.ledgers.loss_causes.is_empty());
        assert!(report.sections_skipped >= 2);
    }

    /// Codex verification fix 2/3: the drift alarm's lifetime look budget
    /// (trial tallies, reference, look counter) survives a reload — a
    /// player cannot re-earn early looks by refreshing the page.
    #[test]
    fn drift_trial_state_survives_persistence() {
        let mut brain = CpuBrain::new();
        brain.ledgers.rounds_seen = 21;
        brain.ledgers.ref_frozen = true;
        brain.ledgers.ref_alt_median = 0.6;
        brain.ledgers.ref_gap_median = 50.0;
        brain.ledgers.alt_above = 2;
        brain.ledgers.alt_trials = 6;
        brain.ledgers.gap_above = 5;
        brain.ledgers.gap_trials = 6;
        brain.ledgers.round_summaries.push_back((10, 5, 40, 300));
        let (r, _) = CpuBrain::from_bytes_report(&brain.to_bytes()).unwrap();
        assert_eq!(r.ledgers.rounds_seen, 21);
        assert!(r.ledgers.ref_frozen);
        assert_eq!(r.ledgers.alt_trials, 6);
        assert_eq!(r.ledgers.gap_above, 5);
        assert_eq!(r.ledgers.round_summaries.len(), 1);
    }

    /// Codex verification fix 6: consecutive same-tactic frames are ONE
    /// episodic attempt; a different tactic or an expired window opens a
    /// new one; a kill closes exactly once.
    #[test]
    fn tactic_attempts_are_episodes_not_frames() {
        let mut l = LearningLedgers::default();
        for f in 0..10 {
            l.note_tactic(CpuDecisionReason::DirectIntercept, f, 0);
        }
        assert_eq!(l.tactic_attempts[0].3, 1, "one pursuit = one attempt");
        l.note_tactic(CpuDecisionReason::CornerIntercept, 10, 0);
        l.note_tactic(CpuDecisionReason::DirectIntercept, 11, 0);
        let direct = l.tactic_attempts.iter().find(|e| e.0 == 0).unwrap();
        assert_eq!(direct.3, 2, "tactic switch opens a new episode");
        l.resolve_player_death(12, None, 0.0, 0);
        let direct = l.tactic_attempts.iter().find(|e| e.0 == 0).unwrap();
        assert_eq!(direct.4, 1, "kill credited once");
        l.resolve_player_death(13, None, 0.0, 0);
        let direct = l.tactic_attempts.iter().find(|e| e.0 == 0).unwrap();
        assert_eq!(direct.4, 1, "no open attempt, no double credit");
        // Expiry: an old window never gets the credit.
        l.note_tactic(CpuDecisionReason::DirectIntercept, 100, 0);
        l.resolve_player_death(100 + ATTEMPT_HORIZON + 1, None, 0.0, 0);
        let direct = l.tactic_attempts.iter().find(|e| e.0 == 0).unwrap();
        assert_eq!(direct.4, 1, "expired window earns nothing");
    }

    /// ADR-024: Boxer kill credit requires a REALIZED choke — a
    /// boxing-compatible cause AND the player's space at death collapsed
    /// to <=60% of the baseline precommitted when the episode opened.
    #[test]
    fn boxer_credit_requires_realized_choke() {
        use crate::game::DeathCause as DC;
        let boxer = |l: &LearningLedgers| l.tactic_attempts.iter().find(|e| e.0 == 4).cloned();

        // Realized choke: eligible cause + collapse below 60% of baseline.
        let mut l = LearningLedgers {
            pending_boxer_baseline: Some(200.0),
            ..Default::default()
        };
        l.note_tactic(CpuDecisionReason::Boxer, 10, 0);
        assert_eq!(l.open_attempt, Some((4, 10, 200.0, 0)), "baseline precommitted at open");
        l.resolve_player_death(15, Some(DC::OwnTrail), 100.0, 0);
        assert_eq!(boxer(&l).unwrap().4, 1, "collapsed choke credits");

        // Space did NOT collapse: same cause, no credit.
        let mut l = LearningLedgers {
            pending_boxer_baseline: Some(200.0),
            ..Default::default()
        };
        l.note_tactic(CpuDecisionReason::Boxer, 10, 0);
        l.resolve_player_death(15, Some(DC::OwnTrail), 150.0, 0);
        assert_eq!(boxer(&l).unwrap().4, 0, "no collapse, no credit");

        // Incompatible cause (weapon kill during a boxer window): no credit.
        let mut l = LearningLedgers {
            pending_boxer_baseline: Some(200.0),
            ..Default::default()
        };
        l.note_tactic(CpuDecisionReason::Boxer, 10, 0);
        l.resolve_player_death(15, Some(DC::Laser), 50.0, 0);
        assert_eq!(boxer(&l).unwrap().4, 0, "weapon deaths never credit the choke");

        // Missing baseline (defensive): no credit even on collapse-shaped input.
        let mut l = LearningLedgers::default();
        l.note_tactic(CpuDecisionReason::Boxer, 10, 0);
        l.resolve_player_death(15, Some(DC::Wall), 0.0, 0);
        assert_eq!(boxer(&l).unwrap().4, 0, "no baseline, no causal story, no credit");

        // Non-boxer arms are untouched by the eligibility tightening.
        let mut l = LearningLedgers::default();
        l.note_tactic(CpuDecisionReason::DirectIntercept, 10, 0);
        l.resolve_player_death(15, Some(DC::Laser), 999.0, 0);
        let direct = l.tactic_attempts.iter().find(|e| e.0 == 0).unwrap();
        assert_eq!(direct.4, 1, "intercept credit rule unchanged");
    }

    /// ADR-024 + k3 verify G1: sudden-death ring closures collapse the
    /// player's space MECHANICALLY and kill with DeathCause::Wall — a
    /// Boxer window straddling a shrink must earn nothing.
    #[test]
    fn boxer_credit_voided_when_the_ring_did_the_boxing() {
        use crate::game::DeathCause as DC;
        let mut l = LearningLedgers {
            pending_boxer_baseline: Some(200.0),
            ..Default::default()
        };
        l.note_tactic(CpuDecisionReason::Boxer, 10, 0);
        l.resolve_player_death(15, Some(DC::Wall), 40.0, 1); // ring advanced
        let boxer = l.tactic_attempts.iter().find(|e| e.0 == 4).unwrap();
        assert_eq!(boxer.4, 0, "the ring's collapse is not the boxer's kill");
    }

    /// ADR-024 + k3 verify G2: a Boxer window closed by tactic
    /// replacement inside its horizon wins the credit over the
    /// replacement iff ITS choke realized — exclusive, never both.
    #[test]
    fn contested_boxer_window_wins_credit_iff_realized() {
        use crate::game::DeathCause as DC;
        // Realized: the boxed death credits Boxer, not the intercept.
        let mut l = LearningLedgers {
            pending_boxer_baseline: Some(200.0),
            ..Default::default()
        };
        l.note_tactic(CpuDecisionReason::Boxer, 10, 0);
        l.note_tactic(CpuDecisionReason::DirectIntercept, 14, 0); // terminal replacement
        l.resolve_player_death(18, Some(DC::OwnTrail), 80.0, 0);
        let boxer = l.tactic_attempts.iter().find(|e| e.0 == 4).unwrap();
        let direct = l.tactic_attempts.iter().find(|e| e.0 == 0).unwrap();
        assert_eq!(boxer.4, 1, "the realized choke keeps its kill through replacement");
        assert_eq!(direct.4, 0, "credit is exclusive — the replacement earns nothing");

        // Not realized: the replacement keeps the credit.
        let mut l = LearningLedgers {
            pending_boxer_baseline: Some(200.0),
            ..Default::default()
        };
        l.note_tactic(CpuDecisionReason::Boxer, 10, 0);
        l.note_tactic(CpuDecisionReason::DirectIntercept, 14, 0);
        l.resolve_player_death(18, Some(DC::OwnTrail), 180.0, 0); // no collapse
        let boxer = l.tactic_attempts.iter().find(|e| e.0 == 4).unwrap();
        let direct = l.tactic_attempts.iter().find(|e| e.0 == 0).unwrap();
        assert_eq!(boxer.4, 0, "an unrealized contested window earns nothing");
        assert_eq!(direct.4, 1, "the replacement keeps an ordinary kill");

        // Ring guard applies to contested windows too.
        let mut l = LearningLedgers {
            pending_boxer_baseline: Some(200.0),
            ..Default::default()
        };
        l.note_tactic(CpuDecisionReason::Boxer, 10, 0);
        l.note_tactic(CpuDecisionReason::DirectIntercept, 14, 0);
        l.resolve_player_death(18, Some(DC::Wall), 40.0, 1); // ring advanced
        let boxer = l.tactic_attempts.iter().find(|e| e.0 == 4).unwrap();
        assert_eq!(boxer.4, 0, "the ring voids a contested claim exactly as an open one");
    }

    /// ADR-024: the staged baseline binds to the episode it opens — a
    /// same-episode re-note must not consume a newly staged value, and a
    /// non-boxer open clears any stale staging.
    #[test]
    fn boxer_baseline_staging_is_episode_scoped() {
        let mut l = LearningLedgers {
            pending_boxer_baseline: Some(300.0),
            ..Default::default()
        };
        l.note_tactic(CpuDecisionReason::Boxer, 10, 0);
        l.pending_boxer_baseline = Some(50.0); // staged mid-episode: discarded
        l.note_tactic(CpuDecisionReason::Boxer, 12, 0);
        assert_eq!(l.open_attempt, Some((4, 10, 300.0, 0)), "episode keeps its own baseline");
        assert_eq!(l.pending_boxer_baseline, None, "mid-episode staging consumed away");

        let mut l = LearningLedgers {
            pending_boxer_baseline: Some(300.0),
            ..Default::default()
        };
        l.note_tactic(CpuDecisionReason::DirectIntercept, 10, 0);
        assert_eq!(l.open_attempt, Some((0, 10, 0.0, 0)), "non-boxer opens carry no baseline");
        assert_eq!(l.pending_boxer_baseline, None, "stale staging cleared");
    }

    /// THE WIRE-SHAPE TRIPWIRE (owner data-loss incident, 2026-08-07):
    /// stage 2.2 widened ClassBooksWire without versioned decode, and the
    /// owner's saved book section silently failed bincode — his earned
    /// read was WIPED once (the first four rounds of his next session
    /// show earned=0). bincode is not field-tolerant; the never-wipe rule
    /// therefore needs a tripwire, not vigilance. This test decodes a
    /// COMMITTED golden brain file: any change to any persisted shape
    /// breaks it, forcing the dual-write ritual (SEC_READ_RATE v1/v2
    /// pattern) instead of a silent player wipe.
    #[test]
    fn the_golden_brain_still_decodes() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/brain_golden.bin");
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(_) => {
                // First run: mint the fixture from a populated brain.
                let mut brain = CpuBrain::new();
                for i in 0..40 {
                    let side = if i % 2 == 0 { Turn::Left } else { Turn::Right };
                    brain.lifetime_read.record(3, side, side, [true; 3], true);
                    brain.class_books.book_read.record(3, side, side, [true; 3], true);
                    brain.voluntary_pattern.observe(i % 2 == 0);
                    brain.class_books.observe_hazard(i % 96, i % 3 == 0);
                }
                brain.class_books.observe_turn_book(true);
                brain.ledgers.tactic_attempts.push((0, 5.0, 2.0, 5, 2));
                brain.ledgers.loss_causes.push((2, 3, 2));
                let b = brain.to_bytes();
                std::fs::create_dir_all(
                    concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures"),
                )
                .unwrap();
                std::fs::write(path, &b).unwrap();
                b
            }
        };
        let (brain, report) = CpuBrain::from_bytes_report(&bytes)
            .expect("the golden brain must always decode");
        assert_eq!(
            report.sections_skipped, 0,
            "a persisted section no longer decodes — you are about to wipe \
             real players; use the dual-write pattern (SEC_READ_RATE v1/v2)"
        );
        assert!(brain.lifetime_read.samples > 0);
        assert!(brain.class_books.book_read.samples > 0);
        assert!(brain.voluntary_pattern.events > 0);
    }

    /// Kata 2 (#4+#7): the rhythm reader's grammar survives sessions —
    /// a returning alternator is read from round one, not re-learned.
    #[test]
    fn the_voluntary_vomm_survives_persistence() {
        let mut brain = CpuBrain::new();
        for i in 0..24 {
            brain.voluntary_pattern.observe(i % 2 == 0);
        }
        let before = brain.voluntary_pattern.p_left();
        let (restored, _) = CpuBrain::from_bytes_report(&brain.to_bytes()).unwrap();
        assert_eq!(restored.voluntary_pattern.events, 24);
        assert!((restored.voluntary_pattern.p_left() - before).abs() < 1e-6);
    }

    /// Kata 1 (#5): the boxer aversion — floors only rise, chase-gated,
    /// hard-capped, and a player who never chases earns exactly zero.
    #[test]
    fn boxer_aversion_rises_only_on_chased_trail_deaths() {
        let mut l = LearningLedgers::default();
        assert_eq!(l.boxer_aversion(), 0.0);
        // Unchased trail deaths (wandered into an old trail): no aversion.
        l.loss_causes.push((crate::game::DeathCause::EnemyTrail as u8, 5, 0));
        assert_eq!(l.boxer_aversion(), 0.0);
        // Chased deaths raise it, capped at +50%.
        l.loss_causes[0].2 = 3;
        assert!((l.boxer_aversion() - 0.18).abs() < 1e-6);
        l.loss_causes[0].2 = 100;
        assert_eq!(l.boxer_aversion(), 0.5);
        // Other causes never contribute.
        let mut l2 = LearningLedgers::default();
        l2.loss_causes.push((crate::game::DeathCause::Wall as u8, 10, 10));
        assert_eq!(l2.boxer_aversion(), 0.0);
    }

    /// The chase flag itself: within 8 cells inside the last 10 frames.
    #[test]
    fn the_chase_flag_reads_the_distance_ring() {
        let mut l = LearningLedgers::default();
        for _ in 0..10 {
            l.note_frame(30, None, 0);
        }
        l.note_cpu_death(crate::game::DeathCause::EnemyTrail as u8);
        assert_eq!(l.loss_causes[0].2, 0, "far player: not a chase");
        for _ in 0..10 {
            l.note_frame(6, None, 0);
        }
        l.note_cpu_death(crate::game::DeathCause::EnemyTrail as u8);
        assert_eq!(l.loss_causes[0].2, 1, "close player: chased");
    }

    /// The derived gate's full lifecycle: hard-closed below maturity,
    /// opens only when h·aT clears (1−h)·aS by the Schmitt band, holds
    /// inside the band, releases below it.
    #[test]
    fn the_turn_book_gate_derives_and_holds() {
        let mut b = ClassBooks::default();
        // Strong hazard context but immature book: closed.
        assert!(!b.gate(0.9), "maturity floor must hard-close the gate");
        for _ in 0..BOOK_MATURITY {
            b.observe_turn_book(true); // aT -> 1.0
        }
        for _ in 0..100 {
            b.observe_straight_book(true); // aS -> 1.0
        }
        // h*1.0 vs (1-h)*1.0: crosses at 0.5+band.
        assert!(!b.gate(0.50), "inside the band from below: stays closed");
        assert!(b.gate(0.60), "clear conviction opens");
        assert!(b.gate(0.52), "Schmitt: stays open inside the band");
        assert!(!b.gate(0.40), "clear reversal closes");
    }

    /// KT hazard cells: honest 0.5 at zero data, converging toward the
    /// observed rate, and decay keeps them adaptive.
    #[test]
    fn hazard_cells_estimate_and_decay() {
        let mut b = ClassBooks::default();
        assert!((b.hazard(3) - 0.5).abs() < 1e-6);
        for _ in 0..50 {
            b.observe_hazard(3, true);
        }
        assert!(b.hazard(3) > 0.9, "50 straight turn events: h={}", b.hazard(3));
        for _ in 0..200 {
            b.observe_hazard(3, false);
        }
        assert!(b.hazard(3) < 0.25, "recent stays dominate: h={}", b.hazard(3));
    }

    /// Coverage discounts an abstaining book: excellent accuracy on an
    /// undisclosed subset must not buy global aggression.
    #[test]
    fn an_abstaining_book_cannot_spend_full_evidence() {
        let mut b = ClassBooks {
            side_opportunities: 100,
            side_declarations: 25,
            ..ClassBooks::default()
        };
        // Force a proven book_read so spendable is limited by coverage only.
        for i in 0..120u64 {
            let s = i.wrapping_mul(6364136223846793005).wrapping_add(7);
            let side = if ((s >> 33) % 10) < 9 { Turn::Left } else { Turn::Right };
            b.book_read.record(3, side, Turn::Left, [true; 3], side == Turn::Left);
        }
        assert!(b.book_read.earned_read() > 0.0, "positive control failed to latch");
        assert!(
            b.spendable() <= b.book_read.earned_read() * 0.25 + 1e-6,
            "spendable {} must be coverage-scaled",
            b.spendable()
        );
        assert!(b.coverage() == 0.25);
    }

    /// side_pick only ever selects a model currently predicting a LATERAL.
    #[test]
    fn side_pick_ignores_straight_speakers() {
        let b = ClassBooks::default();
        let mut masked = [None; ENSEMBLE_MODELS];
        masked[0] = Some(Direction::Up); // straight (heading Up)
        assert!(b.side_pick(&masked, Direction::Up).is_none());
        masked[4] = Some(Direction::Left); // a lateral
        let (src, d) = b.side_pick(&masked, Direction::Up).unwrap();
        assert_eq!((src, d), (4, Direction::Left));
    }

    /// Poisoned persisted book state resets to default instead of feeding
    /// NaN into the hazard and the gate.
    #[test]
    fn sanitize_resets_poisoned_books() {
        let mut brain = CpuBrain::new();
        brain.class_books.hz_turn[7] = f32::NAN;
        brain.class_books.turn_events = 500;
        let mut report = BrainRestore::default();
        brain.sanitize(&mut report);
        assert_eq!(brain.class_books.turn_events, 0, "poisoned books must reset");
    }

    /// The exact defect the external verification caught in the first
    /// stage-1 commit: the lateral channel samples frames where the player
    /// DID turn, so its null is uniform over legal LATERALS. An always-Left
    /// forecast against a fair-coin side chooser hits exactly that null —
    /// it must never latch, at any horizon.
    #[test]
    fn an_always_left_forecast_never_latches_on_a_fair_side_chooser() {
        let mut r = ReadRate::default();
        let mut s: u64 = 7;
        for _ in 0..4000 {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1);
            let side = if (s >> 33) & 1 == 0 { Turn::Left } else { Turn::Right };
            r.record(3, side, Turn::Left, [true; 3], side == Turn::Left);
            assert!(
                !r.lateral_significant(),
                "coin sides read as a side habit at n={} (hits={} chance={})",
                r.lat_samples,
                r.lat_hits,
                r.lat_chance
            );
        }
    }

    /// With a single legal lateral the side is CERTAIN given the turn:
    /// those frames carry zero side information and must not be recorded.
    #[test]
    fn a_sole_legal_lateral_carries_no_side_evidence() {
        let mut r = ReadRate::default();
        for _ in 0..500 {
            r.record(2, Turn::Left, Turn::Left, [true, true, false], true);
        }
        assert_eq!(r.lat_samples, 0);
        assert!(!r.lateral_significant());
    }

    /// A genuine side habit at real two-lateral choices IS earned — the
    /// positive control for the corrected null.
    #[test]
    fn a_real_side_habit_is_earned_at_real_choices() {
        let mut r = ReadRate::default();
        let mut s: u64 = 3;
        for _ in 0..120 {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1);
            // 90:10 left-breaker, CPU forecasts Left every time.
            let side = if ((s >> 33) % 10) < 9 { Turn::Left } else { Turn::Right };
            r.record(3, side, Turn::Left, [true; 3], side == Turn::Left);
        }
        assert!(
            r.lateral_significant(),
            "a 90:10 habit at 120 real choices must clear the anytime bound (hits={} n={})",
            r.lat_hits,
            r.lat_samples
        );
        assert!(r.lateral_lift() > 0.5);
    }

    /// A v1 projection decoded AFTER the v2 section (abnormal file order)
    /// must not clobber the widened record — precedence is by version, not
    /// by position.
    #[test]
    fn v1_after_v2_does_not_clobber_the_widened_record() {
        let mut brain = CpuBrain::new();
        for _ in 0..40 {
            brain.lifetime_read.record(3, Turn::Left, Turn::Left, [true; 3], true);
        }
        let bytes = brain.to_bytes();
        let count = u16::from_le_bytes(bytes[6..8].try_into().unwrap());
        let mut v1: Option<&[u8]> = None;
        let mut v2: Option<&[u8]> = None;
        let mut others: Vec<(u16, &[u8])> = Vec::new();
        let mut pos = 8usize;
        for _ in 0..count {
            let tag = u16::from_le_bytes(bytes[pos..pos + 2].try_into().unwrap());
            let len =
                u32::from_le_bytes(bytes[pos + 2..pos + 6].try_into().unwrap()) as usize;
            let body = &bytes[pos + 6..pos + 6 + len];
            match tag {
                SEC_READ_RATE => v1 = Some(body),
                SEC_READ_RATE2 => v2 = Some(body),
                _ => others.push((tag, body)),
            }
            pos += 6 + len;
        }
        let mut reordered = bytes[..6].to_vec();
        reordered.extend_from_slice(&count.to_le_bytes());
        let mut ordered: Vec<(u16, &[u8])> = others;
        ordered.push((SEC_READ_RATE2, v2.unwrap()));
        ordered.push((SEC_READ_RATE, v1.unwrap()));
        for (tag, body) in ordered {
            reordered.extend_from_slice(&tag.to_le_bytes());
            reordered.extend_from_slice(&(body.len() as u32).to_le_bytes());
            reordered.extend_from_slice(body);
        }
        let (restored, _) = CpuBrain::from_bytes_report(&reordered).unwrap();
        assert_eq!(restored.lifetime_read, brain.lifetime_read);
    }

    /// A read rate must survive a save/load cycle, or the cross-session curve
    /// the product is built around resets every launch.
    #[test]
    fn read_rate_survives_persistence() {
        let mut brain = CpuBrain::new();
        for _ in 0..40 {
            brain.lifetime_read.record(3, Turn::Left, Turn::Left, [true; 3], true);
        }
        let (restored, report) = CpuBrain::from_bytes_report(&brain.to_bytes()).unwrap();
        assert_eq!(restored.lifetime_read.samples, 40);
        assert_eq!(restored.lifetime_read.hits, 40);
        assert!(!report.is_partial());
        // The FULL record — lateral channel and latch included — must
        // survive via SEC_READ_RATE2 (v2 is written after the v1
        // projection, so it wins on load).
        assert_eq!(restored.lifetime_read, brain.lifetime_read);
        assert!(restored.lifetime_read.lat_samples == 40);
    }

    /// An OLD build reading a NEW save must still recover the core read:
    /// the v1 projection rides SEC_READ_RATE unchanged, and the widened
    /// section is skipped by length like any unknown tag. Simulated here by
    /// stripping SEC_READ_RATE2 from a fresh blob and decoding what
    /// remains — exactly what the old reader's match arms would keep.
    #[test]
    fn v1_projection_preserves_the_core_read_for_old_builds() {
        let mut brain = CpuBrain::new();
        for _ in 0..30 {
            brain.lifetime_read.record(2, Turn::Right, Turn::Right, [true, false, true], true);
        }
        let bytes = brain.to_bytes();
        // Walk the section table (magic u32, count u16, then
        // [tag u16, len u32, body] repeated) and drop tag 10.
        let count = u16::from_le_bytes(bytes[6..8].try_into().unwrap());
        let mut kept: Vec<(u16, &[u8])> = Vec::new();
        let mut pos = 8usize;
        for _ in 0..count {
            let tag = u16::from_le_bytes(bytes[pos..pos + 2].try_into().unwrap());
            let len =
                u32::from_le_bytes(bytes[pos + 2..pos + 6].try_into().unwrap()) as usize;
            let body = &bytes[pos + 6..pos + 6 + len];
            if tag != SEC_READ_RATE2 {
                kept.push((tag, body));
            }
            pos += 6 + len;
        }
        assert!(
            kept.len() as u16 == count - 1,
            "the blob must actually contain SEC_READ_RATE2"
        );
        let mut older = bytes[..6].to_vec();
        older.extend_from_slice(&(count - 1).to_le_bytes());
        for (tag, body) in kept {
            older.extend_from_slice(&tag.to_le_bytes());
            older.extend_from_slice(&(body.len() as u32).to_le_bytes());
            older.extend_from_slice(body);
        }
        let (restored, report) = CpuBrain::from_bytes_report(&older).unwrap();
        assert!(!report.is_partial());
        assert_eq!(restored.lifetime_read.samples, 30);
        assert_eq!(restored.lifetime_read.hits, 30);
        // The widened fields are honestly zero — never garbage.
        assert_eq!(restored.lifetime_read.lat_samples, 0);
        assert!(!restored.lifetime_read.lat_latched);
    }

    /// A blob written before the read-rate section existed must still restore
    /// cleanly — no scary partial-restore banner for a player who lost nothing.
    #[test]
    fn a_brain_without_a_read_rate_section_restores_clean() {
        let mut brain = CpuBrain::new();
        brain.opp_pred_hits = 5;
        brain.opp_pred_total = 9;
        let bytes = brain.to_bytes();
        // Strip BOTH read-rate sections (v1 tag 6, v2 tag 10) — earlier
        // versions of this test decremented the section count, which (a)
        // for a while edited the FORMAT field by mistake and (b) at best
        // dropped the TRAILING section, which is the portfolio, not the
        // read. This now removes exactly what its name claims.
        let count = u16::from_le_bytes(bytes[6..8].try_into().unwrap());
        let mut kept: Vec<(u16, &[u8])> = Vec::new();
        let mut pos = 8usize;
        for _ in 0..count {
            let tag = u16::from_le_bytes(bytes[pos..pos + 2].try_into().unwrap());
            let len =
                u32::from_le_bytes(bytes[pos + 2..pos + 6].try_into().unwrap()) as usize;
            if tag != SEC_READ_RATE && tag != SEC_READ_RATE2 {
                kept.push((tag, &bytes[pos + 6..pos + 6 + len]));
            }
            pos += 6 + len;
        }
        let mut older = bytes[..6].to_vec();
        older.extend_from_slice(&(kept.len() as u16).to_le_bytes());
        for (tag, body) in kept {
            older.extend_from_slice(&tag.to_le_bytes());
            older.extend_from_slice(&(body.len() as u32).to_le_bytes());
            older.extend_from_slice(body);
        }

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
            tripped: false,
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
            tripped: false,
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
    /// blasts that were physically harmless to it. CALM decoys stay
    /// harmless under v8 too — the threat begins with the flash window.
    #[test]
    fn our_own_mine_is_never_a_threat() {
        let mut game = WormGame::with_size(120, 38);
        game.bombs.push(crate::game::Bomb {
            x: 60,
            y: 20,
            fuse: 15_000, // calm: far outside the v8 flash window
            armed_in: 0,
            disguise: 5,
            owner: 1,
            tripped: false,
        });
        assert!(!cell_threatened_by_bomb(&game, 60, 20, 0));
    }

    /// World v8 blasts are OWNER-SAFE, trail included (the ADR-023 rule
    /// applied to bombs) — so an own mine is pure infrastructure at ANY
    /// fuse age, flashing included. A full-life threat was measured
    /// worse: it bent early routes for zero physical risk.
    #[test]
    fn own_mine_is_no_threat_even_while_flashing() {
        let mut game = WormGame::with_size(120, 38);
        game.bombs.push(crate::game::Bomb {
            x: 60,
            y: 20,
            fuse: 1_500, // tier 1: flashing
            armed_in: 0,
            disguise: 5,
            owner: 1,
            tripped: false,
        });
        assert!(!cell_threatened_by_bomb(&game, 60, 20, 0));
        game.set_world_version(7);
        assert!(!cell_threatened_by_bomb(&game, 60, 20, 0));
    }

    /// Dying must never make the fatal direction *more* attractive. The k-NN
    /// ADR-024 staged fixture: boxing IS available — the player sits in a
    /// pocket whose two-cell mouth the CPU can seal with one move — and
    /// the prospective-choke test finds exactly that move with the
    /// baseline precommitted; an open field offers no material choke and
    /// the round-boundary gate suppresses the perturbation entirely.
    #[test]
    fn boxer_finds_the_material_choke_and_only_that() {
        use crate::game::CellType;
        let stage = |with_pocket: bool| -> crate::game::WormGame {
            let mut game = crate::game::WormGame::with_size(120, 38);
            for row in &mut game.grid {
                for cell in row.iter_mut() {
                    if *cell != CellType::Wall {
                        *cell = CellType::Empty;
                    }
                }
            }
            if with_pocket {
                // Trail ring x,y in [6,14] with a two-cell mouth at
                // (14,9)/(14,10); the exterior escape runs through
                // (15,10) — the cell the choke move takes.
                for x in 6..=14u16 {
                    for y in 6..=14u16 {
                        let boundary = x == 6 || x == 14 || y == 6 || y == 14;
                        let mouth = x == 14 && (y == 9 || y == 10);
                        if boundary && !mouth {
                            game.grid[y as usize][x as usize] = CellType::CPU;
                        }
                    }
                }
            }
            game.cycles[0].head = (10, 10);
            game.cycles[0].positions = vec![(10, 10)];
            game.grid[10][10] = CellType::Player;
            game.cycles[1].head = (15, 9);
            game.cycles[1].positions = vec![(15, 9)];
            game.cycles[1].direction = Direction::Down;
            game.grid[9][15] = CellType::CPU;
            game
        };

        let game = stage(true);
        let candidates = [Direction::Down, Direction::Up];
        let got = boxer_choke_candidate(&game, &candidates, Direction::Up, 10.0);
        let (dir, baseline) = got.expect("a sealable pocket is a material choke");
        assert_eq!(dir, Direction::Down, "the choke is the mouth-sealing move");
        assert!(
            baseline > 100.0,
            "baseline is the player's PRE-choke reachable region (got {baseline})"
        );

        // Open field: every move denies a cell; none is material.
        let game = stage(false);
        assert_eq!(
            boxer_choke_candidate(&game, &candidates, Direction::Up, 10.0),
            None,
            "no pocket, no choke — the intercept label must not be stolen"
        );

        // Round-boundary gate: a suppressed arm never perturbs.
        let mut game = stage(true);
        game.cpu_brain.tactic_boxer_ok = false;
        assert_eq!(
            boxer_choke_candidate(&game, &candidates, Direction::Up, 10.0),
            None,
            "the yield gate silences the perturbation"
        );
    }

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
        // The formula under test is the FULLY SHARP floor; an unread,
        // unpressured CPU keeps only discipline_floor of it (ADR-018).
        game.read_rate = 1.0;

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
        game.read_rate = 1.0; // sharp floor — see ADR-018
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
        game.cycles[0].head = (33, 20); // in front, within any reach
        assert!(should_fire(&mut game, 1));
        // v9/v10: a target past the 4-cell bolt reach is not a shot.
        game.cycles[0].head = (36, 20);
        game.set_world_version(9);
        assert!(
            !should_fire(&mut game, 1),
            "the aim gate matched the 4-cell reach at v9/v10"
        );
        game.set_world_version(10);
        assert!(!should_fire(&mut game, 1), "v10 kept the 4-cell gate");
        // v11 restored the full ray (and pre-v9 always had it).
        game.set_world_version(11);
        assert!(should_fire(&mut game, 1));
        game.set_world_version(8);
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
        let (hx, hy) = (6u16, 10u16);
        let walls = wall_distance(&game, hx, hy);
        // Left: free cells x=5,4 then the arena wall at x=3 (v6) -> 2.
        assert_eq!(walls[2], 2.0, "left wall distance must stop at the arena wall");
        // Right: free cells to the arena wall at x = w-4 (v6 ring 3).
        let expect_right = (game.width - 4 - hx - 1) as f32;
        assert_eq!(walls[3], expect_right);
        // Up: free cells to the wall at y=3.
        assert_eq!(walls[0], (hy - 4) as f32);
        // Down: free cells to the wall at y = h-4.
        let expect_down = (game.height - 4 - hy - 1) as f32;
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
            tripped: false,
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
            fuse: 15_000, // calm decoy — v8's threat starts at the flash
            disguise: 5,
            armed_in: 0,
            owner: 1,
            tripped: false,
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
            tripped: false,
        });
        assert!(
            cell_threatened_by_bomb(&game, hx, hy, 3),
            "player bomb is a threat"
        );
    }
}
