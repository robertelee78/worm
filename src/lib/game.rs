#[cfg(not(target_arch = "wasm32"))]
use crossterm::terminal::size;
use rand::rngs::StdRng;
use rand::SeedableRng;
use std::time::Duration;

pub const FRAME_DELAY_MS: u64 = 150;

#[derive(Clone, Copy, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Direction {
    pub fn as_delta(self) -> (i16, i16) {
        match self {
            Direction::Up => (0, -1),
            Direction::Down => (0, 1),
            Direction::Left => (-1, 0),
            Direction::Right => (1, 0),
        }
    }

    pub fn from_input(byte: u8) -> Option<Self> {
        match byte {
            b'w' | b'W' => Some(Direction::Up),
            b's' | b'S' => Some(Direction::Down),
            b'a' | b'A' => Some(Direction::Left),
            b'd' | b'D' => Some(Direction::Right),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
#[repr(u8)]
pub enum CellType {
    Empty = 0,
    Wall = 1,
    Player = 2,
    CPU = 3,
    Food = 4,
    /// A hole punched through the arena wall — passable, leads to the outer corridor.
    Hole = 5,
    /// A collectible power-up; the kind is looked up in WormGame::powerups (mirrors food).
    PowerUp = 6,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PowerUpKind {
    /// Hitscan beam along facing. Bounces off arena walls (ring 2) to reach
    /// opponents hiding in corridors, detonates bombs in its path, passes
    /// through trails and holes; stops at the outer frame.
    Laser,
    /// Three bolts (straight + two forward diagonals). They fly until they hit
    /// a wall — they never break one — so range is a property of the board
    /// rather than a magic number that meant something different on every
    /// board size.
    TriShot,
    /// Planted at the current cell; detonates after ~3s. Chebyshev radius
    /// A proximity mine: inert while arming, then detonates when an enemy head
    /// enters its trigger ring. The blast is a cross, and chains into other
    /// bombs.
    Bomb,
}

/// A live tri-shot bolt. `from` is the cycle that fired it — bolts can never
/// hit their own firer (the bolt spawns on the head and would otherwise land
/// on the head's next cell the same frame).
#[derive(Clone, Debug)]
pub struct Projectile {
    pub x: u16,
    pub y: u16,
    pub dx: i16,
    pub dy: i16,
    pub steps_left: u8,
    pub from: u8,
}

/// A planted bomb counting down to detonation. `owner` is the cycle that
/// planted it — a bomb never kills its own planter (mirrors `Projectile::from`;
/// laser-detonating your own bomb must not self-kill, the 3s fuse would
/// otherwise be voided).
#[derive(Clone, Debug)]
pub struct Bomb {
    pub x: u16,
    pub y: u16,
    /// Backstop countdown, in FRAMES. The mine's job is proximity; this only
    /// stops stale mines accumulating across a long round. It used to be the
    /// weapon, derived from milliseconds — which meant one item behaved like
    /// three over a round, since the same 3s is 26 moves at the opening tick
    /// and 85 at the speed floor.
    pub fuse: u32,
    /// The food value this mine MASQUERADES as, 1..=9.
    ///
    /// A planted mine is indistinguishable from food — same glyph, same
    /// value-scaled size, same colour — so the only way to avoid one is to
    /// remember where it was planted. That is the mechanic: bait. It also
    /// gives the opponent model something worth learning, because "does this
    /// human take bait?" is a habit.
    pub disguise: u8,
    /// Frames until the proximity trigger goes live. While non-zero the ring
    /// is inert for EVERYONE: the planter gets clear, and the opponent gets a
    /// real dash-through window.
    pub armed_in: u32,
    pub owner: u8,
    /// True once the proximity ring (or a chain blast) has set this mine off.
    /// ONLY a tripped mine detonates. A fuse that simply runs out FIZZLES —
    /// quiet removal, no blast — because the fuse's documented job is
    /// stopping stale mines accumulating, and for months it detonated
    /// instead: a spontaneous 10-cell kill cross, on a timer the player
    /// cannot see, attached to a thing drawn as food. Play-tested verdict:
    /// "randomly killed by bomb blasts that didn't actually happen".
    pub tripped: bool,
}

/// Reach of each blast arm. Unchanged from the old square's radius — a cross
/// is already far less lethal (41 cells against 441), so shortening the arms
/// as well would leave it threatening nothing.
pub const BOMB_RADIUS_CELLS: i16 = 10;
/// Guaranteed-death core, and the ring that triggers the mine. Deliberately
/// the same number, so the rule reads as "the ring that sets it off is the
/// ring that certainly kills you".
pub const BOMB_CORE_RADIUS: i16 = 2;
pub const MINE_TRIGGER_CELLS: i16 = 2;
/// Frames a freshly planted mine stays inert. Long enough for the planter to
/// clear the trigger ring (3 moves), short enough that the dash-through window
/// is tight rather than free.
pub const MINE_ARM_FRAMES: u32 = 8;
/// Failsafe lifetime in FRAMES, so a mine cannot sit on the board forever.
pub const BOMB_FUSE_FRAMES: u32 = 240;

/// Is `(x, y)` inside a blast centred on `(cx, cy)`?
///
/// A CROSS, not a square: the core square plus four axis arms. One predicate,
/// shared by the kill test and BOTH previews — three hand-written copies of a
/// blast shape is how "I never saw that coming" gets back in.
pub fn in_blast(cx: i32, cy: i32, x: i32, y: i32, arm: i32) -> bool {
    let (ax, ay) = ((x - cx).abs(), (y - cy).abs());
    let core = BOMB_CORE_RADIUS as i32;
    (ax <= core && ay <= core) || (ay == 0 && ax <= arm) || (ax == 0 && ay <= arm)
}
/// Frames the CPU's laser charges (visibly, along the beam) before firing.
pub const LASER_TELEGRAPH_FRAMES: u32 = 10;
/// Ricochets a beam gets before it spends its energy punching through the wall.
pub const LASER_MAX_BOUNCES: u32 = 4;

/// Where a beam went, and the wall cell it breached.
///
/// The breach is data, not an effect: `beam_cells` is `&self` and runs every
/// frame while the CPU is merely aiming, so only `fire_powerup` applies it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BeamPath {
    pub cells: Vec<(u16, u16)>,
    /// The arena-wall cell to convert to a `Hole`, on the fifth strike.
    pub breach: Option<(u16, u16)>,
    /// The wall/hole cell the beam DIED on (breach or not) — where a
    /// whiff's spark belongs (codex v7 verify: `cells.last()` put it on
    /// the preceding traversable cell).
    pub stop: Option<(u16, u16)>,
}

impl BeamPath {
    pub fn contains(&self, cell: &(u16, u16)) -> bool {
        self.cells.contains(cell)
    }
}

impl<'a> IntoIterator for &'a BeamPath {
    type Item = &'a (u16, u16);
    type IntoIter = std::slice::Iter<'a, (u16, u16)>;
    fn into_iter(self) -> Self::IntoIter {
        self.cells.iter()
    }
}
/// Sudden death: after this many frames the arena starts shrinking inward.
pub const SUDDEN_DEATH_START: u32 = 3000;
/// Sudden death: one wall ring closes every this many frames.
pub const SUDDEN_DEATH_INTERVAL: u32 = 150;
/// Bolt lifetime cap. Not a range limit — bolts die on walls, and the arena is
/// always enclosed, so this only exists as defence-in-depth against a future
/// change to the ring-0 frame invariant.
pub const TRI_SHOT_MAX_STEPS: u8 = u8::MAX;
pub const MAX_POWERUPS_ON_BOARD: usize = 3;

#[derive(Clone, Debug)]
pub struct Particle {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub lifetime: u32,
    pub color: (u8, u8, u8),
}

pub struct LightCycle {
    pub head: (u16, u16),
    pub direction: Direction,
    /// The direction the cycle was moving before this turn. Used by the CPU
    /// opponent model to encode direction-transition patterns (corner behaviour).
    pub prev_direction: Direction,
    pub color: (u8, u8, u8),
    pub alive: bool,
    pub is_player: bool,
    pub score: u32,
    pub speed_multiplier: f32,
    /// Snake body, head first. Grows only when food is eaten.
    pub positions: Vec<(u16, u16)>,
    /// Cells still owed to the snake from the last food eaten (its value).
    pub pending_growth: u32,
    /// The power-up currently held (one slot; picking up another replaces it).
    pub held_powerup: Option<PowerUpKind>,
}

impl LightCycle {
    pub fn new(x: u16, y: u16, dir: Direction, color: (u8, u8, u8), is_player: bool) -> Self {
        Self {
            head: (x, y),
            direction: dir,
            prev_direction: dir,
            color,
            alive: true,
            is_player,
            score: 0,
            speed_multiplier: 1.0,
            positions: vec![(x, y)],
            pending_growth: 0,
            held_powerup: None,
        }
    }

    pub fn change_direction(&mut self, new_dir: Direction) {
        match (self.direction, new_dir) {
            (Direction::Up, Direction::Down) | (Direction::Down, Direction::Up) => {}
            (Direction::Left, Direction::Right) | (Direction::Right, Direction::Left) => {}
            _ => self.direction = new_dir,
        }
    }

    /// Record a direction change by snapshotting the current direction into
    /// prev_direction before applying the new one. Called once per frame
    /// from the game loop to track the player's actual movement history
    /// (not just explicit changes), so the CPU model sees continuous motion.
    pub fn snapshot_direction(&mut self) {
        self.prev_direction = self.direction;
    }
}

/// Current WORLD-RULES version for NEW rounds (recorded per round; replays
/// pin theirs). v1: original geometry. v2: the corridor turns the corners.
/// v3: projectiles resolve BEFORE the worms move each frame — a bolt you
/// fired is ahead of you and lands before your body arrives (owner
/// incident 2026-08-07: a tri-shot that should have killed the CPU was
/// pre-empted by the firer's own collision and mis-reported as a ram).
/// v4: SLIPSTREAM is asymmetric SIMULATION time (owner spec): a worm out
/// in the ring-1 corridor steps only 1 frame in 16 while the world clock
/// runs 4× — corridor worm ≈ 25% of original speed, arena worm ≈ 4×.
/// Projectiles ride the fast clock (light does not slow down).
/// v5: a FROZEN worm's prev_direction stays its last EXECUTED heading.
/// v4 snapshotted every frame, so a latched-but-unexecuted press became
/// the 180-ban's anchor within one held frame: changing your mind got
/// keypresses eaten ("impossible to turn"), and a two-press sequence
/// could sneak a true reversal past the ban into your own neck.
/// v6 (owner spec; ADR-022 unbundling — corridor ONLY): the outer
/// corridor is TWO lanes wide — the arena wall moves to ring 3, so
/// turning and overtaking inside the ring become real maneuvers and the
/// interior shrinks a cell per side. The owner's decoy-bomb and napalm
/// specs are later versions: one physics change per version, each with
/// its own replay pinning and benchmark receipts.
/// v11 (owner: "the tri-shot isn't lethal enough … it seems to have no
/// damage … maybe they need to go further, but if they touch the
/// opponent at all, that's what needs to catch them on fire"): NAPALM
/// REACH. Bolts fly a FULL RAY again at TWO cells per frame — a fired
/// bolt cannot be outrun — as two ordered one-cell substeps, each
/// running the complete collision pipeline (never a teleport over a
/// cell). Any swept contact IGNITES AND CATCHES — the v9 crossing-swap
/// branch's instant TriShotBolt death was inconsistent with
/// napalm-on-touch and is gone at v11: touch = fire, the burn schedule
/// does the killing. Receipt for the change: the burn engine was
/// proven correct (catch at frame 3, len 10 -> 6) — the live
/// damagelessness was REACH: 4-cell worm-speed bolts were dodged or
/// outrun by construction, so a touch almost never happened.
/// v10 (owner bug report: "if I hit the arrow keys rapidly, the 2nd
/// key is often not registered … we need to do a better job of
/// collecting keys"): THE INPUT QUEUE. Player inputs — turns AND fires
/// — are collected in a bounded ordered queue (cap 3, drop-newest) and
/// consumed at the frame's player phase: at most one turn executes per
/// frame, each validated against the heading the worm ACTUALLY moved
/// last (a true 180 at its consumption moment is dropped and the next
/// entry drains in the same frame); a fire executes when it reaches
/// the queue head, so "turn then fire" discharges along the NEW
/// heading one frame later — the aim the player set up, never the
/// stale one (codex v10 consult, the turn-then-fire blocker). Ghost
/// recording moves to consumption time: one accepted turn per frame by
/// construction. Pre-v10 ghosts replay the old single-slot, press-time
/// semantics through the old pump.
/// v9 (owner spec; ADR-022): NAPALM. The tri-shot's bolts fly only 4
/// cells, and wherever a bolt ENDS — wall-hit, 4-cell expiry, or worm
/// contact — it ignites a flame patch that burns ~3 wall-clock seconds.
/// A worm in CONTINUOUS contact burns down on a wall-clock schedule: up
/// to 5 segments in the first second, 3 in the second, 1 in the third —
/// and the schedule is STICKY (once caught it runs to completion even
/// as the burning tail shrinks out of the fire). Burned past the head =
/// dead (DeathCause::Burned, attributed to the flame's owner for the
/// weapon ledger). The firer is IMMUNE to their own flames — the same
/// ADR-023 rule the laser and (since v8) the bomb obey. Flames COOK a
/// decoy they touch (early detonation), burn frozen worms (wall-clock
/// spares nobody), and flames tick in the common hazard phase on every
/// frame exit. In the two-lane corridor a patch blocks ONE lane.
/// v8 (owner spec; ADR-022): THE TIMED DECOY. A planted bomb is a food
/// decoy for ~15 wall-clock seconds, then DETONATES — punching arena
/// walls like any other blast — instead of fizzling. The last two
/// seconds FLASH (tier 1), the last one flashes harder (tier 2), so the
/// timer kill is telegraphed: an attentive player gets two full seconds
/// of visible warning, and the CPU reads the same fuse age the flash
/// shows (information parity, no pre-flash reveal anywhere). Under v8
/// `Bomb::fuse` counts MILLISECONDS, drained by the current frame
/// delay each global frame — true wall-clock, deterministic, and a
/// slipstream freeze cannot disarm a mine (bombs tick globally). The
/// blast is OWNER-SAFE, trail included — ADR-023's firer-immunity rule
/// applied to bombs (measured: the first expiry wave was severing the
/// planting CPU to scrap through its own forgotten mines). Sudden
/// death also starts closing one
/// ring INSIDE the v6 arena wall (base 3) instead of the pre-v6 base 2
/// it had kept out of replay caution.
/// v7 (ADR-023, unanimous consult): LASER SIMULTANEITY — the beam
/// exists across the one-cell movement transition. It is evaluated at
/// discharge (unchanged: bombs, head kill, sever, breach — the aim the
/// player took is the aim the game grades) AND once more after that
/// frame's movement, against the immutable snapshotted cells: a head
/// that stepped INTO the line dies, a body cell that entered is
/// severed. Before v7 the discharge was graded strictly pre-move while
/// the flash painted post-move — the owner shot exactly what the game
/// showed him and the game graded a world it never showed (his
/// recorded round: beam row 24, CPU stepping to (4,24) the same frame,
/// hole punched, nothing severed — and every "near miss" in his
/// forensics was a target stepping into an already-dead beam). Mirror
/// of the v3 bolt-ordering fix. Same-frame deaths are atomic (both
/// dead = draw); the firer is immune to their own beam; a frozen worm
/// enters nothing but remains hittable at discharge.
pub const ARENA_VERSION: u8 = 11;

/// A collected player input (world v10): consumed in order at the
/// frame's player phase.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PlayerInput {
    Turn(Direction),
    Fire,
}

/// A napalm patch (world v9): ignited where a tri-shot bolt ends.
/// Life counts wall-clock milliseconds, drained by the current frame
/// delay each global frame (the v8 fuse clock).
#[derive(Clone, Debug, PartialEq)]
pub struct Flame {
    pub x: u16,
    pub y: u16,
    /// Milliseconds of burn remaining.
    pub life_ms: u32,
    /// Who lit it — burn kills credit this cycle's weapon ledger, and
    /// the owner is immune to their own fire (ADR-023 rule).
    pub owner: u8,
}

/// One worm's burn state (world v9). STICKY: once caught, the 5/3/1
/// schedule runs to completion on the wall clock even if the burning
/// tail shrinks out of the fire. Never parallel arrays, never inferred
/// from DeathCause alone (codex v6 reject list) — the owner is recorded
/// at catch for ledger attribution.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BurnState {
    /// Milliseconds since caught; 0 = not burning.
    pub contact_ms: u32,
    /// Segments already burned off this catch.
    pub taken: u32,
    /// Who lit the fire that caught this worm.
    pub burned_by: u8,
}

/// A beam discharged this frame, awaiting its post-move occupancy test
/// (ADR-023 world v7). The cells are IMMUTABLE — snapshotted once at
/// discharge; the reconciliation never retraces.
#[derive(Clone, Debug)]
pub struct PendingBeam {
    pub firer: u8,
    pub cells: Vec<(u16, u16)>,
    /// Ignition already connected (kill/sever/bomb) — suppresses the
    /// whiff clank.
    pub ignition_hit: bool,
    /// Terminal wall/hole cell — where a whiff's spark belongs.
    pub stop: Option<(u16, u16)>,
    pub breached: bool,
}

/// One beam's render state (ADR-023 contract): age 0 = lethal core,
/// 1-5 dimming afterimage, 6-20 embers. `fresh` marks a beam discharged
/// this frame so its solid core renders exactly once on every exit path.
#[derive(Clone, Debug)]
pub struct BeamFx {
    pub cells: Vec<(u16, u16)>,
    pub age: u32,
    fresh: bool,
}

/// Forensic snapshot of a laser discharge (diagnostic only).
#[derive(Clone, Debug)]
#[doc(hidden)]
pub struct LaserAudit {
    pub firer: usize,
    pub cells: Vec<(u16, u16)>,
    pub opp_positions: Vec<(u16, u16)>,
    pub cut: Option<usize>,
}

/// Diagnostic event-supply funnel (codex v6 consult, Q2): cheap counters
/// over the evidence-eligibility path, cumulative for the life of this
/// WormGame. Never persisted, never read by play logic — receipts only.
#[derive(Default, Clone, Copy)]
pub struct FunnelStats {
    /// Scored (moving, non-frozen) player frames.
    pub moves: u32,
    /// Frames where holding the line was legal.
    pub straight_legal: u32,
    /// ...and both laterals were also legal.
    pub two_lat: u32,
    /// Voluntary laterals taken (straight legal, turned anyway).
    pub vol_lat: u32,
    /// ...with both sides open — the side-evidence supply.
    pub vol_two_sided: u32,
    /// Pending book records consumed (matched or stale).
    pub pend_taken: u32,
    /// ...that matched their target frame.
    pub pend_matched: u32,
    /// Matched records that carried a side call.
    pub side_declared: u32,
    /// Pending records DROPPED unconsumed — overwritten by a later
    /// producer frame (frozen-player frames produce but never consume)
    /// or discarded by restart(). The take-side counters cannot see
    /// these; without this the funnel overstates its own completeness.
    pub pend_dropped: u32,
    /// Lateral taken with BOTH laterals legal, straight legal or not — an
    /// UPPER BOUND on the published lateral channel's feed: record() has
    /// the same gate but only runs on frames that carried a forecast, so
    /// silent-model frames count here without feeding lat_samples.
    pub lat_supply: u32,
    /// Forced breaks (straight illegal, lateral taken).
    pub forced_break: u32,
}

pub struct WormGame {
    /// Arena geometry this game builds (replays pin their recorded one).
    pub arena_version: u8,
    /// Evidence-supply funnel receipts (diagnostic only).
    pub funnel: FunnelStats,
    /// World v10: the player's collected inputs, in press order.
    pub input_queue: std::collections::VecDeque<PlayerInput>,
    /// Live napalm patches (world v9).
    pub flames: Vec<Flame>,
    /// Per-cycle burn state (world v9).
    pub burns: [BurnState; 2],
    /// World v7 (ADR-023): beams discharged this frame, awaiting their
    /// post-move occupancy test.
    pub pending_beams: Vec<PendingBeam>,
    /// Beam render layer (ADR-023 contract).
    pub beam_fx: Vec<BeamFx>,
    /// Forensic record of the LAST laser discharge (diagnostic only).
    #[doc(hidden)]
    pub laser_audit: Option<LaserAudit>,
    /// Same, but never consumed (diagnostic only).
    #[doc(hidden)]
    pub laser_audit_last: Option<LaserAudit>,
    /// Whether the current round's ledgers have been finalized (exactly-
    /// once discipline; see finalize_round_ledgers).
    pub ledgers_finalized: bool,
    pub width: u16,
    pub height: u16,
    pub grid: Vec<Vec<CellType>>,
    pub cycles: Vec<LightCycle>,
    pub player: usize,
    /// Up to 5 food items on the board at any time. Each is (x, y, value).
    pub food_items: Vec<(u16, u16, u8)>,
    pub score: u32,
    pub game_over: bool,
    pub particles: Vec<Particle>,
    pub time: u32,
    pub winner: Option<usize>,
    pub difficulty: u32,
    pub frame_count: u32,
    pub cpu_history: Vec<CPUPlayRecord>,
    pub cpu_brain: crate::cpu_ai::CpuBrain,
    pub frames_since_cpu_move: u32,
    /// Live power-ups on the board: (x, y, kind). Mirrors food_items.
    pub powerups: Vec<(u16, u16, PowerUpKind)>,
    /// Live tri-shot bolts in flight.
    pub projectiles: Vec<Projectile>,
    /// Planted bombs counting down.
    pub bombs: Vec<Bomb>,
    /// Frames until the next power-up spawn attempt.
    pub powerup_timer: u32,
    /// Seeded WORLD RNG (food/power-up spawns, mine disguises, particles) —
    /// deterministic per round from the recorded round seed. None = thread RNG.
    pub rng: Option<StdRng>,
    /// Separate CPU-DECISION stream (explore rolls). Split from the world
    /// stream so a ghost replay — which re-drives the CPU from the log and
    /// never calls cpu_decide — consumes the world stream identically and
    /// reproduces the round's item spawns bit-for-bit.
    pub cpu_rng: Option<StdRng>,
    /// Ghost recorder: everything needed to replay THIS round exactly —
    /// the per-round seed the RNG was reseeded from, board size, and the
    /// ordered input-event stream. A recorded round replays bit-identically
    /// from (seed, size, log) alone, which is what lets a real player's
    /// games become evaluation data (ADR-016).
    pub replay: ReplayLog,
    /// When Some, this round is a GHOST REPLAY: inputs come from the script
    /// at the same engine sites that recorded them, cpu_decide/should_fire
    /// never run, and physics uses live-equivalent collision policy.
    pub script: Option<ReplayScript>,
    /// When false, the CPU is driven externally (benchmark scripted opponents)
    /// and update() neither calls cpu_decide/should_fire nor records CPU
    /// episodes — the "naive" bench row is then a genuinely naive wall
    /// follower instead of a fresh-brain adaptive CPU in disguise.
    pub cpu_autopilot: bool,
    /// Ghost-eval mode: run the full learning + forecasting pipeline (episode
    /// recording, ensemble scoring, sealed forecasts) WITHOUT cpu_decide —
    /// both worms are driven externally from a replay log, and the brain
    /// under evaluation watches the recorded human exactly as it would have
    /// watched them live (ADR-016).
    pub shadow_learning: bool,
    /// Session scoreboard (rps-ai's You-vs-Computer bar): games won by
    /// [player, cpu] since process start. Banked in restart(), never reset
    /// in-session.
    pub session_wins: [u32; 2],
    /// Fixed board dimensions (browser/explicit-size games). None = derive
    /// from the terminal on every restart (native resize support).
    pub fixed_dims: Option<(u16, u16)>,
    /// Total food value eaten by both cycles this game — drives the speed
    /// ramp (frame_delay shrinks as this grows). Reset per game.
    pub food_eaten_total: u32,
    /// Symmetric per-cycle food value for HUD/history. `LightCycle::score` is
    /// intentionally benchmark-compatible and P2 includes survival frames, so
    /// it must never be presented as a fair P1-vs-P2 comparison.
    pub food_eaten_by: [u32; 2],
    /// Active-ensemble prediction evidence for this game only. The persisted
    /// CpuBrain counters below it provide the lifetime/session scope.
    pub round_pred_hits: u32,
    pub round_pred_total: u32,
    /// This round's read record, measured against the player's own base rate.
    /// The every-frame `round_pred_*` counters above are kept untouched so
    /// existing telemetry does not silently change meaning.
    pub round_read: crate::cpu_ai::ReadRate,
    /// How well the CPU reads THIS player, in [0,1] — lift over their own base
    /// rate, pooled over their lifetime. Recomputed ONCE per round and held
    /// constant for its duration: a CPU whose aggression drifts mid-round
    /// reads as erratic rather than as adaptive.
    pub read_rate: f32,
    /// Seed for the prediction seals. Derived from the game seed, never drawn
    /// from the RNG stream.
    pub seal_seed: u64,
    /// Every revealed seal folded into one number, so a whole round has a
    /// single value a player can copy and re-verify offline.
    pub seal_chain: u64,
    /// Seals revealed this round.
    pub seal_frames: u32,
    /// Frame-owned CPU evidence: scored forecast, actual decision, and the
    /// separately-labelled forecast for the next frame.
    pub cpu_telemetry: crate::cpu_ai::CpuFrameTelemetry,
    /// Most recent completed decision in this round. Game-over history uses
    /// this explicitly-labelled fallback when the lethal frame ended before
    /// the CPU received a turn; it never masquerades as the current frame.
    pub round_last_cpu_decision: Option<crate::cpu_ai::CpuDecisionTrace>,
    /// What killed the losing cycle (first lethal event wins). Shown on the
    /// game-over screen so "how did I die?" is never a mystery.
    pub death_cause: Option<DeathCause>,
    /// Frames the CPU's laser has been visibly charging. The beam telegraphs
    /// for LASER_TELEGRAPH_FRAMES before it fires so crossing the CPU's
    /// firing line is dodgeable, not an unannounced instant death.
    pub cpu_laser_charge: u32,
    /// Sudden death: how many inward wall rings have closed so far.
    pub shrink_level: u16,
}

/// The lethal event that ended the game.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum DeathCause {
    Wall,
    OwnTrail,
    EnemyTrail,
    HeadOn,
    BombBlast,
    Laser,
    TriShotBolt,
    /// Napalm (world v9): burned down past the head.
    Burned,
}

impl DeathCause {
    pub fn as_str(self) -> &'static str {
        match self {
            DeathCause::Wall => "hit the wall",
            DeathCause::OwnTrail => "hit own trail",
            DeathCause::EnemyTrail => "hit enemy trail",
            DeathCause::HeadOn => "head-on collision",
            // Names the mechanic, because the victim never saw a bomb: the
            // thing that exploded was drawn as food from the moment it was
            // planted. An unexplained death reads as a cheat (ADR-003).
            DeathCause::BombBlast => "blown up by a mine disguised as food",
            DeathCause::Laser => "laser beam",
            DeathCause::TriShotBolt => "caught in a tri-shot burst",
            DeathCause::Burned => "burned down by napalm",
        }
    }
}

#[derive(Clone)]
pub struct CPUPlayRecord {
    pub player_move_pattern: String,
    pub cpu_move_pattern: String,
    pub outcome: i32,
    pub seq: u32,
}

/// The ghost log — a round's complete input record as ONE totally-ordered
/// event stream (~2-6 KB per round). v1 kept directions and fires in
/// separate arrays and lost their relative order and phase; external
/// review found three divergence classes ordering alone causes
/// (fatal-frame turns unrecorded, turn-then-fire firing along the wrong
/// heading, CPU fires replayed at the wrong phase inside the frame). v2
/// events are recorded AT the live input sites and re-injected at those
/// same sites, so phase fidelity holds by construction.
///
/// Event kinds — the site IS the phase:
///   0 player direction (change_direction, between frames)
///   1 player fire      (fire_powerup,     between frames)
///   2 CPU direction    (the cpu_decide site, inside update, pre-move)
///   3 CPU fire         (the should_fire site, inside update, pre-move)
#[derive(Clone, Debug, Default)]
pub struct ReplayLog {
    /// Seed the round's RNG was (re)seeded from at round start.
    pub round_seed: u64,
    pub width: u16,
    pub height: u16,
    /// Arena geometry version the round was played on (see `build_grid`).
    /// Rounds recorded before versioning existed carry no field and
    /// replay as 1.
    pub arena: u8,
    /// (frame stamp at the site, kind, value). Between-frame events carry
    /// the last COMPLETED frame; in-update events carry the current one.
    pub events: Vec<(u32, u8, u8)>,
    /// Last recorded CPU direction — kind-2 events log changes only.
    pub last_cpu_dir: Option<Direction>,
}

impl ReplayLog {
    pub fn to_json(&self, frames: u32) -> String {
        let ev: Vec<String> = self
            .events
            .iter()
            .map(|&(f, k, v)| format!("[{},{},{}]", f, k, v))
            .collect();
        // The seed is a STRING on the wire: it is a u64, and JavaScript's
        // JSON.parse coerces bare numbers to f64 — seeds above 2^53 came
        // back mangled and the replay diverged.
        format!(
            "{{\"v\":2,\"seed\":\"{}\",\"w\":{},\"h\":{},\"arena\":{},\"frames\":{},\"ev\":[{}]}}",
            self.round_seed,
            self.width,
            self.height,
            self.arena,
            frames,
            ev.join(",")
        )
    }
}

/// A recorded event stream being replayed — consumed strictly in order at
/// the same engine sites that recorded it.
#[derive(Clone, Debug, Default)]
pub struct ReplayScript {
    pub events: Vec<(u32, u8, u8)>,
    pub cursor: usize,
}

impl ReplayScript {
    /// Consume the next event if it matches (frame, kind).
    fn take(&mut self, frame: u32, kind: u8) -> Option<u8> {
        match self.events.get(self.cursor) {
            Some(&(f, k, v)) if f == frame && k == kind => {
                self.cursor += 1;
                Some(v)
            }
            _ => None,
        }
    }
    /// Peek the next event when it matches (frame, any of kinds).
    fn next_is(&self, frame: u32, kinds: &[u8]) -> Option<(u8, u8)> {
        match self.events.get(self.cursor) {
            Some(&(f, k, v)) if f == frame && kinds.contains(&k) => Some((k, v)),
            _ => None,
        }
    }
}

impl Default for WormGame {
    fn default() -> Self {
        Self::new()
    }
}

impl WormGame {
    /// Random range helper — uses seeded RNG when set, thread RNG otherwise.
    /// (WASM builds are always seeded from JS; the thread-RNG fallback is
    /// native-only because getrandom has no backend on wasm32-unknown.)
    pub fn rng_range<T, R>(&mut self, range: R) -> T
    where
        R: rand::distr::uniform::SampleRange<T>,
        T: rand::distr::uniform::SampleUniform,
    {
        use rand::RngExt;
        match self.rng.as_mut() {
            Some(rng) => rng.random_range(range),
            #[cfg(not(target_arch = "wasm32"))]
            None => rand::rng().random_range(range),
            #[cfg(target_arch = "wasm32")]
            None => unimplemented!("worm: unseeded RNG on wasm — pass a seed"),
        }
    }

    /// CPU-decision stream: random float in [a, b). See `cpu_rng`.
    pub fn rng_cpu_f32(&mut self, a: f32, b: f32) -> f32 {
        use rand::RngExt;
        match self.cpu_rng.as_mut() {
            Some(rng) => rng.random_range(a..b),
            #[cfg(not(target_arch = "wasm32"))]
            None => rand::rng().random_range(a..b),
            #[cfg(target_arch = "wasm32")]
            None => unimplemented!("worm: unseeded RNG on wasm — pass a seed"),
        }
    }

    /// Random float in [a, b) — uses seeded RNG when set, thread RNG otherwise.
    pub fn rng_f32(&mut self, a: f32, b: f32) -> f32 {
        use rand::RngExt;
        match self.rng.as_mut() {
            Some(rng) => rng.random_range(a..b),
            #[cfg(not(target_arch = "wasm32"))]
            None => rand::rng().random_range(a..b),
            #[cfg(target_arch = "wasm32")]
            None => unimplemented!("worm: unseeded RNG on wasm — pass a seed"),
        }
    }

    pub fn new() -> Self {
        let dims = Dimensions::get_terminal_size();
        Self::build(dims.width, dims.height, None, None)
    }

    /// Create a game with a seeded RNG for deterministic benchmarks.
    pub fn with_seed(seed: u64) -> Self {
        let dims = Dimensions::get_terminal_size();
        Self::build(dims.width, dims.height, Some(seed), None)
    }

    /// Explicit board size (browser / scripted games). Restarts keep these
    /// dimensions instead of re-reading the terminal.
    pub fn with_size(width: u16, height: u16) -> Self {
        Self::build(width, height, None, Some((width, height)))
    }

    /// Explicit board size + seeded RNG (the browser game's constructor).
    pub fn with_size_seed(width: u16, height: u16, seed: u64) -> Self {
        Self::build(width, height, Some(seed), Some((width, height)))
    }

    fn build(width: u16, height: u16, seed: Option<u64>, fixed_dims: Option<(u16, u16)>) -> Self {
        let center_x = width / 2;
        let center_y = height / 2;
        let spacing = 12;

        let player = LightCycle::new(
            center_x.saturating_sub(spacing),
            center_y,
            Direction::Right,
            (0, 255, 255),
            true,
        );

        let cpu = LightCycle::new(
            center_x.saturating_add(spacing),
            center_y,
            Direction::Left,
            (255, 0, 255),
            false,
        );

        let grid = Self::build_grid(width, height, ARENA_VERSION);

        let mut game = Self {
            arena_version: ARENA_VERSION,
            funnel: FunnelStats::default(),
            input_queue: std::collections::VecDeque::new(),
            flames: Vec::new(),
            burns: [BurnState::default(), BurnState::default()],
            pending_beams: Vec::new(),
            beam_fx: Vec::new(),
            laser_audit: None,
            laser_audit_last: None,
            ledgers_finalized: false,
            width,
            height,
            grid,
            cycles: vec![player, cpu],
            player: 0,
            food_items: Vec::new(),
            score: 0,
            game_over: false,
            particles: Vec::new(),
            time: 0,
            winner: None,
            difficulty: 1,
            frame_count: 0,
            cpu_history: Vec::new(),
            cpu_brain: crate::cpu_ai::CpuBrain::new(),
            frames_since_cpu_move: 0,
            powerups: Vec::new(),
            projectiles: Vec::new(),
            bombs: Vec::new(),
            powerup_timer: 60,
            rng: seed.map(StdRng::seed_from_u64),
            cpu_rng: seed.map(|s| StdRng::seed_from_u64(s ^ 0xC0FF_EE00_D15E_A5E5)),
            cpu_autopilot: true,
            shadow_learning: false,
            session_wins: [0, 0],
            fixed_dims,
            food_eaten_total: 0,
            food_eaten_by: [0, 0],
            round_read: crate::cpu_ai::ReadRate::default(),
            read_rate: 0.0,
            seal_seed: seed.unwrap_or(0x5EA1_5EED),
            seal_chain: 0,
            seal_frames: 0,
            round_pred_hits: 0,
            round_pred_total: 0,
            cpu_telemetry: crate::cpu_ai::CpuFrameTelemetry::default(),
            round_last_cpu_decision: None,
            death_cause: None,
            cpu_laser_charge: 0,
            shrink_level: 0,
            replay: ReplayLog::default(),
            script: None,
        };
        // Round 1 goes through the same reseed as every later round, so the
        // first game is ghost-replayable too. Deterministic: the round seed
        // is drawn from the launch-seeded stream.
        game.begin_round_replay(None);
        game.generate_food_items();
        game
    }

    /// How awake the CPU is, 0..1 (ADR-018, the beatable opening). Sharpness
    /// comes from EITHER the read (it knows you) OR scoreboard pressure
    /// (you are beating it — losing focuses anyone). Both inputs are public
    /// information; at 0.6+ the CPU is fully tick-perfect. An unread,
    /// unpressured CPU plays slow and loose — and is genuinely losable.
    pub fn sharpness(&self) -> f32 {
        let read = self.read_rate.clamp(0.0, 1.0);
        let wins = self.displayed_wins();
        let deficit = (wins[0] as f32 - wins[1] as f32) / 4.0;
        (read.max(deficit.clamp(0.0, 1.0)) / 0.6).min(1.0)
    }

    /// Resolve the finished round into the self-knowledge ledgers,
    /// EXACTLY ONCE (ADR-021; idempotence via `ledgers_finalized`).
    /// Called at the game-over save path and consumed by restart().
    /// Ghost evaluation shares the discipline: replays reproduce real
    /// history, so death attribution from them is legitimate; round
    /// summaries (about the PLAYER) record in both modes.
    pub fn finalize_round_ledgers(&mut self) {
        if self.ledgers_finalized {
            return;
        }
        self.ledgers_finalized = true;
        match self.winner {
            Some(1) => {
                // ADR-024: the Boxer credit rule needs the player's
                // reachable space at death to test the realized choke
                // against the episode's precommitted baseline. One flood,
                // once per round end.
                let (phx, phy) = self.cycles[0].head;
                let space_at_death =
                    crate::cpu_ai::count_open_space(self, phx, phy);
                self.cpu_brain.ledgers.resolve_player_death(
                    self.frame_count,
                    self.death_cause,
                    space_at_death,
                    self.shrink_level,
                );
                if let Some(cause) = self.death_cause {
                    let kind = match cause {
                        DeathCause::Laser => Some(PowerUpKind::Laser),
                        DeathCause::TriShotBolt => Some(PowerUpKind::TriShot),
                        // Watch-item (ADR-022): burn kills feed the same
                        // close-range loop they came from.
                        DeathCause::Burned => Some(PowerUpKind::TriShot),
                        DeathCause::BombBlast => Some(PowerUpKind::Bomb),
                        _ => None,
                    };
                    if let Some(k) = kind {
                        self.cpu_brain.ledgers.note_weapon_lethal(k);
                    }
                }
            }
            Some(0) => {
                if let Some(cause) = self.death_cause {
                    self.cpu_brain.ledgers.note_cpu_death(cause as u8);
                }
            }
            _ => {}
        }
        self.cpu_brain.ledgers.end_round(self.frame_count);
    }

    /// Sharpness for SURVIVAL BASICS: any proven read ends the casual
    /// opening outright. The continuous sharpness below is the right
    /// scale for AGGRESSION (spend proportional to evidence), but scaling
    /// survival discipline by a partial read produced a lossy half-woken
    /// middle — measured: dozy won 96%, fully-sharp won 92%, and the
    /// half-sharp transition lost games both of those win (ADR-020
    /// stage 2.1). Sloppy basics exist for the genuinely-unread phase
    /// ONLY; a CPU holding latched evidence has no business faceplanting.
    pub fn discipline_sharpness(&self) -> f32 {
        if self.cpu_brain.earned_snapshot > 0.0 || self.cpu_brain.discipline_latched {
            1.0
        } else {
            self.sharpness()
        }
    }

    /// Is the CPU being ENVELOPED — its open region collapsed under 60%
    /// of eight decisions ago with the player nearby? The same signal
    /// that raises its evacuate standards (task #13 v1); exposed so the
    /// weapon heuristic can respond too. Board knowledge.
    pub fn cpu_enveloped(&self) -> bool {
        let ring = &self.cpu_brain.region_ring;
        let collapsing = ring.len() >= 8
            && ring.back().copied().unwrap_or(0)
                < ring.front().copied().unwrap_or(1) * 6 / 10;
        let (px, py) = self.cycles[0].head;
        let (cx, cy) = self.cycles[1].head;
        let near =
            ((px as i32 - cx as i32).abs() + (py as i32 - cy as i32).abs()) <= 12;
        collapsing && near
    }

    /// How much manufactured opening recklessness (the bold_* knobs) is
    /// still warranted: 1.0 at first contact and even scores, fading to 0
    /// as the CPU pulls AHEAD on the visible scoreboard. Boldness exists to
    /// make first games exciting and killable; a CPU already winning while
    /// unsharp does not need manufactured risk (measured: the half-woken
    /// middle of a warm arc gave back ~10 points of win rate to it).
    /// Public information only, same as sharpness.
    pub fn boldness_scale(&self) -> f32 {
        let wins = self.displayed_wins();
        let lead = wins[1] as f32 - wins[0] as f32;
        (1.0 - (lead - 1.0) / 3.0).clamp(0.0, 1.0)
    }

    /// Reseed the round RNG from a fresh per-round seed and start the ghost
    /// log. `forced` replays a recorded round; `None` derives the seed from
    /// the current stream, so an entire session stays a pure function of the
    /// launch seed while every round becomes independently reproducible from
    /// its own (seed, size, input log) triple.
    pub fn begin_round_replay(&mut self, forced: Option<u64>) {
        let round_seed = match forced {
            Some(s) => s,
            None => self.rng_range(0..u64::MAX),
        };
        self.rng = Some(StdRng::seed_from_u64(round_seed));
        self.cpu_rng = Some(StdRng::seed_from_u64(round_seed ^ 0xC0FF_EE00_D15E_A5E5));
        self.replay = ReplayLog {
            round_seed,
            width: self.width,
            height: self.height,
            arena: self.arena_version,
            ..Default::default()
        };
    }

    /// Build the arena grid. Ring 0 (screen frame) is always Wall. When the
    /// terminal is big enough, ring 2 is the punchable arena wall and ring 1
    /// is the outer corridor — the pacman tunnel between punched holes.
    fn build_grid(width: u16, height: u16, arena: u8) -> Vec<Vec<CellType>> {
        let mut grid = vec![vec![CellType::Empty; width as usize]; height as usize];
        let corridor = width >= 10 && height >= 10;
        for y in 0..height {
            for x in 0..width {
                let frame = x == 0 || y == 0 || x == width - 1 || y == height - 1;
                let ring = if arena >= 6 { 3 } else { 2 };
                let on_ring2 = x == ring
                    || y == ring
                    || x == width - 1 - ring
                    || y == height - 1 - ring;
                // ARENA V2 (owner play report, 2026-08-06): v1 ran the
                // arena-wall rows/columns all the way to the frame, so at
                // every corner the wall's ends CROSSED the ring-1
                // corridor — the "pacman tunnel" was really four dead-end
                // segments, and entering a hole then turning cornerward
                // was death by geometry. V2 keeps the arena wall inside
                // its own rectangle; the corridor turns the corners.
                // V1 is kept verbatim: recorded ghosts replay on the
                // geometry they were played on.
                let ring = if arena >= 6 { 3 } else { 2 };
                let arena_wall = corridor
                    && on_ring2
                    && (arena < 2
                        || (x >= ring
                            && y >= ring
                            && x <= width - 1 - ring
                            && y <= height - 1 - ring));
                if frame || arena_wall {
                    grid[y as usize][x as usize] = CellType::Wall;
                }
            }
        }
        grid
    }

    /// THE WORLD-RULES VIEW (ADR-022): named readings of the one
    /// serialized version byte, replacing scattered `arena_version >= N`
    /// comparisons. A view, never independent flags.
    /// How many lanes wide the outer corridor is.
    pub fn corridor_lanes(&self) -> u16 {
        if self.arena_version >= 6 { 2 } else { 1 }
    }

    /// What a bomb does when its fuse runs out.
    /// `true` = detonate (v8 timed decoy); `false` = fizzle.
    pub fn bomb_expiry_detonates(&self) -> bool {
        self.arena_version >= 8
    }

    /// The ring offset sudden death closes FROM (exclusive): the first
    /// ring to seal is base+1. Pre-v8 kept the pre-corridor base 2 for
    /// replay identity even though the v6 wall moved to ring 3 — which
    /// made the first "closure" the arena wall itself; v8 corrects it.
    pub fn sudden_death_base(&self) -> u16 {
        if self.arena_version >= 8 { 3 } else { 2 }
    }

    /// Whether this terminal is big enough for an outer corridor around the arena wall.
    pub fn has_corridor(&self) -> bool {
        self.width >= 10 && self.height >= 10
    }

    /// The CURRENT arena wall — punchable, and what a beam ricochets off.
    /// Ring 0 is the outer frame and is never punchable.
    ///
    /// Tracks `shrink_level`. It used to be pinned to ring 2, so from the first
    /// sudden-death shrink onward the live inner wall was not recognised as an
    /// arena wall at all: beams stopped dead instead of bouncing or breaching,
    /// and bomb blasts silently stopped breaking walls. The endgame quietly
    /// played by different rules from the rest of the round.
    /// Rebuild THIS game under a different world-rules version — TEST
    /// FIXTURE ONLY (ADR-022: production version construction stays on
    /// the recorded-round path; repainting worms across a changed grid
    /// is only sound on boards the caller controls). Re-marks worms and
    /// food onto the fresh grid.
    #[doc(hidden)]
    pub fn set_world_version(&mut self, v: u8) {
        self.arena_version = v;
        self.grid = Self::build_grid(self.width, self.height, v);
        for (i, c) in self.cycles.iter().enumerate() {
            let marker = if i == 0 { CellType::Player } else { CellType::CPU };
            for &(x, y) in &c.positions {
                self.grid[y as usize][x as usize] = marker;
            }
        }
        for &(x, y, _) in &self.food_items {
            if self.grid[y as usize][x as usize] == CellType::Empty {
                self.grid[y as usize][x as usize] = CellType::Food;
            }
        }
    }

    pub fn arena_wall_offset(&self) -> u16 {
        let base = if self.arena_version >= 6 { 3 } else { 2 };
        base + self.shrink_level
    }

    pub fn is_arena_wall(&self, x: u16, y: u16) -> bool {
        let off = self.arena_wall_offset();
        self.has_corridor()
            && (x == off || y == off || x == self.width - 1 - off || y == self.height - 1 - off)
    }

    /// Can a cycle occupy this cell? Walls, trails, the frame and live bombs are fatal.
    pub fn passable(&self, x: u16, y: u16) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        if self.bombs.iter().any(|b| b.x == x && b.y == y) {
            return false;
        }
        matches!(
            self.grid[y as usize][x as usize],
            CellType::Empty | CellType::Food | CellType::Hole | CellType::PowerUp
        )
    }

    /// Place 1-5 food items on empty cells. The count is random per refill,
    /// so the board never has a fixed pattern — it stays "how many did we get"
    /// rather than a predictable spawn.
    pub fn generate_food_items(&mut self) {
        // Food spawns inside the arena only — never in the outer corridor.
        let (xlo, xhi, ylo, yhi) = if self.has_corridor() {
            (4, self.width - 4, 4, self.height - 4)
        } else {
            (
                2,
                self.width.saturating_sub(2),
                2,
                self.height.saturating_sub(2),
            )
        };
        let n = self.rng_range(1..=5);
        self.food_items.clear();
        for _ in 0..n {
            for _ in 0..200 {
                let x = self.rng_range(xlo..xhi);
                let y = self.rng_range(ylo..yhi);
                if self.grid[y as usize][x as usize] == CellType::Empty
                    && !self.bombs.iter().any(|b| (b.x, b.y) == (x, y))
                    && !self
                        .food_items
                        .iter()
                        .any(|(fx, fy, _)| *fx == x && *fy == y)
                {
                    let num = self.rng_range(1..=9);
                    self.food_items.push((x, y, num));
                    self.grid[y as usize][x as usize] = CellType::Food;
                    break;
                }
            }
        }
        if self.food_items.is_empty() {
            // Guaranteed fallback so there is always food.
            for _ in 0..200 {
                let x = self.rng_range(xlo..xhi);
                let y = self.rng_range(ylo..yhi);
                if self.grid[y as usize][x as usize] == CellType::Empty
                    && !self.bombs.iter().any(|b| (b.x, b.y) == (x, y))
                {
                    let num = self.rng_range(1..=9);
                    self.food_items.push((x, y, num));
                    self.grid[y as usize][x as usize] = CellType::Food;
                    break;
                }
            }
        }
    }

    fn add_impact_particles(&mut self, x: u16, y: u16, color: (u8, u8, u8)) {
        use std::f32::consts::TAU;
        for _ in 0..30 {
            let angle: f32 = self.rng_f32(0.0, TAU);
            let speed = self.rng_f32(0.5, 2.5);
            let hue_offset: f32 = self.rng_f32(-30.0, 30.0);
            let base_hue = rgb_to_hue(color);
            let final_color = hsv_to_rgb((base_hue + hue_offset).rem_euclid(360.0), 0.9, 1.0);
            let lifetime = self.rng_range(15..40);
            self.particles.push(Particle {
                x: x as f32,
                y: y as f32,
                vx: angle.cos() * speed,
                vy: angle.sin() * speed,
                lifetime,
                color: final_color,
            });
        }
    }

    pub fn update(&mut self) -> bool {
        if self.game_over {
            // The sim is done but the browser keeps painting the final
            // board: the beam layer still decays, so the killing shot cools
            // from core to afterimage to embers instead of glowing "hot"
            // forever (codex v7 verify).
            self.age_beam_fx();
            return false;
        }

        // GHOST REPLAY pump: re-apply the player's between-frame inputs in
        // their exact recorded order (turn-then-fire and fire-then-turn are
        // different weapons discharges) through the same public entry points
        // that recorded them.
        if self.script.is_some() && self.arena_version < 10 {
            let frame = self.frame_count;
            loop {
                let next = self
                    .script
                    .as_ref()
                    .and_then(|script| script.next_is(frame, &[0, 1]));
                match next {
                    Some((0, d)) => {
                        if let Some(script) = self.script.as_mut() {
                            script.cursor += 1;
                        }
                        self.change_direction(match d {
                            0 => Direction::Up,
                            1 => Direction::Down,
                            2 => Direction::Left,
                            _ => Direction::Right,
                        });
                    }
                    Some((1, _)) => {
                        if let Some(script) = self.script.as_mut() {
                            script.cursor += 1;
                        }
                        self.fire_powerup(self.player);
                        if self.game_over {
                            return false;
                        }
                    }
                    _ => break,
                }
            }
        }

        self.frame_count += 1;
        self.time += 1;

        // WORLD v3: in-flight bolts get their step FIRST. They were fired
        // in the past and travel ahead of the worm that fired them; under
        // v1/v2 ordering they stepped at frame END, so a firer chasing
        // their own bolt could ram the target and die before the bolt's
        // kill resolved. Replayed v1/v2 ghosts keep the old order (the
        // frame-end call below is version-gated the other way).
        if self.arena_version >= 3 {
            self.advance_projectiles();
            if self.game_over {
                return false;
            }
        }

        // SLIPSTREAM (world v4): a worm whose head is out in the corridor
        // steps only when frame_count % 16 == 0 — on all other frames it
        // is FROZEN: no movement, no tail retract, no collisions of its
        // own making, no decisions, no learning about it (a frozen frame
        // is not a choice). The other worm plays at full rate. Solid
        // while frozen: it can still be hit, shot, and sealed in.
        let slip_hold = self.arena_version >= 4 && !self.frame_count.is_multiple_of(16);
        let player_frozen = slip_hold && self.cycle_in_corridor(self.player);
        let cpu_frozen = slip_hold && self.cycle_in_corridor(1);

        // Consume last frame's forecast into a fresh transaction before any
        // lethal early return. A frame can therefore expose a scored forecast
        // without a CPU decision, or a decision without a next forecast, but
        // never stale fields from a different frame.
        let incoming_forecast = self.cpu_telemetry.next_forecast.take();
        self.cpu_telemetry = crate::cpu_ai::CpuFrameTelemetry::for_frame(self.frame_count);
        // A frozen player makes no move this frame: the pending forecast is
        // neither scored nor discarded — it stays sealed for their next
        // MOVING frame (slipstream, world v4).
        if player_frozen {
            self.cpu_telemetry.next_forecast = incoming_forecast;
        }

        // (The old `difficulty = time/300 + 1` clock ramp lived here. It was
        // read nowhere, and an arbitrary timer is the opposite of what the
        // difficulty is supposed to mean: the CPU gets harder because it has
        // learned YOU, not because the clock advanced. See refresh_read_rate.)

        // Update particles with gravity and fading
        self.particles.retain_mut(|p| {
            p.lifetime = p.lifetime.saturating_sub(1);
            p.vx *= 0.97;
            p.vy *= 0.97;
            p.vy += 0.1;
            p.x += p.vx;
            p.y += p.vy;
            p.lifetime > 0
        });

        // Opponent-model observation: encode the player's situation BEFORE
        // the player moves — the label recorded at frame end is the direction
        // taken from exactly this state (rps-ai: context-before → move-taken).
        // The tail at this point holds moves through last frame, so the
        // transition block cannot contain this frame's label (no leakage).
        let player_ctx_pre =
            crate::cpu_ai::encode_player_context(self, &self.cpu_brain.player_tail);
        // WORLD v10: consume the collected inputs — fires drain freely,
        // at most ONE turn executes per frame, each validated against
        // prev_direction (the heading actually moved) AT ITS MOMENT: a
        // true 180 is dropped and the next entry drains this same frame
        // (codex: [Left, Down] from Right executes Down now, not never).
        // Ghost recording happens HERE, at consumption — one accepted
        // turn per frame by construction; replayed v10 ghosts feed this
        // same queue at their stamped frame.
        if self.arena_version >= 10 {
            if self.script.is_some() {
                let frame = self.frame_count;
                loop {
                    let next = self
                        .script
                        .as_ref()
                        .and_then(|script| script.next_is(frame, &[0, 1]));
                    match next {
                        Some((0, d)) => {
                            if let Some(script) = self.script.as_mut() {
                                script.cursor += 1;
                            }
                            self.input_queue.push_back(PlayerInput::Turn(match d {
                                0 => Direction::Up,
                                1 => Direction::Down,
                                2 => Direction::Left,
                                _ => Direction::Right,
                            }));
                        }
                        Some((1, _)) => {
                            if let Some(script) = self.script.as_mut() {
                                script.cursor += 1;
                            }
                            self.input_queue.push_back(PlayerInput::Fire);
                        }
                        _ => break,
                    }
                }
            }
            let mut turned = false;
            while let Some(&front) = self.input_queue.front() {
                match front {
                    PlayerInput::Fire => {
                        self.input_queue.pop_front();
                        if self.script.is_none() {
                            self.replay.events.push((self.frame_count, 1, 0));
                        }
                        self.fire_powerup(self.player);
                        if self.game_over {
                            return false;
                        }
                    }
                    PlayerInput::Turn(d) => {
                        if turned {
                            break;
                        }
                        self.input_queue.pop_front();
                        let moved = self.cycles[self.player].prev_direction;
                        let is_180 = matches!(
                            (moved, d),
                            (Direction::Up, Direction::Down)
                                | (Direction::Down, Direction::Up)
                                | (Direction::Left, Direction::Right)
                                | (Direction::Right, Direction::Left)
                        );
                        if !is_180 {
                            self.cycles[self.player].direction = d;
                            turned = true;
                            if self.script.is_none() {
                                self.replay.events.push((self.frame_count, 0, d as u8));
                            }
                        }
                    }
                }
            }
        }
        let player_dir_this_frame = self.cycles[self.player].direction;
        // Was this frame INFORMATIVE about the human? Captured here, at frame
        // start, because it describes the board they chose on — after the move
        // it is unrecoverable. Drives which episodes the corpus keeps.
        //
        // Deliberately NOT `option_count >= 2`: measured, that fires on 99.6%
        // of frames, because trails retract and the arena stays ~90% empty, so
        // having two options is the normal state rather than a junction. It
        // would stratify nothing.
        //
        // A frame carries information about the player when they were FORCED
        // to break (straight was illegal) or when they CHOSE to turn with the
        // line still open. Continuing straight down an open corridor is the
        // board talking, not the human.
        let player_heading_pre = self.cycles[self.player].prev_direction;
        let player_had_choice = {
            let legal_pre = crate::cpu_ai::legal_options(self, self.player);
            !legal_pre.contains(&player_heading_pre)
                || player_dir_this_frame != player_heading_pre
        };

        // Last frame's forecasts targeted this input, even when this move is
        // lethal. Score them before collision early-returns can end the game.
        // A FROZEN frame is not a decision: nothing scores, nothing observes
        // (slipstream would otherwise pollute the read with phantom
        // straights).
        if (self.cpu_autopilot || self.shadow_learning) && !player_frozen {
            // The choice the player actually faced this frame, read before
            // anything moves. `prev_direction` is the heading they were
            // travelling, so the turn is relative to that — the same anchor
            // the 180 latch uses.
            let player_options = crate::cpu_ai::option_count(self, self.player);
            let player_heading = self.cycles[self.player].prev_direction;
            let player_turn =
                crate::cpu_ai::Turn::from_dirs(player_heading, player_dir_this_frame);

            // Fold the move into the relative-turn prior — but ONLY at forced
            // turns, where holding the line was not an option.
            //
            // Recording every frame destroys the statistic. PRIOR_DECAY ages
            // all three counts on every call, while an increment lands only on
            // the turn actually taken; with ~98% of frames Straight, Left and
            // Right each decay ~50x between consecutive turns and the rarer of
            // the two collapses to numerical zero. Measured against a persona
            // breaking left 84:16, the tally read [99.14, 0.856, 1.4e-8] — it
            // had stopped counting how OFTEN and started counting how RECENTLY.
            //
            // Recording only forced turns makes the decay rate match the event
            // rate, and it is also the honest definition of the habit: "which
            // way do you break when you cannot go straight".
            // ...at every forced turn — single-option ones included.
            //
            // A stricter "only when both sides were legal" gate was measured:
            // it records ~1.2 events per game against ~4.4 for this rule, and
            // the prior it produces is data-starved (2:1 on an 85:15 habit).
            // It was also answering the wrong question. The prior's consumer
            // is the forced-turn forecast, whose job is to predict WHICH WAY
            // THE PLAYER ENDS UP GOING when blocked — and single-option
            // outcomes are part of that target distribution. A player's
            // environment is downstream of their own habit (a left-breaker
            // spends their life on left-curving paths), so those frames carry
            // real signal about the exact quantity being predicted. This
            // models outcomes, not idealised choices, and outcomes are what
            // the forecast is scored on.
            let legal_now = crate::cpu_ai::legal_options(self, self.player);
            if !legal_now.contains(&player_heading) {
                if let Some(turn) = player_turn {
                    self.cpu_brain.opp_brain.observe_turn(turn);
                    // The pattern model sees the same break stream.
                    match turn {
                        crate::cpu_ai::Turn::Left => {
                            self.cpu_brain.turn_pattern.observe(true)
                        }
                        crate::cpu_ai::Turn::Right => {
                            self.cpu_brain.turn_pattern.observe(false)
                        }
                        crate::cpu_ai::Turn::Straight => {}
                    }
                }
            }

            if let Some(forecast) = incoming_forecast {
                if let Some(predicted) = forecast.predicted {
                    let hit = predicted == player_dir_this_frame;
                    self.cpu_brain.opp_pred_total += 1;
                    self.round_pred_total += 1;
                    if hit {
                        self.cpu_brain.opp_pred_hits += 1;
                        self.round_pred_hits += 1;
                    }
                    // The honest record, alongside — never instead of — the
                    // every-frame counters above. A reversal yields no turn
                    // (tests write `direction` directly, bypassing the latch),
                    // so that frame is skipped rather than unwrapped.
                    if let Some(turn) = player_turn {
                        // The forecast's own class, same anchor as the actual
                        // turn — feeds the class-conditional baseline.
                        let predicted_turn = crate::cpu_ai::Turn::from_dirs(
                            player_heading,
                            predicted,
                        )
                        .unwrap_or(crate::cpu_ai::Turn::Straight);
                        // The board's legal set this frame, as relative
                        // turns — public knowledge both predictors get.
                        let mut legal_turns = [false; 3];
                        for &d in &legal_now {
                            if let Some(t) =
                                crate::cpu_ai::Turn::from_dirs(player_heading, d)
                            {
                                legal_turns[crate::cpu_ai::turn_index(t)] = true;
                            }
                        }
                        self.round_read.record(
                            player_options,
                            turn,
                            predicted_turn,
                            legal_turns,
                            hit,
                        );
                        self.cpu_brain.lifetime_read.record(
                            player_options,
                            turn,
                            predicted_turn,
                            legal_turns,
                            hit,
                        );
                    }
                    // Reveal: fold this seal and the move it was scored
                    // against into a single round-long chain the player can
                    // copy and re-verify.
                    self.seal_chain = crate::cpu_ai::fnv1a64(
                        &[
                            self.seal_chain.to_le_bytes().as_slice(),
                            forecast.seal.to_le_bytes().as_slice(),
                            &[player_dir_this_frame as u8],
                        ]
                        .concat(),
                    );
                    self.seal_frames += 1;
                    self.cpu_telemetry.scored = Some(crate::cpu_ai::ScoredForecast {
                        forecast,
                        actual: player_dir_this_frame,
                        hit,
                    });
                }
            }
            // THE TURN BOOK's training (ADR-020 stage 2) — specialist
            // accounting, gate-independent, BEFORE score_frame consumes the
            // pending predictions this frame is graded against. Eligible
            // frames are those where straight was legal: the player could
            // have held the line, so WHEN and WHICH WAY were genuinely
            // theirs. Forced turns belong to the mask, not the books.
            {
                self.funnel.moves += 1;
                let straight_legal = legal_now.contains(&player_heading);
                let both_laterals_legal = {
                    let mut l = 0;
                    for &d in &legal_now {
                        if matches!(
                            crate::cpu_ai::Turn::from_dirs(player_heading, d),
                            Some(crate::cpu_ai::Turn::Left) | Some(crate::cpu_ai::Turn::Right)
                        ) {
                            l += 1;
                        }
                    }
                    l == 2
                };
                let turned_lateral = matches!(
                    player_turn,
                    Some(crate::cpu_ai::Turn::Left) | Some(crate::cpu_ai::Turn::Right)
                );
                // A stale record (round restart, frame skip) must never
                // score against the wrong input.
                let pend_any = self.cpu_brain.pending_book.is_some();
                let pend_book = self
                    .cpu_brain
                    .pending_book
                    .take()
                    .filter(|p| p.target_frame == self.frame_count);
                {
                    let f = &mut self.funnel;
                    let turned = turned_lateral;
                    if straight_legal {
                        f.straight_legal += 1;
                        if both_laterals_legal {
                            f.two_lat += 1;
                        }
                        if turned {
                            f.vol_lat += 1;
                            if both_laterals_legal {
                                f.vol_two_sided += 1;
                            }
                        }
                    }
                    if turned && both_laterals_legal {
                        f.lat_supply += 1;
                    }
                    if !straight_legal && turned {
                        f.forced_break += 1;
                    }
                    if pend_any {
                        f.pend_taken += 1;
                    }
                    if let Some(p) = &pend_book {
                        f.pend_matched += 1;
                        if p.side.is_some() {
                            f.side_declared += 1;
                        }
                    }
                }
                if let Some(p) = pend_book {
                    if straight_legal && player_turn.is_some() {
                        self.cpu_brain.class_books.observe_hazard(p.cell, turned_lateral);
                        self.cpu_brain.class_books.observe_straight_book(
                            player_turn == Some(crate::cpu_ai::Turn::Straight),
                        );
                        // SIDE-skill population (codex round 2): only
                        // genuine two-sided choices count — with one legal
                        // lateral the side is board-determined.
                        if turned_lateral && both_laterals_legal {
                            if let Some(fd) = p.food_side_dir {
                                self.cpu_brain
                                    .class_books
                                    .observe_toward_food(fd == player_dir_this_frame);
                            }
                            self.cpu_brain.class_books.side_opportunities =
                                self.cpu_brain.class_books.side_opportunities.saturating_add(1);
                            if let Some(side) = p.side {
                                self.cpu_brain.class_books.side_declarations = self
                                    .cpu_brain
                                    .class_books
                                    .side_declarations
                                    .saturating_add(1);
                                let side_hit = side == player_dir_this_frame;
                                self.cpu_brain.class_books.observe_turn_book(side_hit);
                                // The book's own honest record — same
                                // machinery, same nulls as the published
                                // read (ADR-020 stage 2.1).
                                if let (Some(actual_t), Some(pred_t)) = (
                                    player_turn,
                                    crate::cpu_ai::Turn::from_dirs(player_heading, side),
                                ) {
                                    let mut legal_turns = [false; 3];
                                    for &d in &legal_now {
                                        if let Some(t) =
                                            crate::cpu_ai::Turn::from_dirs(player_heading, d)
                                        {
                                            legal_turns[crate::cpu_ai::turn_index(t)] = true;
                                        }
                                    }
                                    self.cpu_brain.class_books.book_read.record(
                                        player_options,
                                        actual_t,
                                        pred_t,
                                        legal_turns,
                                        side_hit,
                                    );
                                }
                            }
                            let pend = self.cpu_brain.ensemble.pending;
                            self.cpu_brain
                                .class_books
                                .score_turn_frame(&pend, player_dir_this_frame);
                        }
                    }
                }
                // ADR-021 Kata 0: the drift family's raw material — the
                // same voluntary-lateral eligibility as the book, plus
                // the chase-context distance ring.
                {
                    let (px, py) = self.cycles[self.player].head;
                    let (cx2, cy2) = self.cycles[1].head;
                    let dist = ((px as i32 - cx2 as i32).abs()
                        + (py as i32 - cy2 as i32).abs()) as u32;
                    let lat = if straight_legal && turned_lateral {
                        Some(player_turn == Some(crate::cpu_ai::Turn::Left))
                    } else {
                        None
                    };
                    let gap_before = self.cpu_brain.gap_since_voluntary;
                    self.cpu_brain.ledgers.note_frame(dist, lat, gap_before);
                }
                // The dedicated hazard counter: voluntary laterals only —
                // and the voluntary-turn VOMM (M13 `alt`) sees the same
                // stream.
                if straight_legal && turned_lateral {
                    self.cpu_brain.gap_since_voluntary = 0;
                    if let Some(t) = player_turn {
                        self.cpu_brain
                            .voluntary_pattern
                            .observe(t == crate::cpu_ai::Turn::Left);
                    }
                } else {
                    self.cpu_brain.gap_since_voluntary =
                        self.cpu_brain.gap_since_voluntary.saturating_add(1);
                }
                self.cpu_brain.frames_since_food =
                    self.cpu_brain.frames_since_food.saturating_add(1);
            }
            self.cpu_brain
                .ensemble
                .score_frame(player_dir_this_frame, self.frame_count);
        }

        // Retract player tail first (unless owed growth cells) so the vacated
        // cell isn't a false self-collision. Only erase a cell that still holds
        // this snake's own marker — after a blast trims the trail, or when the
        // opponent has legally entered a just-vacated cell, a stale pop must
        // not wipe a cell someone else now occupies.
        if !player_frozen {
            let cycle = &mut self.cycles[self.player];
            if cycle.positions.len() > 1 && cycle.pending_growth == 0 {
                let tail = cycle.positions.pop().unwrap();
                if self.grid[tail.1 as usize][tail.0 as usize] == CellType::Player {
                    self.grid[tail.1 as usize][tail.0 as usize] = CellType::Empty;
                }
            }
        }

        // Pre-compute new positions for both cycles (immutable borrows).
        // A slipstream-frozen player holds position: no move, no collision.
        let (player_new, player_crashed) = if player_frozen {
            (self.cycles[self.player].head, false)
        } else {
            let cycle = &self.cycles[self.player];
            let (dx, dy) = cycle.direction.as_delta();
            let new_x = (cycle.head.0 as i16 + dx)
                .max(0)
                .min((self.width - 1) as i16) as u16;
            let new_y = (cycle.head.1 as i16 + dy)
                .max(0)
                .min((self.height - 1) as i16) as u16;

            // The CPU's tail retracts later this frame (after cpu_decide), so
            // its tail-tip cell is already as good as vacated. The CPU's own
            // crash check runs after both retractions and gets that courtesy
            // for free; without this exemption the player dies making the
            // exact move the CPU survives.
            let cpu_tail_vacating = {
                let opp = &self.cycles[1];
                opp.alive
                    && opp.positions.len() > 1
                    && opp.pending_growth == 0
                    && opp.positions.last() == Some(&(new_x, new_y))
                    && !self.bombs.iter().any(|b| b.x == new_x && b.y == new_y)
            };
            let crashed = !self.passable(new_x, new_y) && !cpu_tail_vacating;

            ((new_x, new_y), crashed)
        };

        // Player collision
        if player_crashed {
            // Record the fatal choice into the opponent corpus BEFORE the
            // early return below. This path used to skip recording entirely —
            // survivor bias: the memory omitted precisely the terminal
            // mistakes and over-aggressive choices that are most informative
            // about how this human loses. "At this kind of junction they
            // drive into a wall" is exactly the read a dominating CPU wants.
            crate::cpu_ai::record_player_episode(
                &mut self.cpu_brain,
                player_ctx_pre,
                player_dir_this_frame,
                true, // a death is always an informative frame
            );
            // What killed the player: bomb cell, wall, own trail, or CPU trail.
            if self.death_cause.is_none() {
                self.death_cause = Some(
                    if self
                        .bombs
                        .iter()
                        .any(|b| b.x == player_new.0 && b.y == player_new.1)
                    {
                        DeathCause::BombBlast
                    } else {
                        match self.grid[player_new.1 as usize][player_new.0 as usize] {
                            CellType::Player => DeathCause::OwnTrail,
                            CellType::CPU => DeathCause::EnemyTrail,
                            _ => DeathCause::Wall,
                        }
                    },
                );
            }
            // True head-on: the player rams the CPU's head cell while the CPU
            // simultaneously steps into the player's head cell. Both die -> draw
            // (the sequential player-first check would otherwise hand the CPU
            // the win). The CPU's intended move is taken from its current
            // direction; cpu_decide preserves it in every non-turn frame.
            let cpu_rams_back = self.cycles[1].alive && player_new == self.cycles[1].head && {
                // (see below: the autopilot branch decides escape simulation)
                let cy = &self.cycles[1];
                let (dx, dy) = cy.direction.as_delta();
                let nx = (cy.head.0 as i16 + dx).max(0).min((self.width - 1) as i16) as u16;
                let ny = (cy.head.1 as i16 + dy).max(0).min((self.height - 1) as i16) as u16;
                (nx, ny) == self.cycles[0].head
            };
            // Same-frame sibling: the CPU would also die this frame. The
            // self-driving CPU turns away from a blocked cell (cpu_decide
            // picks from legal_directions), so "straight ahead is blocked" is
            // a routine wall-follow turn frame, not a death — probing only the
            // heading banked false draws on ordinary player crashes. The CPU
            // dies only when truly boxed in: no non-reverse direction is
            // survivable once its own about-to-vacate tail tip is discounted.
            // Scripted opponents (cpu_autopilot off) keep their heading, so
            // for them the straight-ahead probe remains the accurate test.
            let cpu_would_crash = self.cycles[1].alive && {
                let cy = &self.cycles[1];
                // A ghost replay of a live (autopilot) round must resolve
                // simultaneous crashes with the SAME policy the live round
                // used — physics must not depend on who is steering.
                if self.cpu_autopilot || self.script.is_some() {
                    let vacating = if cy.positions.len() > 1 && cy.pending_growth == 0 {
                        cy.positions.last().copied()
                    } else {
                        None
                    };
                    let back = match cy.direction {
                        Direction::Up => Direction::Down,
                        Direction::Down => Direction::Up,
                        Direction::Left => Direction::Right,
                        Direction::Right => Direction::Left,
                    };
                    ![
                        Direction::Up,
                        Direction::Down,
                        Direction::Left,
                        Direction::Right,
                    ]
                    .into_iter()
                    .filter(|&d| d != back)
                    .any(|d| {
                        let (dx, dy) = d.as_delta();
                        let nx = cy.head.0 as i16 + dx;
                        let ny = cy.head.1 as i16 + dy;
                        if nx < 0 || ny < 0 || nx >= self.width as i16 || ny >= self.height as i16 {
                            return false;
                        }
                        let (nx, ny) = (nx as u16, ny as u16);
                        self.passable(nx, ny)
                            || (vacating == Some((nx, ny))
                                && !self.bombs.iter().any(|b| b.x == nx && b.y == ny))
                    })
                } else {
                    let (dx, dy) = cy.direction.as_delta();
                    let nx = (cy.head.0 as i16 + dx).max(0).min((self.width - 1) as i16) as u16;
                    let ny = (cy.head.1 as i16 + dy).max(0).min((self.height - 1) as i16) as u16;
                    !self.passable(nx, ny)
                }
            };
            self.add_impact_particles(player_new.0, player_new.1, self.cycles[self.player].color);
            if cpu_rams_back {
                self.death_cause = Some(DeathCause::HeadOn);
            }
            if cpu_rams_back || cpu_would_crash {
                self.add_impact_particles(
                    self.cycles[1].head.0,
                    self.cycles[1].head.1,
                    self.cycles[1].color,
                );
                self.cycles[1].alive = false;
                self.winner = None;
            } else {
                self.winner = Some(1);
            }
            self.cycles[self.player].alive = false;
            self.game_over = true;
            play_death_riff(100);
            // ADR-023: the beam phase runs on EVERY movement-resolving exit.
            // (A crashing head never occupied the line, and the un-moved CPU
            // entered nothing — this is the uniformity call, not a kill site.)
            self.reconcile_beams();
            self.age_beam_fx();
            // In-flight bolts and a bomb at fuse 0 still get their frame-end
            // step; either may kill the surviving CPU and turn the loss into
            // a draw (bombs alone used to tick while bolts froze mid-air).
            self.advance_projectiles();
            self.tick_flames();
            self.tick_bombs();
            return false;
        }

        // Player food collection — from the multi-food tray.
        let player_color = self.cycles[self.player].color;
        let mut player_food_val: u8 = 0;
        if let Some(idx) = self
            .food_items
            .iter()
            .position(|&(fx, fy, _)| (fx, fy) == player_new)
        {
            let (_, _, v) = self.food_items.remove(idx);
            player_food_val = v;
            self.cpu_brain.frames_since_food = 0;
            self.cycles[self.player].score += player_food_val as u32;
            self.score += player_food_val as u32 * 10;
            self.food_eaten_total += player_food_val as u32;
        }

        // Move player head (grow by the food value eaten)
        if !player_frozen {
            let cycle = &mut self.cycles[self.player];
            cycle.head = player_new;
            cycle.positions.insert(0, player_new);
            // The tail was kept this frame iff growth was already owed at
            // retraction time; that kept tail consumes one credit even on a
            // frame that also eats (eating used to skip the payment, granting
            // one segment more than the food value).
            let tail_kept = cycle.pending_growth > 0;
            if player_food_val > 0 {
                cycle.pending_growth += player_food_val as u32;
            }
            if tail_kept {
                cycle.pending_growth -= 1;
            }
            self.grid[player_new.1 as usize][player_new.0 as usize] = CellType::Player;
        }

        // Player power-up pickup (grid cell gets overwritten by the head marker below,
        // same lifecycle as food).
        if let Some(idx) = self
            .powerups
            .iter()
            .position(|&(px, py, _)| (px, py) == player_new)
        {
            let (_, _, kind) = self.powerups.remove(idx);
            self.cycles[self.player].held_powerup = Some(kind);
            play_beep(SfxKind::PowerUp, 1560, 40);
        }

        if player_food_val > 0 {
            self.food_eaten_by[0] += player_food_val as u32;
            self.add_impact_particles(player_new.0, player_new.1, player_color);
            play_food_pickup(player_food_val);
        }

        // Retract CPU tail before the CPU decides (unless owed growth cells):
        // cpu_decide, the crash check, and the learning encoder all see the
        // same post-retract world — previously the AI treated its own
        // about-to-vacate tail tip as an illegal move. Own-marker guard, same
        // as the player's retraction: the player may have legally entered
        // this cell already.
        if !cpu_frozen {
            let cycle = &mut self.cycles[1];
            if cycle.positions.len() > 1 && cycle.pending_growth == 0 {
                let tail = cycle.positions.pop().unwrap();
                if self.grid[tail.1 as usize][tail.0 as usize] == CellType::CPU {
                    self.grid[tail.1 as usize][tail.0 as usize] = CellType::Empty;
                }
            }
        }

        // --- Learn from the player's move, THEN forecast their next one ---
        //
        // This runs BEFORE the CPU decides, and that ordering is the whole
        // point. It used to sit at the end of the frame, so `cpu_decide` had
        // no fresh forecast to read and fell back to `cpu_telemetry.scored` —
        // the forecast for the frame ALREADY IN PROGRESS, whose true answer is
        // sitting in `cycles[0].direction` by then. The CPU was steering on a
        // restatement of something it could already see, which is why the
        // opponent model influenced almost nothing.
        //
        // Safe here: the player has already moved this frame (their head and
        // trail are written above), and everything below consumes only
        // `player_ctx_pre` / `player_dir_this_frame`, both captured at frame
        // start. The forecast therefore targets t+1 from the board the player
        // will actually choose on.
        // --- Opponent Model Learning ---
        // Store (pre-move player context → direction the player took), encoded
        // at frame start before the tail saw this frame's move.
        // (rps-ai learns from what the HUMAN played next, not what the AI did.)
        crate::cpu_ai::record_player_episode(
            &mut self.cpu_brain,
            player_ctx_pre,
            player_dir_this_frame,
            player_had_choice,
        );
        // Player move history feeding the CPU tail (trailing-match bonus).
        self.cpu_brain.record_player_move(player_dir_this_frame);

        // --- The rps-ai ensemble: score last frame's predictions against the
        // actual move, then refresh every model's prediction for next frame
        // (counterfactual recording — rps-ai stores model0..5 every round).
        if self.cpu_autopilot || self.shadow_learning {
            let (pending, _premask_active, _premask_conf, intent_targets) =
                crate::cpu_ai::compute_ensemble(self, &self.cpu_brain);
            // Errand hysteresis for the eat/arm intent models — see
            // `CpuBrain::intent_targets`.
            self.cpu_brain.intent_targets = intent_targets;

            // The heading the player will be travelling when they choose next.
            // `snapshot_direction` has not run yet, so this frame's move lives
            // in `direction` — and the next frame's reversal ban is relative to
            // it, not to `prev_direction`.
            let heading = self.cycles[self.player].direction;
            let legal_next = crate::cpu_ai::legal_options_from(self, self.player, heading);
            let turn_prior = self.cpu_brain.opp_brain.turn_prior();
            // The break-pattern read outranks the flat prior once it has
            // enough events to have earned an opinion.
            let pattern_left = if self.cpu_brain.turn_pattern.events
                >= crate::cpu_ai::VOMM_MIN_EVENTS
            {
                Some(self.cpu_brain.turn_pattern.p_left())
            } else {
                None
            };

            // Mask EVERY model's prediction, not just the winner's.
            //
            // The ensemble used to store raw model output in `pending` and
            // score that, while gameplay published the masked forecast — so
            // model SELECTION was optimising a different prediction from the
            // one actually acted on. A model could be crowned for predicting
            // moves into walls, and one that lost on raw output could be the
            // better predictor of what the game really published. Scoring what
            // is published is the point of scoring at all.
            let masked: Vec<Option<Direction>> = pending
                .iter()
                .map(|&p| {
                    crate::cpu_ai::mask_to_legal(p, &legal_next, heading, &turn_prior, pattern_left)
                })
                .collect();

            // Selection happens AFTER masking, among models that actually
            // speak this frame — a silent model must never drive with its
            // historical hit-rate as phantom confidence (codex finding).
            let (active, confidence) = crate::cpu_ai::select_active(&self.cpu_brain, &masked);

            // THE TURN BOOK's publish decision (ADR-020 stage 2). Hazard
            // context from the post-move board this forecast targets;
            // side pick sealed gate-independently (aT trains either way);
            // the derived gate decides which book's answer ships.
            let straight_next = legal_next.contains(&heading);
            let (book_source, book_dir, book_conf) = {
                let (px, py) = self.cycles[self.player].head;
                let (cx, cy) = self.cycles[1].head;
                let dist = ((px as i32 - cx as i32).abs()
                    + (py as i32 - cy as i32).abs()) as u32;
                let cpu_close =
                    dist <= 12 && dist < self.cpu_brain.prev_pc_dist.max(1);
                self.cpu_brain.prev_pc_dist = dist;
                let nearest = self
                    .food_items
                    .iter()
                    .min_by_key(|&&(fx, fy, _)| {
                        (fx as i32 - px as i32).abs() + (fy as i32 - py as i32).abs()
                    })
                    .map(|&(fx, fy, _)| (fx, fy));
                let fside = crate::cpu_ai::food_side(px, py, heading, nearest);
                let cell = crate::cpu_ai::hazard_cell(
                    self.cpu_brain.gap_since_voluntary,
                    fside,
                    self.cpu_brain.frames_since_food <= 3,
                    cpu_close,
                );
                let side = self.cpu_brain.class_books.side_pick(&masked, heading);
                if self.cpu_brain.pending_book.is_some() {
                    // A record the take-side never saw (frozen-player frames
                    // produce without consuming) — receipt the loss.
                    self.funnel.pend_dropped += 1;
                }
                self.cpu_brain.pending_book = Some(crate::cpu_ai::PendingBook {
                    target_frame: self.frame_count + 1,
                    cell,
                    side: side.map(|(_, d)| d),
                    food_side_dir: match fside {
                        crate::cpu_ai::FoodSide::Left => {
                            Some(crate::cpu_ai::left_turn(heading))
                        }
                        crate::cpu_ai::FoodSide::Right => {
                            Some(crate::cpu_ai::right_turn(heading))
                        }
                        crate::cpu_ai::FoodSide::Ahead => None,
                    },
                });
                // Fold the book's precommitment (target frame, context
                // cell, side) into the reveal chain. This is an INTERNAL
                // sequencing invariant — it pins that the pick existed
                // before the outcome inside this build — not a
                // third-party-verifiable commitment: the public seal
                // covers only the published forecast, and the precommit
                // record is not exposed in the disclosed state.
                self.seal_chain = crate::cpu_ai::fnv1a64(&[
                    self.seal_chain.to_le_bytes().as_slice(),
                    (self.frame_count + 1).to_le_bytes().as_slice(),
                    &[cell as u8, side.map(|(_, d)| d as u8 + 1).unwrap_or(0)],
                ]
                .concat());
                let h = self.cpu_brain.class_books.hazard(cell);
                // The gate only ever matters on frames where straight is a
                // real alternative — forced turns already belong to the
                // legal mask and the turn prior.
                if straight_next && self.cpu_brain.class_books.gate(h) {
                    match side {
                        Some((src, d)) => (
                            Some(src),
                            Some(d),
                            self.cpu_brain.class_books.a_turn(),
                        ),
                        None => (None, None, 0.0),
                    }
                } else {
                    (None, None, 0.0)
                }
            };

            let e = &mut self.cpu_brain.ensemble;
            for (slot, m) in e.pending.iter_mut().zip(masked.iter()) {
                *slot = *m;
            }
            let (active, confidence, from_book) = match (book_source, book_dir) {
                (Some(src), Some(_)) => (src, book_conf, true),
                _ => (active, confidence, false),
            };
            e.active = active;
            e.confidence = confidence;
            e.predicted_dir = if from_book { book_dir } else { e.pending[active] };
            self.cpu_brain.last_opp_prediction = e.predicted_dir;
            // Commit the prediction BEFORE the player's input for that frame
            // exists. The salt is a pure function of (seal_seed, frame) — it
            // deliberately never touches the game RNG, because drawing from
            // that stream would shift food spawns and explore rolls and
            // silently invalidate every seeded benchmark in the repo.
            let target = self.frame_count + 1;
            let salt = crate::cpu_ai::seal_salt(self.seal_seed, target);
            let predicted = e.predicted_dir;
            self.cpu_telemetry.next_forecast = Some(crate::cpu_ai::ForecastTrace {
                target_frame: target,
                source: active,
                predicted,
                confidence,
                book: if from_book { 1 } else { 0 },
                seal: crate::cpu_ai::seal_commit(salt, predicted, target),
            });
        }


        // CPU AI — faithful k-NN memory opponent (rps-ai mechanism).
        // Only runs when the CPU drives itself: scripted bench opponents
        // (cpu_autopilot = false) keep their externally-steered heading and
        // leave no episodes in the learner's brain.
        let mut cpu_obs = None;
        // THE BEATABLE OPENING (ADR-018): an unread CPU is slow-witted — it
        // re-decides only every Nth frame, like the casual human it hasn't
        // read yet, and reading the player restores tick-perfect wits.
        // Held headings + thinned floors = genuine, killable mistakes.
        // Explainable in one line: "it starts slow and sloppy; learning you
        // is what makes it sharp." Never applies to replays (scripted).
        let open_k = {
            let t = crate::tuning::tuning();
            1 + ((t.open_latency - 1.0).max(0.0) * (1.0 - self.discipline_sharpness())).round()
                as u32
        };
        // A doze never overrides WALL reflexes — play-tested, a CPU that
        // holds heading into a static wall reads as broken, not casual (the
        // contract is "solid basics that hasn't read you yet"). But trails
        // deliberately do NOT wake it: a fixated casual player rams trails
        // mid-chase, and an unsharp CPU dying into the trail YOU laid is the
        // classic earned Tron kill — beatable through play, never through
        // watching it faceplant scenery.
        // SLIPSTREAM REACTION TAX (owner spec): at 4× world clock a human's
        // turning accuracy degrades — the CPU's must too, or slip time
        // hands it superhuman play for free. While the fast clock runs,
        // the arena-side CPU re-decides only every 4th frame — the SAME
        // decisions-per-second it had at normal time, in a body moving 4×
        // — holding heading between decisions under the standard doze
        // semantics: wall and ring reflexes stay on, trails stay UNSEEN.
        // Ramming a trail at speed is precisely the turning-accuracy
        // failure a human suffers, symmetrically priced.
        let slip_clock = self.arena_version >= 4
            && ((self.cycles[0].alive && self.cycle_in_corridor(0))
                || (self.cycles[1].alive && self.cycle_in_corridor(1)));
        let slip_lag =
            slip_clock && !cpu_frozen && !self.frame_count.is_multiple_of(4);
        let cpu_dozing = self.cpu_autopilot
            && ((open_k > 1 && !self.frame_count.is_multiple_of(open_k)) || slip_lag)
            && {
            let cy = &self.cycles[1];
            let (dx, dy) = cy.direction.as_delta();
            let nx = cy.head.0 as i16 + dx;
            let ny = cy.head.1 as i16 + dy;
            let wall_ahead = nx < 0
                || ny < 0
                || nx >= self.width as i16
                || ny >= self.height as i16
                || self.grid[ny as usize][nx as usize] == CellType::Wall;
            // OWN mine ahead wakes even a dozy driver — you always know
            // where your own plant is (self-knowledge, not sharpness).
            // ENEMY mines stay invisible to the doze: being fooled by a
            // disguise is exactly what the doze is for. (v6's longer
            // decoy fuse made own-mine doze deaths a measured warm-arm
            // failure mode: BombBlast under held headings.)
            let own_mine_ahead = nx >= 0
                && ny >= 0
                && self
                    .bombs
                    .iter()
                    .any(|b| b.owner == 1 && b.x == nx as u16 && b.y == ny as u16);
            // World v9 (both consultants, adopted narrowly): holding
            // heading into an open cell with ZERO onward exits — a
            // one-step dead-end pocket — wakes the doze exactly like a
            // wall would. Same static-geometry class as the wall/ring/
            // own-mine reflexes; a trail DIRECTLY ahead stays invisible
            // (stepping into it remains the classic earned kill), and
            // the wake only hands control to the normal decision — it
            // never redirects a decided step. (Receipt: dozy CPUs died
            // pocketed at the interior-ring corners between the
            // persona's wall lap and the wall — 16 of 26 warm losses,
            // the same cells and frames since v6, amplified by every
            // physics change.)
            let pocket_ahead = self.arena_version >= 9
                && !wall_ahead
                && nx >= 0
                && ny >= 0
                && self.passable(nx as u16, ny as u16)
                && {
                    let (hx0, hy0) = (cy.head.0 as i16, cy.head.1 as i16);
                    [(1i16, 0i16), (-1, 0), (0, 1), (0, -1)].iter().all(
                        |&(ddx, ddy)| {
                            let (px, py) = (nx + ddx, ny + ddy);
                            (px, py) == (hx0, hy0)
                                || px < 0
                                || py < 0
                                || px >= self.width as i16
                                || py >= self.height as i16
                                || !self.passable(px as u16, py as u16)
                        },
                    )
                };
            !wall_ahead
                && !own_mine_ahead
                && !pocket_ahead
                && !crate::cpu_ai::ring_doomed_step(self, cy.head, cy.direction)
        };
        let cpu_dir = if cpu_frozen {
            // Slipstream-frozen: no decision, no discharge — held heading.
            self.cycles[1].direction
        } else if self.cpu_autopilot && cpu_dozing {
            // Attention lapse: hold the heading, no decisions, no firing.
            self.cycles[1].direction
        } else if self.cpu_autopilot {
            // The CPU fires a held power-up when the heuristic sees a good
            // shot. The laser is special-cased: it kills the same frame it
            // fires, so it charges visibly for LASER_TELEGRAPH_FRAMES first
            // (red embers along the beam) — crossing the CPU's firing line is
            // dodgeable instead of an unannounced instant death.
            let mut wants_fire = crate::cpu_ai::should_fire(self, 1);
            // ADR-021 Kata 5 (#2): the bait book's supply-generator. The
            // geometric gate stays the incumbent; a mine the CPU has sat
            // on for 40+ frames may be placed ONCE per round anyway —
            // mine only (placement is the least directly lethal weapon
            // and the disguise is the thing being measured), never in a
            // player's first three rounds (the novice opening is a priced
            // contract), and only with room to leave the trigger ring.
            // Player-independent, bounded, disclosed — an exploration
            // floor, not learned aggression.
            if !wants_fire
                && self.cycles[1].held_powerup == Some(PowerUpKind::Bomb)
                && self.cpu_brain.ledgers.mine_held_streak >= 40
                && !self.cpu_brain.ledgers.explore_used
                && self.cpu_brain.ledgers.rounds_seen >= 3
                && crate::cpu_ai::legal_directions(self, &self.cycles[1]).len() >= 2
            {
                wants_fire = true;
                self.cpu_brain.ledgers.explore_used = true;
            }
            if self.cycles[1].held_powerup == Some(PowerUpKind::Bomb) {
                self.cpu_brain.ledgers.mine_held_streak =
                    self.cpu_brain.ledgers.mine_held_streak.saturating_add(1);
            } else {
                self.cpu_brain.ledgers.mine_held_streak = 0;
            }
            if let Some(kind) = self.cycles[1].held_powerup {
                // Held/gate opportunity only — an actual FIRE is recorded at
                // the discharge site below (codex: a charging laser's
                // telegraph frames were being counted as fires).
                self.cpu_brain.ledgers.note_weapon(kind, wants_fire, false);
            }
            let holding_laser = self.cycles[1].held_powerup == Some(PowerUpKind::Laser);
            let fire_now = if holding_laser {
                if wants_fire {
                    self.cpu_laser_charge += 1;
                    let (hx, hy) = self.cycles[1].head;
                    let (dx, dy) = self.cycles[1].direction.as_delta();
                    let beam = self.beam_cells(hx, hy, dx, dy);
                    for &(bx, by) in &beam {
                        self.particles.push(Particle {
                            x: bx as f32,
                            y: by as f32,
                            vx: 0.0,
                            vy: 0.0,
                            lifetime: 3,
                            color: (255, 70, 70),
                        });
                    }
                    self.cpu_laser_charge >= LASER_TELEGRAPH_FRAMES
                } else {
                    // Target left the firing line — the charge resets.
                    self.cpu_laser_charge = 0;
                    false
                }
            } else {
                wants_fire
            };
            if fire_now {
                self.cpu_laser_charge = 0;
                if let Some(kind) = self.cycles[1].held_powerup {
                    self.cpu_brain.ledgers.note_weapon_fired(kind);
                }
                self.fire_powerup(1);
                if self.game_over {
                    // fire_powerup already ran the frame-end draw-parity pass.
                    return false;
                }
            }
            let dir = crate::cpu_ai::cpu_decide(self);
            // Learn from what the decision actually saw: encode the situation
            // BEFORE the turn is applied and before the head moves. (The old
            // code encoded after the move — a one-frame shift that paired
            // every pre-move decision with a post-move situation.)
            cpu_obs = Some(crate::cpu_ai::encode_situation(self, &self.cpu_brain));
            // Survival counter = frames survived on the current heading. It
            // resets on a turn; previously it was reset EVERY frame, so the
            // reward's survival term was a constant 1 and the signal was dead.
            let turned = dir != self.cycles[1].direction;
            self.cycles[1].change_direction(dir);
            if turned {
                self.frames_since_cpu_move = 0;
            }
            // Ghost recorder, kind 2: the CPU's decided direction, at the
            // decision site (pre-move, so fatal turns are captured), change
            // events only.
            if self.script.is_none() && self.replay.last_cpu_dir != Some(dir) {
                self.replay.last_cpu_dir = Some(dir);
                self.replay.events.push((self.frame_count, 2, dir as u8));
            }
            dir
        } else if self.script.is_some() {
            // GHOST REPLAY: consume this frame's recorded CPU events at the
            // exact site the live path produced them — fire first if the
            // stream says the CPU fired this frame, then its direction.
            let frame = self.frame_count;
            if let Some(script) = self.script.as_mut() {
                if script.take(frame, 3).is_some() {
                    self.fire_powerup(1);
                    if self.game_over {
                        return false;
                    }
                }
            }
            let dir = self
                .script
                .as_mut()
                .and_then(|script| script.take(frame, 2))
                .map(|d| match d {
                    0 => Direction::Up,
                    1 => Direction::Down,
                    2 => Direction::Left,
                    _ => Direction::Right,
                })
                .unwrap_or(self.cycles[1].direction);
            self.cycles[1].change_direction(dir);
            dir
        } else {
            self.cycles[1].direction
        };

        // Recompute CPU position after AI decision. Slipstream-frozen:
        // holds position, no collision of its own making.
        let (cpu_new, cpu_crashed) = if cpu_frozen {
            (self.cycles[1].head, false)
        } else {
            let cycle = &self.cycles[1];
            let (dx, dy) = cycle.direction.as_delta();
            let new_x = (cycle.head.0 as i16 + dx)
                .max(0)
                .min((self.width - 1) as i16) as u16;
            let new_y = (cycle.head.1 as i16 + dy)
                .max(0)
                .min((self.height - 1) as i16) as u16;

            let crashed = !self.passable(new_x, new_y);

            ((new_x, new_y), crashed)
        };

        let cpu_color = self.cycles[1].color;

        // CPU collision
        if cpu_crashed {
            if self.death_cause.is_none() {
                self.death_cause = Some(
                    if self
                        .bombs
                        .iter()
                        .any(|b| b.x == cpu_new.0 && b.y == cpu_new.1)
                    {
                        DeathCause::BombBlast
                    } else {
                        match self.grid[cpu_new.1 as usize][cpu_new.0 as usize] {
                            CellType::CPU => DeathCause::OwnTrail,
                            CellType::Player => DeathCause::EnemyTrail,
                            _ => DeathCause::Wall,
                        }
                    },
                );
            }
            // Both entered the same cell this frame: the player's crash check
            // ran first and the cell was empty then, so the player moved in,
            // and the CPU then stepped into the same cell. Both die -> draw.
            let same_cell = self.cycles[0].alive && cpu_new == self.cycles[0].head;
            if same_cell {
                self.death_cause = Some(DeathCause::HeadOn);
            }
            // Learn: the chosen direction died immediately (reward 0). The
            // episode uses the same pre-move observation as survival episodes
            // so crash and survival data share one pairing convention.
            // Scripted CPUs (autopilot off) record nothing — their crashes
            // are not the learner's decisions.
            if let Some(obs) = cpu_obs {
                crate::cpu_ai::record_episode(&mut self.cpu_brain, obs, cpu_dir, 0, 0);
            }
            self.add_impact_particles(cpu_new.0, cpu_new.1, self.cycles[1].color);
            if same_cell {
                self.add_impact_particles(
                    self.cycles[0].head.0,
                    self.cycles[0].head.1,
                    self.cycles[0].color,
                );
                self.cycles[0].alive = false;
            }
            self.cycles[1].alive = false;
            self.game_over = true;
            self.winner = if same_cell { None } else { Some(0) };
            play_death_riff(100);
            // ADR-023: the beam phase runs on EVERY movement-resolving exit —
            // the player may have stepped into a live beam this same frame,
            // turning this into a draw.
            self.reconcile_beams();
            self.age_beam_fx();
            // In-flight bolts and a bomb at fuse 0 still get their frame-end
            // step; either may kill the surviving player and turn the CPU win
            // into a draw (bombs alone used to tick while bolts froze).
            self.advance_projectiles();
            self.tick_flames();
            self.tick_bombs();
            return false;
        }

        // CPU food collection — from the multi-food tray.
        let mut cpu_food_val: u8 = 0;
        if let Some(idx) = self
            .food_items
            .iter()
            .position(|&(fx, fy, _)| (fx, fy) == cpu_new)
        {
            let (_, _, v) = self.food_items.remove(idx);
            cpu_food_val = v;
            self.food_eaten_by[1] += cpu_food_val as u32;
            self.cycles[1].score += cpu_food_val as u32;
            self.score += cpu_food_val as u32 * 10;
            self.food_eaten_total += cpu_food_val as u32;
        }

        // Move CPU head (grow by the food value eaten)
        if !cpu_frozen {
            let cycle = &mut self.cycles[1];
            cycle.head = cpu_new;
            cycle.positions.insert(0, cpu_new);
            // Same kept-tail growth accounting as the player block above.
            let tail_kept = cycle.pending_growth > 0;
            if cpu_food_val > 0 {
                cycle.pending_growth += cpu_food_val as u32;
            }
            if tail_kept {
                cycle.pending_growth -= 1;
            }
            self.grid[cpu_new.1 as usize][cpu_new.0 as usize] = CellType::CPU;
            cycle.score += 1;
        }

        // CPU power-up pickup.
        if let Some(idx) = self
            .powerups
            .iter()
            .position(|&(px, py, _)| (px, py) == cpu_new)
        {
            let (_, _, kind) = self.powerups.remove(idx);
            self.cycles[1].held_powerup = Some(kind);
            play_beep(SfxKind::PowerUp, 1560, 40);
        }

        // If we've cleared the tray, spawn a fresh 1-5. This runs AFTER both
        // head markers are on the grid — refilling between the CPU's food
        // collection and its head write could spawn food on the CPU's new
        // head cell, leaving an invisible orphaned tray entry.
        if self.food_items.is_empty() {
            self.generate_food_items();
        }

        if cpu_food_val > 0 {
            self.add_impact_particles(cpu_new.0, cpu_new.1, cpu_color);
            play_food_pickup(cpu_food_val);
        }

        // Record this round for learning — faithful to rps-ai: learn from what
        // happened, with a monotonic seq. The CPU survived `frames_since_cpu_move`
        // frames on this direction and ate cpu_food_val food.
        if let Some(obs) = cpu_obs {
            self.frames_since_cpu_move += 1;
            crate::cpu_ai::record_episode(
                &mut self.cpu_brain,
                obs,
                cpu_dir,
                self.frames_since_cpu_move,
                cpu_food_val,
            );
        }

        // Track each cycle's last-EXECUTED direction (corner/transition
        // features and the 180 latch read prev_direction). World v5: a
        // frozen worm executed nothing — snapshotting its latched press
        // would corrupt the reversal ban's anchor (see ARENA_VERSION).
        if self.arena_version < 5 || !player_frozen {
            self.cycles[0].snapshot_direction();
        }
        if self.arena_version < 5 || !cpu_frozen {
            self.cycles[1].snapshot_direction();
        }


        // WORLD v7 (ADR-023): the post-move half of the laser's dual test —
        // one common hazard phase; every movement-resolving exit path calls
        // the same method (codex: no early return may skip it).
        if self.reconcile_beams() {
            self.age_beam_fx();
            // Bombs at fuse 0 still get their frame-end tick — a blast can
            // turn this into a (deeper) draw, same as every death site.
            self.tick_flames();
            self.tick_bombs();
            return false;
        }
        self.age_beam_fx();

        // Live projectiles and planted bombs (can end the game). Under
        // world v3 the projectile step already happened at frame START;
        // stepping again here would double their speed.
        if self.arena_version < 3 {
            self.advance_projectiles();
        }
        self.tick_flames();
        self.tick_bombs();
        if self.game_over {
            return false;
        }

        // Sudden death: from SUDDEN_DEATH_START the arena walls close inward
        // one ring every SUDDEN_DEATH_INTERVAL frames, so no round can orbit
        // forever. The next ring to close is telegraphed with ember
        // particles for its last 30 frames.
        if self.has_corridor() && self.time >= SUDDEN_DEATH_START {
            let elapsed = self.time - SUDDEN_DEATH_START;
            let max_level = self.sudden_death_max_level();
            let target = ((elapsed / SUDDEN_DEATH_INTERVAL) as u16).min(max_level);
            while self.shrink_level < target && !self.game_over {
                self.shrink_level += 1;
                let base = self.sudden_death_base();
                self.close_ring(base + self.shrink_level);
            }
            if self.game_over {
                return false;
            }
            if self.shrink_level < max_level {
                let next_in = SUDDEN_DEATH_INTERVAL - (elapsed % SUDDEN_DEATH_INTERVAL);
                if next_in <= 30 && self.time.is_multiple_of(3) {
                    self.ring_ember_particles(self.sudden_death_base() + self.shrink_level + 1);
                }
            }
        }

        // Power-up spawn timer.
        if self.powerup_timer > 0 {
            self.powerup_timer -= 1;
        } else {
            self.spawn_powerup();
            self.powerup_timer = self.rng_range(80..160);
        }

        true
    }

    /// The last shrink level sudden death will reach on this board. Single
    /// source of truth — the schedule in `update` and the CPU's evacuation
    /// logic must not drift apart.
    pub fn sudden_death_max_level(&self) -> u16 {
        (self.width.min(self.height).saturating_sub(8) / 2).saturating_sub(2)
    }

    /// Frames until the sudden-death ring passing through `(x, y)` seals, or
    /// `None` if no scheduled ring passes through it.
    ///
    /// `close_ring` kills any head standing on the ring it seals, so this is
    /// what lets the CPU leave in time. It matters more than it looks: the
    /// right-hand wall-follow that keeps the CPU alive hugs the inner face of
    /// the ring-2 wall, which is exactly the first ring to close — measured
    /// against a passive opponent, 47 of 100 games ended with the CPU standing
    /// on it at the sealing frame. The survival strategy was the death
    /// sentence, and nothing in the AI knew sudden death existed.
    pub fn ring_seal_eta(&self, x: u16, y: u16) -> Option<u32> {
        if !self.has_corridor() {
            return None;
        }
        let max_level = self.sudden_death_max_level();
        for level in (self.shrink_level + 1)..=max_level {
            let off = self.sudden_death_base() + level;
            // Mirrors close_ring's own bail-out, so we never promise a seal it
            // will decline to perform.
            if self.width <= 2 * off + 4 || self.height <= 2 * off + 4 {
                break;
            }
            let (l, r, t, b) = (off, self.width - 1 - off, off, self.height - 1 - off);
            let on_ring =
                (x == l || x == r || y == t || y == b) && (l..=r).contains(&x) && (t..=b).contains(&y);
            if on_ring {
                let seals_at = SUDDEN_DEATH_START + level as u32 * SUDDEN_DEATH_INTERVAL;
                return Some(seals_at.saturating_sub(self.time));
            }
        }
        None
    }

    /// Sudden death: seal the square ring at wall offset `off` (the ring-2
    /// arena wall sits at off=2). Heads on the ring die (both -> draw); trail
    /// cells consumed by the wall leave their cycle's positions list
    /// (grid/positions lockstep, same rule as detonate); food, power-ups and
    /// bombs on the ring are removed.
    fn close_ring(&mut self, off: u16) {
        if self.width <= 2 * off + 4 || self.height <= 2 * off + 4 {
            return;
        }
        let (l, r, t, b) = (off, self.width - 1 - off, off, self.height - 1 - off);
        let on_ring = |x: u16, y: u16| {
            (x == l || x == r || y == t || y == b) && (l..=r).contains(&x) && (t..=b).contains(&y)
        };
        let mut dead = [false; 2];
        for (c, d) in dead.iter_mut().enumerate() {
            let (hx, hy) = self.cycles[c].head;
            if self.cycles[c].alive && on_ring(hx, hy) {
                *d = true;
            }
        }
        let mut sealed: Vec<(u16, u16)> = Vec::new();
        for x in l..=r {
            sealed.push((x, t));
            sealed.push((x, b));
        }
        for y in (t + 1)..b {
            sealed.push((l, y));
            sealed.push((r, y));
        }
        for &(x, y) in &sealed {
            self.grid[y as usize][x as usize] = CellType::Wall;
            self.food_items.retain(|&(fx, fy, _)| (fx, fy) != (x, y));
            self.powerups.retain(|&(px, py, _)| (px, py) != (x, y));
            self.bombs.retain(|bb| (bb.x, bb.y) != (x, y));
        }
        for c in &mut self.cycles {
            c.positions.retain(|p| !sealed.contains(p));
        }
        if dead[0] || dead[1] {
            if self.death_cause.is_none() {
                self.death_cause = Some(DeathCause::Wall);
            }
            for (c, &d) in dead.iter().enumerate() {
                if d {
                    let (hx, hy) = self.cycles[c].head;
                    self.add_impact_particles(hx, hy, self.cycles[c].color);
                    self.cycles[c].alive = false;
                }
            }
            self.game_over = true;
            self.winner = match (dead[0], dead[1]) {
                (true, false) => Some(1),
                (false, true) => Some(0),
                _ => None,
            };
            play_death_riff(100);
        }
    }

    /// Ember particles along the ring that is about to close (deterministic —
    /// no RNG, so seeded games stay reproducible).
    fn ring_ember_particles(&mut self, off: u16) {
        if self.width <= 2 * off + 4 || self.height <= 2 * off + 4 {
            return;
        }
        let (l, r, t, b) = (off, self.width - 1 - off, off, self.height - 1 - off);
        let mut cells: Vec<(u16, u16)> = Vec::new();
        for x in (l..=r).step_by(2) {
            cells.push((x, t));
            cells.push((x, b));
        }
        for y in (t..=b).step_by(2) {
            cells.push((l, y));
            cells.push((r, y));
        }
        for (x, y) in cells {
            self.particles.push(Particle {
                x: x as f32,
                y: y as f32,
                vx: 0.0,
                vy: 0.0,
                lifetime: 4,
                color: (255, 90, 30),
            });
        }
    }

    pub fn change_direction(&mut self, new_dir: Direction) {
        // Post-mortem turns must not exist: the browser keeps steering for a
        // beat after a between-frame laser win, and an accepted turn here
        // would be RECORDED into the finished round's ghost (kimi-k3 #4).
        if self.game_over {
            return;
        }
        // WORLD v10 (owner: "do a better job of collecting keys"): inputs
        // are COLLECTED, not latched. Cheap dedup only; legality is
        // decided at consumption against the heading actually moved.
        // Cap 3, drop-newest (4+ directions in one frame gap carry no
        // coherent intent worth preserving).
        if self.arena_version >= 10 {
            let dup = match self.input_queue.back() {
                Some(PlayerInput::Turn(d)) => *d == new_dir,
                _ => self.input_queue.is_empty()
                    && new_dir == self.cycles[self.player].direction,
            };
            if !dup && self.input_queue.len() < 3 {
                self.input_queue.push_back(PlayerInput::Turn(new_dir));
            }
            return;
        }
        // Latch against the direction actually MOVED last tick
        // (prev_direction, snapshotted at the end of every update), not the
        // pending one: two quick inputs between ticks (Up then Left while
        // moving Right) used to net a 180 into the neck cell — an instant
        // self-kill in both the terminal and browser builds.
        let moved_dir = self.cycles[self.player].prev_direction;
        match (moved_dir, new_dir) {
            (Direction::Up, Direction::Down) | (Direction::Down, Direction::Up) => {}
            (Direction::Left, Direction::Right) | (Direction::Right, Direction::Left) => {}
            _ => {
                self.cycles[self.player].direction = new_dir;
                // Ghost recorder, kind 0: an accepted player turn, in input
                // order, stamped with the last completed frame. Fatal turns
                // are captured here BY CONSTRUCTION — recording happens at
                // acceptance, before any collision can end the frame.
                if self.script.is_none() {
                    self.replay.events.push((self.frame_count, 0, new_dir as u8));
                }
            }
        }
    }

    /// The player's fire input. World v10: joins the ordered input
    /// queue — "turn then fire" discharges along the NEW heading when
    /// the fire reaches the queue head (codex v10 consult: the
    /// turn-then-fire blocker, resolved by plumbing fire through the
    /// same collected-input stream). Pre-v10: immediate discharge.
    pub fn player_fire(&mut self) -> bool {
        if self.game_over {
            return false;
        }
        if self.arena_version >= 10 {
            if self.input_queue.len() < 3 {
                self.input_queue.push_back(PlayerInput::Fire);
            }
            true
        } else {
            self.fire_powerup(self.player)
        }
    }

    /// Recompute the difficulty signal from what the CPU has learned.
    ///
    /// MUST be called after any brain restore as well as at every round
    /// boundary — otherwise a returning player who has been read for twenty
    /// matches faces a CPU reset to tier 1, which is the exact opposite of the
    /// premise.
    ///
    /// The signal is LIFT over the player's own base rate, not raw accuracy.
    /// Raw accuracy cannot drive difficulty here: most moves are "keep going",
    /// so a model scoring 90% may have learned nothing, while 45% against a
    /// 33% baseline is a genuinely strong read. Lift is the only number that
    /// means "it learned you", and it is self-normalising — a player who is
    /// trivially predictable produces a high base rate and cannot inflate the
    /// CPU's aggression by being boring.
    pub fn refresh_read_rate(&mut self) {
        // SIGNIFICANCE-GATED (ADR-020): only evidence that clears the
        // family-wise anytime boundary may drive sharpness — a null player
        // must never wake the CPU on fluctuation, however many channels
        // race. The snapshot taken here is the ONLY earned value in-round
        // consumers may spend (codex round 2: no mid-round latch may open
        // hunts before a boundary check has seen it).
        let base = self.cpu_brain.family_earned_read();
        self.cpu_brain.earned_snapshot = base;
        if base > 0.0 {
            self.cpu_brain.discipline_latched = true;
        }
        // DWELL RELEASE (k3 v9 ruling 2b): a latch that keeps spending
        // ~nothing for K consecutive round boundaries is holding a dead
        // read — the diluted z can hover just above the Schmitt release
        // forever while the player's old habit is long gone. Release is
        // keyed to the spend (harm), and the honest-unlearning claim
        // stays a hard == 0.0.
        {
            // The family spend is max(published, book): the dwell must
            // release EVERY latch funding it, or a 0.02 book residue
            // holds the "read" forever while the published side idles.
            let lr = &self.cpu_brain.lifetime_read;
            let br = &self.cpu_brain.class_books.book_read;
            let latched =
                lr.lat_latched || lr.mc_latched || br.lat_latched || br.mc_latched;
            if latched && base > 0.0 && base < crate::cpu_ai::CpuBrain::SPEND_DWELL_FLOOR
            {
                self.cpu_brain.spend_dwell = self.cpu_brain.spend_dwell.saturating_add(1);
                if self.cpu_brain.spend_dwell
                    >= crate::cpu_ai::CpuBrain::SPEND_DWELL_ROUNDS
                {
                    let lr = &mut self.cpu_brain.lifetime_read;
                    lr.lat_latched = false;
                    lr.mc_latched = false;
                    let br = &mut self.cpu_brain.class_books.book_read;
                    br.lat_latched = false;
                    br.mc_latched = false;
                    self.cpu_brain.spend_dwell = 0;
                    self.cpu_brain.earned_snapshot =
                        self.cpu_brain.family_earned_read();
                }
            } else {
                self.cpu_brain.spend_dwell = 0;
            }
        }
        self.cpu_brain.book_spend_snapshot = self.cpu_brain.class_books.spendable();
        self.cpu_brain.book_authority_snapshot =
            self.cpu_brain.class_books.projection_authority();
        // ADR-021 Kata 4 v2 (rUv rvf-solver's core idea, counting-native):
        // THOMPSON SAMPLING over the two intercepts' posteriors instead of
        // a greedy argmax — principled exploration that keeps measuring
        // the loser without a schedule. One sample per ROUND (authority
        // discipline), drawn deterministically from (seal_seed, rounds) so
        // seeded runs and replays stay bit-identical; normal approximation
        // to the Beta posterior, adequate at the n>=10 maturity floor.
        self.cpu_brain.tactic_prefer_direct = match (
            self.cpu_brain.ledgers.tactic_kill_rate(1),
            self.cpu_brain.ledgers.tactic_kill_rate(0),
        ) {
            (Some(corner), Some(direct)) => {
                let n_c = self.cpu_brain.ledgers.tactic_attempts.iter()
                    .find(|e| e.0 == 1).map(|e| e.1).unwrap_or(1.0);
                let n_d = self.cpu_brain.ledgers.tactic_attempts.iter()
                    .find(|e| e.0 == 0).map(|e| e.1).unwrap_or(1.0);
                let h = crate::cpu_ai::fnv1a64(
                    &[self.seal_seed.to_le_bytes().as_slice(),
                      (self.cpu_brain.portfolio.rounds as u64).to_le_bytes().as_slice()]
                    .concat(),
                );
                // Two unit uniforms from the hash halves -> Box-Muller.
                let u1 = ((h >> 32) as f32 / u32::MAX as f32).clamp(1e-6, 1.0 - 1e-6);
                let u2 = ((h & 0xFFFF_FFFF) as f32 / u32::MAX as f32).clamp(1e-6, 1.0 - 1e-6);
                let r = (-2.0 * u1.ln()).sqrt();
                let z1 = r * (std::f32::consts::TAU * u2).cos();
                let z2 = r * (std::f32::consts::TAU * u2).sin();
                let s_c = corner + z1 * (corner * (1.0 - corner) / (n_c + 1.0)).sqrt();
                let s_d = direct + z2 * (direct * (1.0 - direct) / (n_d + 1.0)).sqrt();
                s_d > s_c
            }
            _ => false,
        };
        // ADR-024: the Boxer perturbation's round-boundary gate. Suppress
        // the choke only when its ledger is MATURE and materially worse
        // than the best plain intercept — a yield, so it can only reduce
        // aggression. Self-recovering: a suppressed arm's decayed attempt
        // mass erodes below the maturity floor and the gate reopens.
        self.cpu_brain.tactic_boxer_ok = {
            let best_intercept = [
                self.cpu_brain.ledgers.tactic_kill_rate(0),
                self.cpu_brain.ledgers.tactic_kill_rate(1),
            ]
            .into_iter()
            .flatten()
            .fold(None::<f32>, |a, r| Some(a.map_or(r, |x| x.max(r))));
            match (self.cpu_brain.ledgers.tactic_kill_rate(4), best_intercept) {
                (Some(boxer), Some(intercept)) => boxer >= 0.5 * intercept,
                _ => true,
            }
        };
        // The active playstyle scales how hard the read is SPENT, never the
        // read itself: cautious plays under its evidence, relentless over it.
        // Survival floors are untouched by every style.
        self.read_rate = (base * self.cpu_brain.portfolio.drive_multiplier()).clamp(0.0, 1.0);
        // HUD tier 1..=5, so the number the player sees and the aggression the
        // CPU spends are the same axis.
        self.difficulty = 1 + (self.read_rate * 4.0).round() as u32;
    }

    pub fn restart(&mut self) {
        // Explicit-size games (browser) keep their dimensions; native games
        // re-read the terminal so resizes take effect.
        let dims = match self.fixed_dims {
            Some((w, h)) => Dimensions {
                width: w,
                height: h,
            },
            None => Dimensions::get_terminal_size(),
        };

        // Session scoreboard: bank the finished game (rps-ai's You-vs-Computer
        // counter) before resetting the board.
        if let Some(w) = self.winner {
            self.session_wins[w] += 1;
        }

        let center_x = dims.width / 2;
        let center_y = dims.height / 2;
        let spacing = 12;

        self.width = dims.width;
        self.height = dims.height;
        self.grid = Self::build_grid(dims.width, dims.height, self.arena_version);
        self.cycles.clear();
        self.cycles.push(LightCycle::new(
            center_x.saturating_sub(spacing),
            center_y,
            Direction::Right,
            (0, 255, 255),
            true,
        ));
        self.cycles.push(LightCycle::new(
            center_x.saturating_add(spacing),
            center_y,
            Direction::Left,
            (255, 0, 255),
            false,
        ));
        self.player = 0;
        self.score = 0;
        self.game_over = false;
        self.particles.clear();
        self.time = 0;
        // Exp3 portfolio: credit the temperament that just played — on-policy,
        // win/draw/loss — and pick the next. The sampling draw hashes
        // (seal_seed, round index) so seeded runs stay bit-identical and the
        // game RNG stream is untouched. Read BEFORE winner is cleared below.
        let reward = match self.winner {
            Some(1) => 1.0,
            None => 0.5,
            _ => 0.0,
        };
        // Shadow/ghost evaluation is OFF-POLICY: the CPU never steered, so
        // crediting a temperament with these outcomes (including a phantom
        // draw before round one) would train the portfolio on games it
        // never played (external review finding).
        if !self.shadow_learning {
            let draw = self.seal_seed ^ ((self.cpu_brain.portfolio.rounds as u64 + 1) << 17);
            self.cpu_brain.portfolio.end_round(reward, draw);
        }
        // ADR-021 Kata 0.1 (codex verification, blocking finding 1): the
        // ledgers finalize AT GAME OVER via finalize_round_ledgers(), which
        // the browser save path calls before persisting — finalizing only
        // here in restart() silently dropped every session's LAST round
        // (its kill credit, its death attribution, its drift summary).
        // restart() now merely CONSUMES an already-finalized record — this
        // call is the idempotent backstop for paths that never saved.
        self.finalize_round_ledgers();
        self.ledgers_finalized = false;

        self.winner = None;
        self.frame_count = 0;
        // Recompute how well the CPU reads this player, once, for the whole
        // round ahead. Difficulty is a HUD tier derived from it — earned by
        // reading you, never by the clock.
        self.refresh_read_rate();
        // Preserve the brain across restarts — this is in-game persistence.
        // rps-ai: "what someone opens with is a habit, it is stored."
        // The brain carries learned patterns from the previous game into the next,
        // so the CPU starts the new game with experience. Only reset the
        // CPU-sequence timer that gates recording (frames_since_cpu_move).
        self.frames_since_cpu_move = 0;
        // rps-ai wipes its per-game record each game: ensemble model scores are
        // per-game (responsive), the k-NN memory beneath persists (the corpus).
        self.cpu_brain.ensemble.reset_scores();
        // The turn book keeps its slow weights, hazard and accuracies —
        // cold-starting class selection every round is the exact failure
        // the owner corpus measured 45 times (ADR-020). Only the fast
        // horizon and the transient per-round context reset.
        self.cpu_brain.class_books.reset_round();
        self.cpu_brain.gap_since_voluntary = 0;
        self.cpu_brain.frames_since_food = 99;
        self.cpu_brain.prev_pc_dist = 0;
        if self.cpu_brain.pending_book.is_some() {
            self.funnel.pend_dropped += 1;
        }
        self.cpu_brain.pending_book = None;
        self.pending_beams.clear();
        self.beam_fx.clear();
        self.input_queue.clear();
        self.flames.clear();
        self.burns = [BurnState::default(), BurnState::default()];
        self.cpu_brain.region_ring.clear();
        self.cpu_brain.last_opp_prediction = None;
        self.cpu_history.clear();
        self.powerups.clear();
        self.projectiles.clear();
        self.bombs.clear();
        self.powerup_timer = 60;
        self.food_eaten_total = 0;
        self.food_eaten_by = [0, 0];
        self.round_read = crate::cpu_ai::ReadRate::default();
        self.seal_chain = 0;
        self.seal_frames = 0;
        self.round_pred_hits = 0;
        self.round_pred_total = 0;
        self.cpu_laser_charge = 0;
        self.shrink_level = 0;
        self.cpu_telemetry = crate::cpu_ai::CpuFrameTelemetry::default();
        self.round_last_cpu_decision = None;
        self.death_cause = None;
        // Fresh per-round seed + ghost log — see `begin_round_replay`. Placed
        // before food generation so the round's entire item stream derives
        // from the recorded seed.
        self.begin_round_replay(None);
        self.generate_food_items();
    }

    /// Apply a browser's newly-available logical arena size at a round
    /// boundary. Active rounds never call this: live viewport changes remain
    /// presentation-only until the user starts the next round.
    pub fn restart_with_size(&mut self, width: u16, height: u16) {
        self.fixed_dims = Some((width, height));
        self.restart();
    }

    /// Reset into the EXACT starting state of a recorded round: same size,
    /// same round seed, same item stream. The caller then re-drives both
    /// worms from the ghost log (`cpu_autopilot` off) and the round replays
    /// bit-for-bit — the world stream is untouched by CPU decisions, which
    /// draw from their own `cpu_rng`.
    pub fn start_recorded_round(
        &mut self,
        seed: u64,
        width: u16,
        height: u16,
        arena: u8,
        events: Vec<(u32, u8, u8)>,
    ) {
        self.fixed_dims = Some((width, height));
        self.arena_version = arena;
        // Never bank a stale winner into the session scoreboard mid-harness.
        self.winner = None;
        self.script = None; // restart() must not consume the incoming script
        self.restart();
        self.script = Some(ReplayScript { events, cursor: 0 });
        // Structural, not caller-side: a script with autopilot left on would
        // silently live-steer the CPU and starve the recorded events.
        self.cpu_autopilot = false;
        // restart() drew its own round seed and spawned food from it; replace
        // both with the recorded stream.
        self.begin_round_replay(Some(seed));
        for (x, y, _) in self.food_items.clone() {
            if self.grid[y as usize][x as usize] == CellType::Food {
                self.grid[y as usize][x as usize] = CellType::Empty;
            }
        }
        self.food_items.clear();
        self.generate_food_items();
    }

    /// Is this cycle's head out in the ring-1 corridor (or standing in a
    /// punched hole)? Drives SLIPSTREAM time.
    pub fn cycle_in_corridor(&self, idx: usize) -> bool {
        if !self.has_corridor() {
            return false;
        }
        let (x, y) = self.cycles[idx].head;
        self.pos_in_corridor(x, y)
    }

    /// Is this cell in the outer corridor (between the frame and the
    /// arena wall — one lane pre-v6, two lanes from v6) or a punched
    /// hole in the wall itself?
    pub fn pos_in_corridor(&self, x: u16, y: u16) -> bool {
        if !self.has_corridor() {
            return false;
        }
        let ring = if self.arena_version >= 6 { 3 } else { 2 };
        let in_ring = (x >= 1 && x < ring)
            || (y >= 1 && y < ring)
            || (x > self.width - 1 - ring && x <= self.width - 2)
            || (y > self.height - 1 - ring && y <= self.height - 2);
        (in_ring && self.grid[y as usize][x as usize] != CellType::Wall)
            || self.grid[y as usize][x as usize] == CellType::Hole
    }

    pub fn player_in_corridor(&self) -> bool {
        self.cycle_in_corridor(self.player)
    }

    pub fn frame_delay(&self) -> Duration {
        // Speed is EARNED BY EATING: every food value point (either cycle)
        // shaves time off the frame, from a relaxed 115ms opening down to the
        // 35ms floor — proportional to the size of the food, not the clock.
        let speedup = (self.food_eaten_total as u64 / 2).min(80);
        let base = 115u64.saturating_sub(speedup).max(35);
        // SLIPSTREAM v2 (owner spec): while anyone is out in the corridor
        // the WORLD CLOCK runs 4× — combined with the corridor worm's
        // 1-in-16 stepping (see update()) that lands the spec exactly:
        // corridor worm 25% of original, arena worm 4×.
        let slip = (self.cycles[0].alive && self.cycle_in_corridor(0))
            || (self.cycles[1].alive && self.cycle_in_corridor(1));
        if slip {
            Duration::from_millis((base / 4).max(9))
        } else {
            Duration::from_millis(base)
        }
    }

    /// 0 = opening crawl, 100 = max speed (HUD + music/sfx intensity).
    pub fn speed_pct(&self) -> u32 {
        let ms = self.frame_delay().as_millis() as u32;
        ((115u32.saturating_sub(ms)) * 100) / 80
    }

    /// Current-game active-prediction accuracy. Lifetime accuracy lives on the
    /// persisted CpuBrain; keeping this scope on WormGame makes restart reset it.
    pub fn round_pred_accuracy(&self) -> f32 {
        if self.round_pred_total == 0 {
            0.0
        } else {
            self.round_pred_hits as f32 / self.round_pred_total as f32
        }
    }

    /// Wins for display: the banked scoreboard PLUS the current game's
    /// winner. The scoreboard is banked in restart() (one game stale at
    /// game-over time); display code must show the game that just ended or
    /// the champion check fires a round late (the "I won but the CPU is the
    /// champion" bug).
    pub fn displayed_wins(&self) -> [u32; 2] {
        let mut w = self.session_wins;
        if self.game_over {
            if let Some(i) = self.winner {
                w[i] += 1;
            }
        }
        w
    }

    /* ------------------------------ power-ups ------------------------------ */

    /// Fire the cycle's held power-up (if any). Returns true when something fired.
    pub fn fire_powerup(&mut self, who: usize) -> bool {
        if self.game_over || !self.cycles[who].alive {
            return false;
        }
        let kind = match self.cycles[who].held_powerup.take() {
            Some(k) => k,
            None => return false,
        };
        // Ghost recorder: a successful fire is an input, and inputs are the
        // replay. Kind 1 = player (between frames, stamped with the last
        // completed frame); kind 3 = CPU (inside update, stamped with the
        // current frame). The site is the phase.
        if self.script.is_none() && (who != self.player || self.arena_version < 10) {
            let kind = if who == self.player { 1 } else { 3 };
            self.replay.events.push((self.frame_count, kind, who as u8));
        }
        let (hx, hy) = self.cycles[who].head;
        let dir = self.cycles[who].direction;
        let (dx, dy) = dir.as_delta();
        match kind {
            PowerUpKind::Laser => {
                let beam = self.beam_cells(hx, hy, dx, dy);
                // The beam detonates any bombs caught in its path. Blast
                // credit follows the TRIGGER, not the planter: the firer is
                // immune to the blast it set off, and the bomb's owner can
                // die to it — previously an enemy bomb you lasered could
                // never harm its planter.
                let mut ignition_hit = false;
                for &(bx, by) in &beam {
                    if let Some(i) = self.bombs.iter().position(|b| b.x == bx && b.y == by) {
                        let b = self.bombs.remove(i);
                        self.detonate(b.x, b.y, who as u8);
                        ignition_hit = true;
                    }
                }
                // Kill on contact with the opponent head.
                let opp = 1 - who;
                if self.cycles[opp].alive && beam.contains(&self.cycles[opp].head) {
                    ignition_hit = true;
                    let (ox, oy) = self.cycles[opp].head;
                    self.add_impact_particles(ox, oy, self.cycles[opp].color);
                    self.cycles[opp].alive = false;
                    if self.death_cause.is_none() {
                        self.death_cause = Some(DeathCause::Laser);
                    }
                    if self.game_over {
                        // A beam-triggered bomb blast already killed someone
                        // this frame; the beam then reaching the other head
                        // means both died -> draw, not an overwrite.
                        self.winner = None;
                    } else {
                        self.game_over = true;
                        self.winner = Some(who);
                        play_death_riff(80);
                    }
                }
                // TAIL SEVER — the beam cuts the opponent's trail where it
                // crosses, and everything beyond the cut is lost.
                //
                // Cut at the crossing NEAREST THEIR HEAD (the minimum index in
                // a head-first `positions`), so a clean shot across the neck
                // costs them nearly the whole body and a lazy shot at the tail
                // tip costs almost nothing. That is what makes aiming worth
                // something.
                //
                // Runs AFTER the bomb loop above on purpose: a beam-triggered
                // blast calls `detonate`, which retains `positions`, so an
                // index computed before it would point at a cell that no longer
                // exists. `skip(1)` keeps index 0 — the head — out of it; a
                // beam on the head is the kill path above, never this one.
                if self.cycles[opp].alive {
                    let cut = self.cycles[opp]
                        .positions
                        .iter()
                        .enumerate()
                        .skip(1)
                        .find(|(_, p)| beam.contains(p))
                        .map(|(i, _)| i);
                    self.laser_audit = Some(LaserAudit {
                        firer: who,
                        cells: beam.cells.clone(),
                        opp_positions: self.cycles[opp].positions.clone(),
                        cut,
                    });
                    self.laser_audit_last = self.laser_audit.clone();
                    if let Some(cut) = cut {
                        ignition_hit = true;
                        self.sever_from(opp, cut);
                        play_beep(SfxKind::Laser, 900, 60);
                    }
                }

                // BREACH — the fifth arena-wall strike punches through. Applied
                // here and nowhere else: `beam_cells` is `&self` and runs every
                // frame while the CPU is only aiming.
                if let Some((bx, by)) = beam.breach {
                    self.grid[by as usize][bx as usize] = CellType::Hole;
                    self.add_impact_particles(bx, by, (120, 255, 120));
                    play_beep(SfxKind::Breach, 660, 60);
                }

                if self.arena_version >= 7 {
                    // ADR-023: the beam exists across this frame's movement
                    // transition — snapshot once, re-test occupancy after
                    // the frame moves (reconcile_beams). The render layer
                    // consumes the SAME cells (beam_fx), never a recompute.
                    self.pending_beams.push(PendingBeam {
                        firer: who as u8,
                        cells: beam.cells.clone(),
                        ignition_hit,
                        stop: beam.stop,
                        breached: beam.breach.is_some(),
                    });
                    self.beam_fx.push(BeamFx {
                        cells: beam.cells.clone(),
                        age: 0,
                        fresh: true,
                    });
                } else {
                    let _ = ignition_hit;
                    // Pre-v7 flash: a particle line. Lifetime in the same
                    // 15..40 range as impact particles so the alpha fade
                    // (lifetime/40) renders a visible line.
                    for &(bx, by) in &beam {
                        self.particles.push(Particle {
                            x: bx as f32,
                            y: by as f32,
                            vx: 0.0,
                            vy: 0.0,
                            lifetime: 20,
                            color: (255, 255, 120),
                        });
                    }
                }
                play_beep(SfxKind::Laser, 1800, 30);
            }
            PowerUpKind::TriShot => {
                // Straight ahead plus the two forward diagonals.
                let dirs = [(dx, dy), (dx + dy, dy + dx), (dx - dy, dy - dx)];
                // World v9 NAPALM flew 4 cells; v11 restores the full
                // ray (owner: "maybe they need to go further") — the
                // napalm-on-touch stays, the reach returns.
                let steps = if self.arena_version == 9 || self.arena_version == 10 {
                    4
                } else {
                    TRI_SHOT_MAX_STEPS
                };
                for (ddx, ddy) in dirs {
                    self.projectiles.push(Projectile {
                        x: hx,
                        y: hy,
                        dx: ddx,
                        dy: ddy,
                        steps_left: steps,
                        from: who as u8,
                    });
                }
                // Muzzle flash so the fire is visible at the head.
                self.add_impact_particles(hx, hy, self.cycles[who].color);
                play_beep(SfxKind::TriShot, 1200, 40);
            }
            PowerUpKind::Bomb => {
                // Rolled from the game RNG so the disguise is reproducible
                // under a seed. NOTE: this consumes a draw at plant time, so
                // seeded streams diverge from pre-mine builds — deliberate.
                let disguise = self.rng_range(1..10) as u8;

                // World v8: ~15 wall-clock seconds of decoy, in ms.
                let fuse = if self.arena_version >= 8 {
                    15_000
                } else {
                    BOMB_FUSE_FRAMES
                };
                self.bombs.push(Bomb {
                    x: hx,
                    y: hy,
                    fuse,
                    armed_in: MINE_ARM_FRAMES,
                    disguise,
                    owner: who as u8,
                    tripped: false,
                });
                self.add_impact_particles(hx, hy, (255, 120, 40));
                // Two-beat "thud" — audible countdown cue without blocking.
                play_beep_sequence(SfxKind::BombPlant, &[220, 160], &[90, 90]);
            }
        }
        if self.game_over {
            // The shot ended the game mid-frame (laser kill or a triggered
            // blast). Discharged beams still get their occupancy test — a
            // pending opposing beam with this firer standing on its line
            // turns the win into a draw (k3 v7 verify B1: mutual beams
            // must be able to draw even when one discharge ends the game).
            // Then in-flight bolts and armed bombs get their frame-end
            // step — a survivor killed here also turns the win into a
            // draw. The player-fired path runs OUTSIDE update() (which
            // stops once game_over is set), so this is its only chance.
            self.reconcile_beams();
            // Age here too: this exit ends the frame, and the fresh flip
            // must happen NOW so the killing beam's solid core paints
            // exactly once (k3 v7 round 2: without this, the first
            // post-game pump does the flip and the core paints twice).
            self.age_beam_fx();
            self.advance_projectiles();
            self.tick_flames();
            self.tick_bombs();
        }
        true
    }

    /// The path a beam traces, and the wall cell it breaches (if any).
    ///
    /// Ricochets off the current arena wall, reflecting the component
    /// orthogonal to the struck segment. On the fifth arena-wall strike the
    /// beam has spent its ricochets and **punches through**, stopping there —
    /// the ricochet is the charge-up and the breach is the payoff.
    ///
    /// It also **terminates at any Hole**, including ones punched earlier.
    /// One rule: a beam ends where it leaves the arena. Passing through old
    /// holes while stopping at fresh ones would be an inconsistency a player
    /// would notice, and it would make the beam's reach depend on damage
    /// history in a way nobody could reason about.
    ///
    /// `&self` deliberately: the breach is RETURNED, never applied here. This
    /// is called every frame while the CPU merely AIMS (and for the telegraph),
    /// so applying it in place would punch a hole per frame for a shot that may
    /// never be fired.
    /// Public forensic view of the beam path (replay auditing).
    #[doc(hidden)]
    pub fn beam_cells_public(&self, hx: u16, hy: u16, dx: i16, dy: i16) -> Vec<(u16, u16)> {
        self.beam_cells(hx, hy, dx, dy).cells
    }

    pub(crate) fn beam_cells(&self, hx: u16, hy: u16, dx: i16, dy: i16) -> BeamPath {
        let mut cells = Vec::new();
        let mut breach = None;
        let mut stop = None;
        let mut x = hx as i16;
        let mut y = hy as i16;
        let mut rdx = dx;
        let mut rdy = dy;
        let mut bounces = 0;
        let off = self.arena_wall_offset();
        loop {
            x += rdx;
            y += rdy;
            if x < 0 || y < 0 || x >= self.width as i16 || y >= self.height as i16 {
                break;
            }
            let (ux, uy) = (x as u16, y as u16);
            match self.grid[uy as usize][ux as usize] {
                CellType::Wall => {
                    if self.is_arena_wall(ux, uy) {
                        if bounces < LASER_MAX_BOUNCES {
                            if (ux == off || ux == self.width - 1 - off) && rdx != 0 {
                                rdx = -rdx;
                            }
                            if (uy == off || uy == self.height - 1 - off) && rdy != 0 {
                                rdy = -rdy;
                            }
                            bounces += 1;
                            continue;
                        }
                        breach = Some((ux, uy));
                    }
                    stop = Some((ux, uy));
                    break;
                }
                // A hole is where the arena ends for a beam — see above.
                CellType::Hole => {
                    stop = Some((ux, uy));
                    break;
                }
                _ => cells.push((ux, uy)),
            }
        }
        BeamPath { cells, breach, stop }
    }

    /// The decoy's telegraph (world v8): 0 = calm, 1 = flashing (final
    /// ~2s), 2 = flashing hard (final ~1s). Fuse is milliseconds under
    /// v8, so the tiers are exact wall-clock — no speed dependence.
    pub fn bomb_flash_tier(&self, b: &Bomb) -> u8 {
        if self.arena_version < 8 || b.tripped {
            return 0;
        }
        if b.fuse <= 1000 {
            2
        } else if b.fuse <= 2000 {
            1
        } else {
            0
        }
    }

    /// ADR-023: one beam-render tick — fresh beams keep their solid
    /// core for the frame that first paints them; everything else decays.
    fn age_beam_fx(&mut self) {
        for fx in &mut self.beam_fx {
            if fx.fresh {
                fx.fresh = false;
            } else {
                fx.age += 1;
            }
        }
        self.beam_fx.retain(|fx| fx.age <= 20);
    }

    /// ADR-023 (world v7): the post-move half of the laser's dual test.
    /// Tests occupancy of this frame's discharged beams against the
    /// IMMUTABLE snapshotted cells: a head that entered the line dies, a
    /// body cell that entered severs. No retrace, no second breach, no
    /// bomb re-scan. Applies same-frame deaths ATOMICALLY (both dead =
    /// draw, even when the other death was a movement crash earlier in
    /// this same frame). Returns true when it ended the game.
    fn reconcile_beams(&mut self) -> bool {
        if self.arena_version < 7 || self.pending_beams.is_empty() {
            return false;
        }
        let beams = std::mem::take(&mut self.pending_beams);
        let mut killed = [false; 2];
        for beam in &beams {
            let cells = &beam.cells;
            let mut beam_connected = beam.ignition_hit;
            for (opp, killed_slot) in killed.iter_mut().enumerate() {
                // The firer is immune to their own beam, head and body.
                if opp as u8 == beam.firer || !self.cycles[opp].alive {
                    continue;
                }
                if cells.contains(&self.cycles[opp].head) {
                    *killed_slot = true;
                    beam_connected = true;
                    let (hx, hy) = self.cycles[opp].head;
                    // Hit marker anchors at the HIT CELL (ADR-023).
                    self.add_impact_particles(hx, hy, self.cycles[opp].color);
                } else {
                    let cut = self.cycles[opp]
                        .positions
                        .iter()
                        .enumerate()
                        .skip(1)
                        .find(|(_, p)| cells.contains(p))
                        .map(|(i, _)| i);
                    if let Some(cut) = cut {
                        beam_connected = true;
                        let (sx, sy) = self.cycles[opp].positions[cut];
                        self.add_impact_particles(sx, sy, self.cycles[opp].color);
                        self.sever_from(opp, cut);
                        play_beep(SfxKind::Laser, 900, 60);
                    }
                }
            }
            if !beam_connected {
                // A TRUE whiff: no worm, no bomb, either phase. The beam
                // dies on its terminal WALL cell with a distinct low clank
                // and a spark THERE (codex v7 verify: cells.last() was the
                // preceding lane cell). A breach already sparked its own
                // cell — clank only, no duplicate spark (breach is not a
                // connect, but it is not silent either).
                if !beam.breached {
                    if let Some((lx, ly)) = beam.stop {
                        self.particles.push(Particle {
                            x: lx as f32,
                            y: ly as f32,
                            vx: 0.0,
                            vy: 0.0,
                            lifetime: 12,
                            color: (180, 180, 180),
                        });
                    }
                }
                play_beep(SfxKind::Laser, 300, 70);
            }
        }
        if !(killed[0] || killed[1]) {
            return false;
        }
        for (i, dead) in killed.iter().enumerate() {
            if *dead {
                self.cycles[i].alive = false;
                if self.death_cause.is_none() {
                    self.death_cause = Some(DeathCause::Laser);
                }
            }
        }
        // ATOMIC same-frame deaths (ADR-023): no first-processed winner.
        self.game_over = true;
        self.winner = match (!self.cycles[0].alive, !self.cycles[1].alive) {
            (true, true) => None,
            (true, false) => Some(1),
            (false, true) => Some(0),
            (false, false) => unreachable!(),
        };
        play_death_riff(90);
        true
    }

    /// Sever `opp`'s trail at positions index `cut` (nearest their head):
    /// everything from the cut back is lost. Shared by the laser beam and the
    /// tri-shot burst — one set of grid/positions lockstep rules, one place.
    pub fn sever_from(&mut self, opp: usize, cut: usize) {
        let severed = self.cycles[opp].positions.split_off(cut.max(1));
        // Grid/positions lockstep, same rule as detonate and close_ring: only
        // clear a cell that still holds THIS cycle's own marker, never one
        // another cycle has since legally occupied, and never a living head.
        let marker = if opp == self.player {
            CellType::Player
        } else {
            CellType::CPU
        };
        let heads: Vec<(u16, u16)> = self
            .cycles
            .iter()
            .filter(|c| c.alive)
            .map(|c| c.head)
            .collect();
        let color = self.cycles[opp].color;
        for (sx, sy) in severed {
            if self.grid[sy as usize][sx as usize] == marker && !heads.contains(&(sx, sy)) {
                self.grid[sy as usize][sx as usize] = CellType::Empty;
                self.particles.push(Particle {
                    x: sx as f32,
                    y: sy as f32,
                    vx: 0.0,
                    vy: 0.0,
                    lifetime: 8,
                    color,
                });
            }
        }
        // Owed growth would silently regrow what was just cut.
        self.cycles[opp].pending_growth = 0;
    }

    /// A tri-shot bolt detonating on contact: a 2x2 burst anchored at the
    /// impact cell and biased one cell FORWARD along the bolt's flight — a
    /// thrown grenade lands past where it strikes, never behind the thrower.
    ///
    /// Inside the burst, against the opponent only (the firer is immune, like
    /// every weapon here): their head dies; their trail is severed at the
    /// burst cell nearest their head, costing everything from there back —
    /// the same cut rule as the laser, so aiming near the neck is worth more
    /// than clipping the tail tip. Mines caught in the burst chain. Walls are
    /// NOT breached — breaching stays the laser's and the mine's job.
    fn bolt_blast(&mut self, x: u16, y: u16, dx: i16, dy: i16, from: u8) {
        // 2x2 quadrant: the impact cell plus one cell along each axis of
        // travel. A diagonal bolt extends into its natural quadrant; a
        // straight bolt borrows its perpendicular sign from the other axis
        // so the choice is deterministic under a seed.
        let ax = if dx != 0 { dx.signum() } else { dy.signum() };
        let ay = if dy != 0 { dy.signum() } else { dx.signum() };
        let mut cells: Vec<(u16, u16)> = Vec::with_capacity(4);
        for ox in [0, ax] {
            for oy in [0, ay] {
                let cx = x as i16 + ox;
                let cy = y as i16 + oy;
                if cx >= 0 && cy >= 0 && cx < self.width as i16 && cy < self.height as i16 {
                    let cell = (cx as u16, cy as u16);
                    if !cells.contains(&cell) {
                        cells.push(cell);
                    }
                }
            }
        }
        for &(bx, by) in &cells {
            self.add_impact_particles(bx, by, (255, 170, 60));
        }
        play_beep(SfxKind::Detonate, 150, 70);

        // Mines caught in the burst chain (they detonate on the next tick).
        for b in &mut self.bombs {
            if cells.contains(&(b.x, b.y)) {
                b.tripped = true;
                b.fuse = 0;
            }
        }

        let opp = (1 - from) as usize;
        if !self.cycles[opp].alive {
            return;
        }
        // Head inside the burst: the kill, with the same draw semantics as a
        // direct bolt hit.
        if cells.contains(&self.cycles[opp].head) {
            let (hx, hy) = self.cycles[opp].head;
            self.add_impact_particles(hx, hy, self.cycles[opp].color);
            self.cycles[opp].alive = false;
            if self.death_cause.is_none() {
                self.death_cause = Some(DeathCause::TriShotBolt);
            }
            if self.game_over {
                self.winner = None;
            } else {
                self.game_over = true;
                self.winner = Some(from as usize);
                play_death_riff(80);
            }
            return;
        }
        // Trail inside the burst: sever at the hit nearest their head.
        let cut = self.cycles[opp]
            .positions
            .iter()
            .enumerate()
            .skip(1)
            .find(|(_, p)| cells.contains(p))
            .map(|(i, _)| i);
        if let Some(cut) = cut {
            self.sever_from(opp, cut);
        }
    }

    /// Advance live tri-shot bolts one cell; bolts die on walls or at max range,
    /// and kill any head they enter.
    pub fn advance_projectiles(&mut self) {
        // World v11: bolts move TWO cells per frame — a fired bolt cannot
        // be outrun — as two ordered one-cell substeps, each running this
        // complete pipeline (codex v11 consult: never teleport over a
        // cell; stop at the first contact).
        let substeps = if self.arena_version >= 11 { 2 } else { 1 };
        let mut i = 0;
        'bolts: while i < self.projectiles.len() {
            for _substep in 0..substeps {
            let (x, y, from) = {
                let p = &self.projectiles[i];
                (p.x as i16 + p.dx, p.y as i16 + p.dy, p.from)
            };
            let off_board =
                x < 0 || y < 0 || x >= self.width as i16 || y >= self.height as i16;
            let wall_cell =
                !off_board && self.grid[y as usize][x as usize] == CellType::Wall;
            if off_board || wall_cell {
                // World v9: a bolt that dies on a wall drops its fire on
                // the LAST open cell it occupied (a bolt off the board
                // edge ignites nothing — there is no cell).
                if self.arena_version >= 9 && wall_cell {
                    let (lx, ly) = (self.projectiles[i].x, self.projectiles[i].y);
                    let from9 = self.projectiles[i].from;
                    self.ignite(lx, ly, from9);
                }
                self.projectiles.remove(i);
                continue 'bolts;
            }
            let (ux, uy) = (x as u16, y as u16);
            let (ox, oy) = (self.projectiles[i].x, self.projectiles[i].y);
            let (pdx, pdy) = (self.projectiles[i].dx, self.projectiles[i].dy);
            let opp = (1 - from) as usize;

            // Crossing swap: heads move before bolts each frame, so an
            // odd-gap head-on approach exchanges cells with the bolt in a
            // single frame — comparing post-move cells alone tunneled
            // straight through. The head's pre-move cell is positions[1].
            // Killed directly: the burst is forward-biased and cannot reach
            // a head that has already swapped BEHIND the impact cell.
            let swapped = self.cycles[opp].alive
                && self.cycles[opp].head == (ox, oy)
                && self.cycles[opp].positions.len() > 1
                && self.cycles[opp].positions[1] == (ux, uy);
            if swapped {
                if self.arena_version >= 11 {
                    // NAPALM everywhere (codex v11): a swept contact
                    // IGNITES AND CATCHES — the burn schedule does the
                    // killing, never an instant swap death. The catch is
                    // DIRECT: touch is touch, regardless of where the
                    // victim stands at the next hazard tick.
                    self.ignite(ux, uy, from);
                    self.catch_on_touch(opp, from);
                    self.projectiles.remove(i);
                    continue 'bolts;
                }
                self.add_impact_particles(ux, uy, self.cycles[opp].color);
                self.cycles[opp].alive = false;
                if self.death_cause.is_none() {
                    self.death_cause = Some(DeathCause::TriShotBolt);
                }
                if self.game_over {
                    // A bolt (or bomb) already killed the other head earlier
                    // this frame — both died, so it is a draw, never a
                    // first-come-first-served winner overwrite.
                    self.winner = None;
                } else {
                    self.game_over = true;
                    self.winner = Some(from as usize);
                    play_death_riff(80);
                }
                self.projectiles.remove(i);
                continue 'bolts;
            }

            // A bolt is a small thrown grenade: contact with ANY part of the
            // opponent — head or trail — detonates a 2x2 burst (see
            // `bolt_blast`: head in the burst dies, trail is severed at the
            // hit, mines chain). The firer's own trail is not a target; bolts
            // fly over it exactly as before.
            let opp_marker = if opp == self.player {
                CellType::Player
            } else {
                CellType::CPU
            };
            let contact = self.cycles[opp].alive
                && (self.cycles[opp].head == (ux, uy)
                    || self.grid[uy as usize][ux as usize] == opp_marker);
            if contact {
                if self.arena_version >= 9 {
                    // NAPALM: contact drops fire UNDER the victim — the
                    // burn schedule does the killing, not a blast. v11:
                    // the touch also catches DIRECTLY (tail-tip contacts
                    // used to retract out from under the flame).
                    self.ignite(ux, uy, from);
                    if self.arena_version >= 11 {
                        self.catch_on_touch(opp, from);
                    }
                } else {
                    self.bolt_blast(ux, uy, pdx, pdy, from);
                }
                self.projectiles.remove(i);
                continue 'bolts;
            }
            let p = &mut self.projectiles[i];
            p.x = ux;
            p.y = uy;
            p.steps_left = p.steps_left.saturating_sub(1);
            if p.steps_left == 0 {
                if self.arena_version >= 9 {
                    // Spent bolt: the napalm lands where it stops.
                    let (fx, fy) = (self.projectiles[i].x, self.projectiles[i].y);
                    let from9 = self.projectiles[i].from;
                    self.ignite(fx, fy, from9);
                }
                self.projectiles.remove(i);
                continue 'bolts;
            }
            }
            i += 1;
        }
    }

    /// World v11: a bolt TOUCH catches the victim directly — the catch
    /// must never depend on the victim still overlapping the ground
    /// flame at the next hazard tick (codex v11 verify: a length-2
    /// victim vacates the ignition cell before tick_flames and touch
    /// produced nothing). Ground fire still drops for area effect.
    fn catch_on_touch(&mut self, victim: usize, by: u8) {
        if self.burns[victim].contact_ms == 0 {
            self.burns[victim] = BurnState {
                contact_ms: 1,
                taken: 0,
                burned_by: by,
            };
        }
    }

    /// Ignite a napalm patch (world v9): ~3 wall-clock seconds of flame.
    /// Fire on a planted decoy COOKS it — early detonation (ADR-022).
    pub fn ignite(&mut self, x: u16, y: u16, owner: u8) {
        if let Some(i) = self.bombs.iter().position(|b| b.x == x && b.y == y) {
            let b = self.bombs.remove(i);
            self.detonate(b.x, b.y, owner);
        }
        self.flames.push(Flame {
            x,
            y,
            life_ms: 3_000,
            owner,
        });
        play_beep(SfxKind::TriShot, 700, 50);
    }

    /// The napalm hazard phase (world v9): age flames on the wall clock,
    /// catch worms standing in fire, and run each caught worm's STICKY
    /// 5/3/1 burn schedule to completion — tail-first, head last; a worm
    /// burned past its head dies. The owner of the flame that caught a
    /// worm is recorded for ledger attribution; a worm is IMMUNE to its
    /// own fire (ADR-023 rule). Frozen worms burn: the clock is global.
    pub fn tick_flames(&mut self) {
        if self.arena_version < 9 {
            return;
        }
        let tick_ms = (self.frame_delay().as_millis() as u32).max(1);
        for f in &mut self.flames {
            f.life_ms = f.life_ms.saturating_sub(tick_ms);
        }
        self.flames.retain(|f| f.life_ms > 0);
        // Cook any decoy a flame overlaps (spawned under an existing
        // flame, or the flame spread onto it via ignite above).
        let cooked: Vec<(u16, u16, u8)> = self
            .bombs
            .iter()
            .filter_map(|b| {
                self.flames
                    .iter()
                    .find(|f| (f.x, f.y) == (b.x, b.y))
                    .map(|f| (b.x, b.y, f.owner))
            })
            .collect();
        for (bx, by, fowner) in cooked {
            if let Some(i) = self.bombs.iter().position(|b| (b.x, b.y) == (bx, by)) {
                let b = self.bombs.remove(i);
                self.detonate(b.x, b.y, fowner);
            }
        }
        let mut deaths = [false; 2];
        for (who, death_slot) in deaths.iter_mut().enumerate() {
            if !self.cycles[who].alive {
                self.burns[who] = BurnState::default();
                continue;
            }
            let burning = self.burns[who].contact_ms > 0;
            if !burning {
                // Catch: any body cell standing in someone ELSE's fire.
                let catcher = self.flames.iter().find(|f| {
                    f.owner != who as u8
                        && self.cycles[who].positions.contains(&(f.x, f.y))
                });
                if let Some(f) = catcher {
                    self.burns[who] = BurnState {
                        contact_ms: 1,
                        taken: 0,
                        burned_by: f.owner,
                    };
                }
                continue;
            }
            // STICKY schedule, wall-clock: up to 5 segments in the first
            // second of contact, 3 in the second, 1 in the third.
            let b = &mut self.burns[who];
            b.contact_ms = b.contact_ms.saturating_add(tick_ms);
            let t = b.contact_ms;
            // FLOOR pacing (k3 v9 verify): each tier's quota spreads
            // across its second and COMPLETES at the boundary — the
            // 5th segment lands at t=1.0s, the 8th at 2.0s, the 9th at
            // 3.0s; ceil pacing front-loaded each tier's first quantum
            // onto the boundary tick.
            let target = if t >= 3_000 {
                9
            } else if t >= 2_000 {
                8 + (t - 2_000) / 1_000
            } else if t >= 1_000 {
                5 + (t - 1_000) * 3 / 1_000
            } else {
                t * 5 / 1_000
            };
            while b.taken < target {
                b.taken += 1;
                let cy = &mut self.cycles[who];
                if cy.positions.len() <= 1 {
                    // Burned past the head.
                    *death_slot = true;
                    break;
                }
                let (tx, ty) = cy.positions.pop().unwrap();
                let marker = if who == 0 { CellType::Player } else { CellType::CPU };
                if self.grid[ty as usize][tx as usize] == marker {
                    self.grid[ty as usize][tx as usize] = CellType::Empty;
                }
                self.particles.push(Particle {
                    x: tx as f32,
                    y: ty as f32,
                    vx: 0.0,
                    vy: 0.0,
                    lifetime: 14,
                    color: (255, 140, 30),
                });
            }
            if b.contact_ms >= 3_000 && !*death_slot {
                // Schedule complete — the fire lets go.
                self.burns[who] = BurnState::default();
            }
        }
        if deaths[0] || deaths[1] {
            for (who, dead) in deaths.iter().enumerate() {
                if *dead {
                    let (hx, hy) = self.cycles[who].head;
                    self.add_impact_particles(hx, hy, (255, 120, 30));
                    self.cycles[who].alive = false;
                    if self.death_cause.is_none() {
                        self.death_cause = Some(DeathCause::Burned);
                    }
                }
            }
            // ATOMIC with any death already on this frame (codex v9
            // verify: the hazard phase must run on game-over exits too —
            // a burn completing while the other worm crashed is a draw,
            // same law as reconcile_beams).
            self.game_over = true;
            self.winner = match (!self.cycles[0].alive, !self.cycles[1].alive) {
                (true, true) => None,
                (true, false) => Some(1),
                (false, true) => Some(0),
                (false, false) => unreachable!(),
            };
            play_death_riff(90);
        }
    }

    pub fn tick_bombs(&mut self) {
        // World v8: the fuse counts MILLISECONDS, drained by the current
        // frame delay — wall-clock at any speed, and a freeze disarms
        // nothing (bombs tick on the global frame, not per-worm frames).
        let tick_ms = if self.arena_version >= 8 {
            (self.frame_delay().as_millis() as u32).max(1)
        } else {
            1
        };
        for b in &mut self.bombs {
            b.armed_in = b.armed_in.saturating_sub(1);
            b.fuse = b.fuse.saturating_sub(tick_ms);
        }
        // Pre-v8: a fuse that runs out FIZZLES — its only job was to stop
        // stale mines accumulating, and an INVISIBLE timer must never be a
        // weapon. World v8 supplies the missing ingredient: the last two
        // seconds FLASH (bomb_flash_tier), so the timer detonation is
        // telegraphed and becomes fair. A tripped mine keeps its fuse==0
        // for the drain below and is exempt from both.
        if self.bomb_expiry_detonates() {
            for b in &mut self.bombs {
                if b.fuse == 0 {
                    b.tripped = true;
                }
            }
        }
        let mut fizzled: Vec<(u16, u16)> = Vec::new();
        self.bombs.retain(|b| {
            let expired = b.fuse == 0 && !b.tripped;
            if expired {
                fizzled.push((b.x, b.y));
            }
            !expired
        });
        for (x, y) in fizzled {
            // A visible little puff, so the disappearing "food" reads as the
            // dud it was rather than a glitch.
            self.particles.push(Particle {
                x: x as f32,
                y: y as f32,
                vx: 0.0,
                vy: 0.0,
                lifetime: 8,
                color: (120, 120, 120),
            });
        }
        // PROXIMITY. An armed mine fires the instant a head that is not its
        // planter's enters the trigger ring.
        //
        // A timer could never be a weapon here: the fuse was 26-85 frames and
        // clearing the radius took 11 moves, so an attentive target simply
        // walked out. A mine cannot be dodged by waiting — only by routing
        // around it, which is the game's actual skill.
        //
        // Sets fuse to 0 rather than detonating inline, so the chain-reaction
        // drain below stays the single detonation path with one set of
        // draw/winner semantics.
        let t = MINE_TRIGGER_CELLS as i32;
        for i in 0..self.bombs.len() {
            if self.bombs[i].armed_in > 0 || self.bombs[i].fuse == 0 {
                continue;
            }
            let (bx, by, owner) = (self.bombs[i].x, self.bombs[i].y, self.bombs[i].owner);
            let tripped = self.cycles.iter().enumerate().any(|(c, cy)| {
                c as u8 != owner
                    && cy.alive
                    && (cy.head.0 as i32 - bx as i32)
                        .abs()
                        .max((cy.head.1 as i32 - by as i32).abs())
                        <= t
            });
            if tripped {
                self.bombs[i].tripped = true;
                self.bombs[i].fuse = 0;
            }
        }
        while let Some(i) = self.bombs.iter().position(|b| b.fuse == 0) {
            let b = self.bombs.remove(i);
            self.detonate(b.x, b.y, b.owner);
        }
    }

    /// Detonate at (x,y): a CROSS (core square + four axis arms) kills heads, clears
    /// trails/food/power-ups, and chains into other armed bombs. Walls survive.
    /// The bomb's `owner` is never killed by its own blast (mirrors the
    /// tri-shot `from` exclusion). If the game already ended earlier in this
    /// frame (e.g. a bolt kill), a head killed here makes it a draw rather
    /// than overwriting the first kill's winner.
    fn detonate(&mut self, x: u16, y: u16, owner: u8) {
        self.add_impact_particles(x, y, (255, 120, 40));
        // Three-note descending rumble.
        play_beep_sequence(SfxKind::Detonate, &[110, 90, 70], &[110, 110, 110]);
        let r = BOMB_RADIUS_CELLS as i32;
        let (cx, cy) = (x as i32, y as i32);
        // Trail cells cleared from the grid must also leave the owning cycle's
        // positions list: a stale entry would later tail-pop an Empty write
        // over a cell another snake has since legally occupied.
        let mut cleared: Vec<(u16, u16)> = Vec::new();
        for yy in (cy - r)..=(cy + r) {
            for xx in (cx - r)..=(cx + r) {
                if xx < 0 || yy < 0 || xx >= self.width as i32 || yy >= self.height as i32 {
                    continue;
                }
                // Cross, not square: 65 cells instead of 441.
                if !in_blast(cx, cy, xx, yy, r) {
                    continue;
                }
                let (ux, uy) = (xx as u16, yy as u16);
                match self.grid[uy as usize][ux as usize] {
                    cell @ (CellType::Player | CellType::CPU) => {
                        // World v8 (ADR-022, decided with the decoy): a blast
                        // is OWNER-SAFE, trail included — the ADR-023 rule
                        // ("the firer is immune to their own discharged
                        // weapon, head and body") applied to bombs. The head
                        // was always excluded; pre-v8 the trail sever stayed
                        // for replay identity. Measured on the warm arms: a
                        // 15s decoy outlives the planner's own wall-follow
                        // lap, and expiry blasts were severing the planting
                        // CPU to len-1 scrap (four deaths in one arm at the
                        // first expiry wave, frame ~192).
                        let owner_marker = if owner == 0 {
                            CellType::Player
                        } else {
                            CellType::CPU
                        };
                        let owner_safe =
                            self.arena_version >= 8 && cell == owner_marker;
                        // A living head marker survives the sweep — head fates
                        // are decided by the radius check below, and erasing a
                        // survivor's marker would let the opponent drive onto
                        // an occupied head cell without a collision. NOTE the
                        // owner-safety must not `continue` the CELL loop: the
                        // chain check below runs for every swept cell, and a
                        // bomb sitting on the owner's own trail still chains
                        // (codex v8 verify, blocking).
                        if !owner_safe
                            && !self.cycles.iter().any(|c| c.alive && c.head == (ux, uy))
                        {
                            self.grid[uy as usize][ux as usize] = CellType::Empty;
                            cleared.push((ux, uy));
                        }
                    }
                    CellType::Food => {
                        self.grid[uy as usize][ux as usize] = CellType::Empty;
                        self.food_items.retain(|&(fx, fy, _)| (fx, fy) != (ux, uy));
                    }
                    CellType::PowerUp => {
                        self.grid[uy as usize][ux as usize] = CellType::Empty;
                        self.powerups.retain(|&(px, py, _)| (px, py) != (ux, uy));
                    }
                    CellType::Wall if self.is_arena_wall(ux, uy) => {
                        // Design intent: a blast punches the ring-2 arena
                        // wall open (a Hole, same as a laser breach) so players can
                        // reach the outer corridor. The ring-0 frame is
                        // indestructible.
                        self.grid[uy as usize][ux as usize] = CellType::Hole;
                    }
                    _ => {}
                }
                // Chain: other armed bombs caught in the blast go off too.
                for b in &mut self.bombs {
                    if b.x == ux && b.y == uy {
                        b.tripped = true;
                        b.fuse = 0;
                    }
                }
            }
        }
        if !cleared.is_empty() {
            for c in &mut self.cycles {
                c.positions.retain(|p| !cleared.contains(p));
            }
        }
        // Kill heads in the radius (owner excluded; both can die -> draw).
        let mut dead = [false; 2];
        for (c, is_dead) in dead.iter_mut().enumerate() {
            if c as u8 == owner {
                continue;
            }
            let (hx, hy) = self.cycles[c].head;
            // Same predicate as the sweep and both previews: on-axis dies,
            // diagonal survives.
            if in_blast(cx, cy, hx as i32, hy as i32, r) {
                *is_dead = true;
            }
        }
        if dead[0] || dead[1] {
            if self.death_cause.is_none() {
                self.death_cause = Some(DeathCause::BombBlast);
            }
            for (c, &is_dead) in dead.iter().enumerate() {
                if is_dead && self.cycles[c].alive {
                    self.cycles[c].alive = false;
                    let (hx, hy) = self.cycles[c].head;
                    self.add_impact_particles(hx, hy, self.cycles[c].color);
                }
            }
            if self.game_over {
                // A kill earlier this frame already decided the game. If the
                // blast now leaves both cycles dead, it is a draw — never let
                // the later event overwrite the first kill's winner.
                if !self.cycles[0].alive && !self.cycles[1].alive {
                    self.winner = None;
                }
            } else {
                self.game_over = true;
                self.winner = match (dead[0], dead[1]) {
                    (true, false) => Some(1),
                    (false, true) => Some(0),
                    _ => None,
                };
                play_death_riff(100);
            }
        }
    }

    /// Spawn one random power-up on a free interior cell (never in the corridor).
    pub fn spawn_powerup(&mut self) {
        if self.powerups.len() >= MAX_POWERUPS_ON_BOARD {
            return;
        }
        let (xlo, xhi, ylo, yhi) = if self.has_corridor() {
            (4, self.width - 4, 4, self.height - 4)
        } else {
            (
                2,
                self.width.saturating_sub(2),
                2,
                self.height.saturating_sub(2),
            )
        };
        for _ in 0..200 {
            let x = self.rng_range(xlo..xhi);
            let y = self.rng_range(ylo..yhi);
            if self.grid[y as usize][x as usize] == CellType::Empty
                && !self.bombs.iter().any(|b| (b.x, b.y) == (x, y))
            {
                // Even thirds. The 0..10 draw shape is kept deliberately: it
                // is ONE call on the seeded RNG stream, and changing the call
                // shape would shift every later draw, not just this one.
                let kind = match self.rng_range(0..10) {
                    0..=2 => PowerUpKind::Laser,
                    3..=5 => PowerUpKind::TriShot,
                    6..=8 => PowerUpKind::Bomb,
                    _ => PowerUpKind::Bomb,
                };
                self.powerups.push((x, y, kind));
                self.grid[y as usize][x as usize] = CellType::PowerUp;
                break;
            }
        }
    }

    /// Which kind of power-up is sitting on this cell, if any.
    ///
    /// The grid only carries `CellType::PowerUp`; the kind lives here. Without
    /// this the CPU could not tell a Laser from a Bomb while the human could
    /// see the icons — an information asymmetry in the human's favour, and the
    /// reason the CPU would happily destroy a held Laser by driving over a
    /// Bomb. O(3): MAX_POWERUPS_ON_BOARD.
    pub fn powerup_at(&self, x: u16, y: u16) -> Option<PowerUpKind> {
        self.powerups
            .iter()
            .find(|&&(px, py, _)| (px, py) == (x, y))
            .map(|&(_, _, k)| k)
    }

    /// Display name for a held power-up slot (HUD).
    pub fn powerup_name(kind: Option<PowerUpKind>) -> &'static str {
        match kind {
            Some(PowerUpKind::Laser) => "LASER",
            Some(PowerUpKind::TriShot) => "TRI-SHOT",
            Some(PowerUpKind::Bomb) => "BOMB",
            None => "-",
        }
    }

    /// Terminal renderer (native only — the browser build draws to canvas).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn render(&self, stdout: &mut std::io::Stdout) {
        use crossterm::{
            cursor::MoveTo,
            execute,
            style::{Color, Print, ResetColor, SetForegroundColor},
            terminal::{Clear, ClearType},
        };

        execute!(stdout, Clear(ClearType::All), MoveTo(0, 0)).unwrap();

        let pulse = (self.time as f32 * 0.2).sin() * 0.3 + 0.7;

        // Body-index lookup for trail gradients (head = 0, tail tip = len-1).
        let mut body_idx: [std::collections::HashMap<(u16, u16), usize>; 2] = [
            std::collections::HashMap::new(),
            std::collections::HashMap::new(),
        ];
        for (c, index) in body_idx.iter_mut().enumerate() {
            for (i, &p) in self.cycles[c].positions.iter().enumerate() {
                index.insert(p, i);
            }
        }

        // Head glyph shows the cycle's current heading.
        fn dir_glyph(d: Direction) -> char {
            match d {
                Direction::Up => '▲',
                Direction::Down => '▼',
                Direction::Left => '◀',
                Direction::Right => '▶',
            }
        }

        for y in 0..self.height as usize {
            let mut line = String::new();
            for x in 0..self.width as usize {
                let cell = self.grid[y][x];
                // Each game cell renders as TWO terminal chars — terminal
                // cells are ~2:1 tall, so 1-char cells made horizontal travel
                // look twice as fast as vertical. `double` = print glyph twice
                // (emoji are already ~2 cols wide, so they print once).
                let (r, g, b, ch, double): (u8, u8, u8, char, bool) = match cell {
                    CellType::Empty => {
                        // Bomb danger telegraph: empty cells inside any armed
                        // bomb's blast radius smoulder for the WHOLE fuse
                        // (parity with the web renderer, which shows the zone
                        // from plant time), heating up as detonation nears.
                        let mut danger: Option<f32> = None;
                        for b in &self.bombs {
                            if in_blast(
                                b.x as i32,
                                b.y as i32,
                                x as i32,
                                y as i32,
                                BOMB_RADIUS_CELLS as i32,
                            ) {
                                // Armed reads hot and steady; still arming
                                // reads dim — the player must be able to see
                                // that the dash-through window is open.
                                let urgency = if b.armed_in > 0 { 0.25 } else { 0.9 };
                                danger = Some(danger.map_or(urgency, |d: f32| d.max(urgency)));
                            }
                        }
                        if let Some(urgency) = danger {
                            let heat = ((self.time as f32 * 0.4).sin() * 0.5 + 0.5) * 60.0;
                            (
                                (40.0 + 90.0 * urgency + heat * urgency) as u8,
                                15,
                                10,
                                '░',
                                true,
                            )
                        } else {
                            (0, 0, 0, ' ', true)
                        }
                    }
                    CellType::Player => {
                        let c = &self.cycles[0];
                        if (x, y) == (c.head.0 as usize, c.head.1 as usize) {
                            // Head: white core, glyph shows heading.
                            (255, 255, 255, dir_glyph(c.direction), true)
                        } else {
                            // Trail: cyan fading from head (█) to tail tip (░).
                            let len = c.positions.len().max(1);
                            let i = body_idx[0]
                                .get(&(x as u16, y as u16))
                                .copied()
                                .unwrap_or(len - 1);
                            let t = (i.min(len - 1)) as f32 / (len - 1).max(1) as f32;
                            let g = (235.0 * (1.0 - t * 0.8) * pulse + 20.0) as u8;
                            let ch = if t < 0.33 {
                                '█'
                            } else if t < 0.66 {
                                '▓'
                            } else {
                                '░'
                            };
                            (0, g, g, ch, true)
                        }
                    }
                    CellType::CPU => {
                        let c = &self.cycles[1];
                        if (x, y) == (c.head.0 as usize, c.head.1 as usize) {
                            (255, 255, 255, dir_glyph(c.direction), true)
                        } else {
                            let len = c.positions.len().max(1);
                            let i = body_idx[1]
                                .get(&(x as u16, y as u16))
                                .copied()
                                .unwrap_or(len - 1);
                            let t = (i.min(len - 1)) as f32 / (len - 1).max(1) as f32;
                            let g = (235.0 * (1.0 - t * 0.8) * pulse + 20.0) as u8;
                            let ch = if t < 0.33 {
                                '█'
                            } else if t < 0.66 {
                                '▓'
                            } else {
                                '░'
                            };
                            (g, 0, g, ch, true)
                        }
                    }
                    CellType::Wall => {
                        // Ring-2 arena wall reads solid; the outer frame stays dim.
                        if self.is_arena_wall(x as u16, y as u16) {
                            (0, 140, 140, '▒', true)
                        } else {
                            (0, 60, 60, '·', true)
                        }
                    }
                    CellType::Food => {
                        let num = self
                            .food_items
                            .iter()
                            .find(|&&(fx, fy, _)| fx as usize == x && fy as usize == y)
                            .map(|&(_, _, n)| n)
                            .unwrap_or(0);
                        // Value-sized glyph — bigger number, bigger morsel,
                        // no digit shown.
                        let ch = food_glyph(num);
                        let hue = num as f32 * 18.0;
                        let (r, g, b) = hsv_to_rgb(hue, 1.0, 1.0);
                        let intensity = ((pulse * 255.0) as u8).max(100);
                        (
                            r.max(intensity),
                            g.max(intensity),
                            b.max(intensity),
                            ch,
                            true,
                        )
                    }
                    CellType::Hole => (140, 140, 140, '○', true),
                    CellType::PowerUp => {
                        let pu = self
                            .powerups
                            .iter()
                            .find(|&&(px, py, _)| px as usize == x && py as usize == y)
                            .map(|&(_, _, k)| k)
                            .unwrap_or(PowerUpKind::Laser);
                        let ch = match pu {
                            PowerUpKind::Laser => '⚡',
                            // Emoji-width like its siblings: the narrow '✦'
                            // printed one column and skewed every cell after
                            // it on that row.
                            PowerUpKind::TriShot => '🔱',
                            PowerUpKind::Bomb => '💣',
                        };
                        (200, 200, 0, ch, false)
                    }
                };

                if ch == ' ' {
                    line.push_str("  ");
                } else if double {
                    line.push_str(&format!("\x1b[38;2;{};{};{}m{}{}", r, g, b, ch, ch));
                } else {
                    line.push_str(&format!("\x1b[38;2;{};{};{}m{}", r, g, b, ch));
                }
            }

            execute!(
                stdout,
                MoveTo(0, y as u16),
                Print(format!("\x1b[0m{}", line))
            )
            .unwrap();
        }

        // Draw border (screen is 2 chars per cell wide → right edge at 2w-1).
        let rw = 2 * self.width - 1;
        execute!(
            stdout,
            SetForegroundColor(Color::Rgb {
                r: 0,
                g: 200,
                b: 255
            }),
            MoveTo(0, 0),
            Print("╔"),
            MoveTo(rw, 0),
            Print("╗"),
            MoveTo(0, self.height - 1),
            Print("╚"),
            MoveTo(rw, self.height - 1),
            Print("╝"),
        )
        .unwrap();

        for x in 1..rw {
            execute!(stdout, MoveTo(x, 0), Print("═")).unwrap();
            execute!(stdout, MoveTo(x, self.height - 1), Print("═")).unwrap();
        }
        for y in 1..self.height - 1 {
            execute!(stdout, MoveTo(0, y), Print("║")).unwrap();
            execute!(stdout, MoveTo(rw, y), Print("║")).unwrap();
        }

        // Draw live tri-shot bolts (separate from the grid — overlay).
        for p in &self.projectiles {
            execute!(
                stdout,
                SetForegroundColor(Color::Rgb {
                    r: 255,
                    g: 255,
                    b: 60
                }),
                MoveTo(p.x * 2, p.y),
                Print("✹✹")
            )
            .unwrap();
        }

        // Draw planted mines — DISGUISED AS FOOD. Same glyph table, same
        // value-scaled size, same hue as a real morsel of that value, so there
        // is no tell. Tracking where the opponent planted theirs is the
        // counter-play; there is nothing to see.
        for b in &self.bombs {
            let ch = food_glyph(b.disguise);
            let (r, g, bl) = hsv_to_rgb(b.disguise as f32 * 18.0, 1.0, 1.0);
            let intensity = ((pulse * 255.0) as u8).max(100);
            execute!(
                stdout,
                SetForegroundColor(Color::Rgb {
                    r: r.max(intensity),
                    g: g.max(intensity),
                    b: bl.max(intensity)
                }),
                MoveTo(b.x * 2, b.y),
                Print(ch)
            )
            .unwrap();
        }

        // Draw particles (over trails, walls and holes too — the beam flash
        // travels through trails and the breach flash lands on a Hole cell).
        for p in &self.particles {
            let x = p.x as u16;
            let y = p.y as u16;
            if x < self.width && y < self.height {
                let alpha = (p.lifetime as f32 / 40.0).min(1.0);
                let (r, g, b) = p.color;
                let fade_r = (r as f32 * alpha) as u8;
                let fade_g = (g as f32 * alpha) as u8;
                let fade_b = (b as f32 * alpha) as u8;
                execute!(
                    stdout,
                    MoveTo(x * 2, y),
                    SetForegroundColor(Color::Rgb {
                        r: fade_r,
                        g: fade_g,
                        b: fade_b
                    }),
                    Print("··")
                )
                .unwrap();
            }
        }

        // Draw UI bar
        let bar_color = if self.game_over {
            Color::Rgb {
                r: 255,
                g: 85,
                b: 128,
            }
        } else {
            Color::Rgb {
                r: 0,
                g: 255,
                b: 255,
            }
        };

        // Brain-memory readout (wide terminals only — keeps narrow HUDs clean):
        // self-episodes / opponent-episodes / opponent-prediction accuracy.
        let wide = self.width * 2 >= 100;
        let mem = if wide {
            format!(
                " │ MEM: {}/{}·{:.0}%",
                self.cpu_brain.episodes.len(),
                self.cpu_brain.opp_brain.episodes.len(),
                self.cpu_brain.opp_pred_accuracy() * 100.0,
            )
        } else {
            String::new()
        };
        let dw = self.displayed_wins();
        execute!(
            stdout,
            SetForegroundColor(bar_color),
            MoveTo(0, self.height),
            Print(format!(
                "╔{}╗ P1 FOOD: {:3} PWR: {:<9} │ P2 FOOD: {:3} PWR: {:<9} │ WINS {}:{} │ SPEED: {:3}% │ FOOD ON BOARD: {:2} │ FRAME: {}{}",
                "═".repeat(((2 * self.width).saturating_sub(1)) as usize),
                self.food_eaten_by[0],
                Self::powerup_name(self.cycles[0].held_powerup),
                self.food_eaten_by[1],
                Self::powerup_name(self.cycles[1].held_powerup),
                dw[0],
                dw[1],
                self.speed_pct(),
                self.food_items.len(),
                self.time,
                mem,
            )),
        ).unwrap();

        // Bottom bar: the live brain panel on wide terminals (rps-ai never
        // showed its models — we do), static help text otherwise.
        let bottom = if wide {
            let e = &self.cpu_brain.ensemble;
            let mut s = String::from("BRAIN ");
            for (i, name) in crate::cpu_ai::MODEL_NAMES.iter().enumerate() {
                let mark = if i == e.active { '*' } else { ' ' };
                s.push_str(&format!("{}:{:+.2}{} ", name, e.score(i), mark));
            }
            let current_decision = self.cpu_telemetry.decision.as_ref();
            let decision = current_decision.or(self.round_last_cpu_decision.as_ref());
            let decision_label = if current_decision.is_some() {
                "decision"
            } else {
                "last decision"
            };
            let arrow = decision
                .and_then(|trace| trace.forecast)
                .and_then(|forecast| forecast.predicted)
                .map(dir_glyph)
                .unwrap_or('·');
            let source = decision
                .and_then(|trace| trace.forecast)
                .map(|forecast| crate::cpu_ai::MODEL_NAMES[forecast.source])
                .unwrap_or("—");
            let action = decision
                .map(|trace| trace.reason.as_str())
                .unwrap_or("no decision");
            format!(
                "{}{}→ {}  round:{:.0}%/{} lifetime:{:.0}%/{} source:{} action:{}",
                s,
                decision_label,
                arrow,
                self.round_pred_accuracy() * 100.0,
                self.round_pred_total,
                self.cpu_brain.opp_pred_accuracy() * 100.0,
                self.cpu_brain.opp_pred_total,
                source,
                action,
            )
        } else {
            "←→ ARROW KEYS or WASD: Move │ SPACE: Fire Power-up │ R: Restart │ Q: Quit │ EAT NUMBERS TO GROW • COLLIDE TO DIE".to_string()
        };
        execute!(
            stdout,
            SetForegroundColor(Color::Rgb {
                r: 255,
                g: 85,
                b: 128
            }),
            MoveTo(0, self.height + 1),
            Print(bottom),
            ResetColor,
        )
        .unwrap();

        if self.game_over {
            let decision = self
                .cpu_telemetry
                .decision
                .as_ref()
                .or(self.round_last_cpu_decision.as_ref());
            let decision_source = decision
                .and_then(|trace| trace.forecast)
                .map(|forecast| crate::cpu_ai::MODEL_NAMES[forecast.source])
                .unwrap_or("—");
            let decision_action = decision
                .map(|trace| trace.reason.as_str())
                .unwrap_or("no decision");
            let winner_text = match self.winner {
                Some(0) => "PLAYER WINS!".to_string(),
                Some(1) => "CPU WINS!".to_string(),
                _ => "DRAW!".to_string(),
            };
            let winner_text = match self.death_cause {
                Some(c) => format!("{} — {}", winner_text, c.as_str()),
                None => winner_text,
            };

            execute!(
                stdout,
                SetForegroundColor(Color::Rgb { r: 255, g: 85, b: 128 }),
                MoveTo(self.width.saturating_sub(8), self.height / 2 - 1),
                Print("═════════════════════"),
                MoveTo(self.width.saturating_sub(8), self.height / 2),
                Print(format!("  {}  ", winner_text)),
                MoveTo(self.width.saturating_sub(8), self.height / 2 + 1),
                Print("═════════════════════"),
                MoveTo(self.width.saturating_sub(10), self.height / 2 + 2),
                Print(format!("FOOD: P1={}  P2={}  FRAMES={}  │  Press R to restart, Q to quit", self.food_eaten_by[0], self.food_eaten_by[1], self.frame_count)),
                // Brain summary: prediction accuracy, the driving model, session wins.
                MoveTo(self.width.saturating_sub(12), self.height / 2 + 3),
                Print(format!(
                    "BRAIN round:{:.0}%/{} lifetime:{:.0}%/{} source:{} action:{} wins you:{} cpu:{}",
                    self.round_pred_accuracy() * 100.0,
                    self.round_pred_total,
                    self.cpu_brain.opp_pred_accuracy() * 100.0,
                    self.cpu_brain.opp_pred_total,
                    decision_source,
                    decision_action,
                    dw[0],
                    dw[1],
                )),
                ResetColor,
            ).unwrap();
        }

        execute!(stdout, ResetColor).unwrap();
    }
}

pub struct Dimensions {
    pub width: u16,
    pub height: u16,
}

impl Dimensions {
    pub fn get_terminal_size() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (w, h) = size().unwrap_or((120, 40));
            Self {
                // 2 terminal chars per game cell (see render): the board uses
                // half the terminal's columns so horizontal and vertical
                // travel read as the same speed.
                width: (w / 2).max(20),
                height: h.saturating_sub(2),
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            // No terminal in the browser — the JS shell passes explicit board
            // dimensions; this is only a default for with_seed-style callers.
            Self {
                width: 120,
                height: 38,
            }
        }
    }
}

/// Glyph for a morsel of this value — bigger number, bigger morsel.
///
/// Shared by real food and by a planted mine's disguise, deliberately: two
/// copies of this table would eventually drift and the drift would BE the
/// tell that gives every mine away.
pub fn food_glyph(value: u8) -> char {
    match value {
        1..=2 => '·',
        3..=4 => '○',
        5..=6 => '◎',
        7..=8 => '●',
        _ => '◆',
    }
}

pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;

    let (r1, g1, b1) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    (
        ((r1 + m) * 255.0) as u8,
        ((g1 + m) * 255.0) as u8,
        ((b1 + m) * 255.0) as u8,
    )
}

pub fn rgb_to_hue((r, g, b): (u8, u8, u8)) -> f32 {
    let r = r as f32 / 255.0;
    let g = g as f32 / 255.0;
    let b = b as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;

    if delta == 0.0 {
        return 0.0;
    }

    let hue = if max == r {
        ((g - b) / delta) % 6.0
    } else if max == g {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    };

    (hue * 60.0).rem_euclid(360.0)
}

/* ------------------------------ sound effects ------------------------------ */

// On native, the terminal bell (threaded for jingles) — kinds are ignored,
// the bell has no pitch. In the browser (wasm), TYPED events are queued for
// the JS shell, which maps each kind to a chiptune patch in web/audio.js and
// drains the queue via drain_sfx_events()/sfx_json() each frame.
//
// Wire protocol (sfx_json): [[kind, freq_hz, dur_ms, delay_ms], ...]
//   kind      SfxKind as u8 (the JS-side patch contract).
//   Food      TWO events (the pickup blip, base freqs 880/1320). Both freqs
//             carry the food value as a pitch shift:
//               freq = base + value * FOOD_VALUE_STEP_HZ  (1-9 → +40..+360 Hz)
//             JS can play the freqs verbatim (richer pickups already sound
//             higher) or recover value = (freq - 880) / FOOD_VALUE_STEP_HZ
//             from the first event.
//   DeathRiff ONE event per kill — web/audio.js sequences the four-note
//             descending riff itself. freq carries the first note (440),
//             dur the per-note step in ms (100 crash, 80 weapon kill).
//   Others    one queue event per bell, delays accumulating within a jingle
//             exactly like v1 (BombPlant two-beat, Detonate three-note).

/// Sound-event kinds for the browser chiptune synth — the discriminant IS the
/// wire contract (see the protocol comment above). Native ignores kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum SfxKind {
    Food = 0,
    PowerUp = 1,
    Laser = 2,
    TriShot = 3,
    BombPlant = 4,
    Detonate = 5,
    /// Breach — a laser beam or bomb blast punching through the arena wall.
    /// KEEPS discriminant 6 (formerly WallPunch): the SfxKind number is the
    /// wire contract the browser switches on, so renumbering would silently
    /// remap every later sound.
    Breach = 6,
    DeathRiff = 7,
}

/// One queued sound event: (kind, freq_hz, duration_ms, delay_ms). A plain
/// tuple — serde-free, like the rest of the wasm API surface.
pub type SfxEvent = (u8, u32, u64, u64);

/// Hz of pitch shift per food value point — see the protocol comment above.
pub const FOOD_VALUE_STEP_HZ: u32 = 40;

/// The descending four-note death riff (pitches; tempo is the per-note step).
const DEATH_RIFF_NOTES: [u32; 4] = [440, 330, 220, 110];

#[cfg(target_arch = "wasm32")]
mod sfx_queue {
    use std::cell::RefCell;
    thread_local! {
        static EVENTS: RefCell<Vec<super::SfxEvent>> = RefCell::new(Vec::new());
    }
    pub fn push(ev: super::SfxEvent) {
        EVENTS.with(|e| e.borrow_mut().push(ev));
    }
    pub fn drain() -> Vec<super::SfxEvent> {
        EVENTS.with(|e| std::mem::take(&mut *e.borrow_mut()))
    }
}

/// Drain queued sound events (kind, freq_hz, duration_ms, delay_ms). Browser only.
#[cfg(target_arch = "wasm32")]
pub fn drain_sfx_events() -> Vec<SfxEvent> {
    sfx_queue::drain()
}

/// Native feature builds exercise the browser API contract without a browser
/// sound queue. Real WebAudio events remain wasm32-only.
#[cfg(not(target_arch = "wasm32"))]
pub fn drain_sfx_events() -> Vec<SfxEvent> {
    Vec::new()
}

/// Expand a kind-tagged jingle into queue entries with accumulating delays —
/// the wasm mirror of the native threaded bell sequence. Pure: unit-tested
/// on native (see sfx_tests).
#[cfg(any(target_arch = "wasm32", test))]
fn sequence_events(kind: SfxKind, freqs: &[u32], durations_ms: &[u64]) -> Vec<SfxEvent> {
    let notes = freqs.len().min(durations_ms.len());
    let mut out = Vec::with_capacity(notes);
    let mut delay = 0u64;
    for i in 0..notes {
        out.push((kind as u8, freqs[i], durations_ms[i], delay));
        delay += durations_ms[i];
    }
    out
}

/// Food pickup events: the two-note blip, both pitches shifted by the food
/// value (see the protocol comment above). Pure: unit-tested on native.
#[cfg(any(target_arch = "wasm32", test))]
fn food_events(value: u8) -> Vec<SfxEvent> {
    let shift = value as u32 * FOOD_VALUE_STEP_HZ;
    sequence_events(SfxKind::Food, &[880 + shift, 1320 + shift], &[70, 0])
}

/// The death riff as ONE queue event: freq = first note, dur = per-note step
/// (the tempo hint); web/audio.js sequences the four notes itself. Pure.
#[cfg(any(target_arch = "wasm32", test))]
fn death_riff_event(step_ms: u64) -> SfxEvent {
    (SfxKind::DeathRiff as u8, DEATH_RIFF_NOTES[0], step_ms, 0)
}

/// Hand-rolled JSON for the sfx queue: [[kind, freq_hz, dur_ms, delay_ms], ...].
/// Lives here (not wasm_api.rs) so the wire format is unit-testable on native
/// builds, where the wasm feature — and wasm_api.rs — is not compiled.
pub fn format_sfx_json(events: &[SfxEvent]) -> String {
    let mut s = String::from("[");
    for (i, ev) in events.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&format!("[{},{},{},{}]", ev.0, ev.1, ev.2, ev.3));
    }
    s.push(']');
    s
}

/// Sound effect — never blocks the game loop.
pub fn play_beep(kind: SfxKind, freq: u32, duration_ms: u64) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = std::io::Write::write_all(&mut std::io::stderr(), b"\x07");
        let _ = std::io::Write::flush(&mut std::io::stderr());
        let _ = (kind, freq, duration_ms); // pitch/duration need a real audio lib
    }
    #[cfg(target_arch = "wasm32")]
    sfx_queue::push((kind as u8, freq, duration_ms, 0));
}

/// Multi-note jingle (threaded bell on native; queued delays in browser).
pub fn play_beep_sequence(kind: SfxKind, freqs: &[u32], durations_ms: &[u64]) {
    let notes = freqs.len().min(durations_ms.len());
    if notes == 0 {
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = kind; // the bell has no patches
        let gaps: Vec<u64> = durations_ms[..notes].to_vec();
        std::thread::spawn(move || {
            for gap in gaps.iter().take(notes) {
                let _ = std::io::Write::write_all(&mut std::io::stderr(), b"\x07");
                let _ = std::io::Write::flush(&mut std::io::stderr());
                std::thread::sleep(Duration::from_millis(*gap));
            }
        });
    }
    #[cfg(target_arch = "wasm32")]
    {
        for ev in sequence_events(kind, freqs, durations_ms) {
            sfx_queue::push(ev);
        }
    }
}

/// The descending four-note death riff. Native keeps the threaded 4-bell
/// jingle (gap `step_ms` per note, final hold `2 * step_ms`); the browser
/// receives ONE DeathRiff event and web/audio.js sequences the notes itself.
fn play_death_riff(step_ms: u64) {
    #[cfg(not(target_arch = "wasm32"))]
    play_beep_sequence(
        SfxKind::DeathRiff,
        &DEATH_RIFF_NOTES,
        &[step_ms, step_ms, step_ms, step_ms * 2],
    );
    #[cfg(target_arch = "wasm32")]
    sfx_queue::push(death_riff_event(step_ms));
}

/// Food pickup jingle. The browser's two Food events carry the value as a
/// pitch shift (see the protocol comment above); native rings the same
/// two-bell blip as always (freqs unused on the terminal).
fn play_food_pickup(value: u8) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = value;
        play_beep_sequence(SfxKind::Food, &[880, 1320], &[70, 0]);
    }
    #[cfg(target_arch = "wasm32")]
    {
        for ev in food_events(value) {
            sfx_queue::push(ev);
        }
    }
}

#[cfg(test)]
mod sfx_tests {
    use super::*;

    #[test]
    fn event_encoders_tag_kind_value_and_tempo() {
        // Jingle: every note tagged with its kind, delays accumulate per duration.
        let evs = sequence_events(SfxKind::Detonate, &[110, 90, 70], &[110, 110, 110]);
        assert_eq!(
            evs,
            vec![
                (SfxKind::Detonate as u8, 110, 110, 0),
                (SfxKind::Detonate as u8, 90, 110, 110),
                (SfxKind::Detonate as u8, 70, 110, 220),
            ]
        );
        // Mismatched lengths truncate to the shorter, like the bell path.
        assert!(sequence_events(SfxKind::Laser, &[1800], &[]).is_empty());
        // Food: the value rides the pitch shift on both notes of the blip.
        let evs = food_events(9);
        assert_eq!(
            evs[0],
            (SfxKind::Food as u8, 880 + 9 * FOOD_VALUE_STEP_HZ, 70, 0)
        );
        assert_eq!(
            evs[1],
            (SfxKind::Food as u8, 1320 + 9 * FOOD_VALUE_STEP_HZ, 0, 70)
        );
        // Death riff: ONE event — first note + per-note step as the tempo hint.
        assert_eq!(
            death_riff_event(100),
            (SfxKind::DeathRiff as u8, DEATH_RIFF_NOTES[0], 100, 0)
        );
    }

    #[test]
    fn format_sfx_json_emits_typed_quads() {
        let evs = [
            (SfxKind::Laser as u8, 1800, 30, 0),
            (SfxKind::Food as u8, 920, 70, 0),
        ];
        assert_eq!(format_sfx_json(&evs), "[[2,1800,30,0],[0,920,70,0]]");
        assert_eq!(format_sfx_json(&[]), "[]");
    }
}
