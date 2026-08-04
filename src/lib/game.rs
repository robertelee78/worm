use std::time::Duration;
use crossterm::terminal::size;
use rand::rngs::StdRng;
use rand::SeedableRng;

pub const FRAME_DELAY_MS: u64 = 150;

#[derive(Clone, Copy, PartialEq, Debug)]
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
pub enum CellType {
    Empty,
    Wall,
    Player,
    CPU,
    Food,
    /// A hole punched through the arena wall — passable, leads to the outer corridor.
    Hole,
    /// A collectible power-up; the kind is looked up in WormGame::powerups (mirrors food).
    PowerUp,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PowerUpKind {
    /// Hitscan beam along facing; kills the opponent on contact, detonates bombs
    /// in its path, passes through trails and holes; stops at the first wall.
    Laser,
    /// Three bolts (straight + two diagonals), TRI_SHOT_RANGE cells each, die on walls.
    TriShot,
    /// Planted at the current cell; detonates after ~3s. Chebyshev radius
    /// BOMB_RADIUS_CELLS kills heads and clears trails; chains into other bombs.
    Bomb,
    /// Flies to the arena wall and punches a permanent Hole through it.
    WallPunch,
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

/// A planted bomb counting down to detonation.
#[derive(Clone, Debug)]
pub struct Bomb {
    pub x: u16,
    pub y: u16,
    pub fuse: u32,
}

pub const BOMB_RADIUS_CELLS: i16 = 10;
pub const BOMB_FUSE_MS: u64 = 3000;
pub const TRI_SHOT_RANGE: u8 = 7;
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

pub struct WormGame {
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
    /// Seeded RNG for deterministic benchmarks. None = thread RNG.
    pub rng: Option<StdRng>,
}

#[derive(Clone)]
pub struct CPUPlayRecord {
    pub player_move_pattern: String,
    pub cpu_move_pattern: String,
    pub outcome: i32,
    pub seq: u32,
}

impl WormGame {
    /// Random range helper — uses seeded RNG when set, thread RNG otherwise.
    pub fn rng_range<T, R>(&mut self, range: R) -> T
    where
        R: rand::distr::uniform::SampleRange<T>,
        T: rand::distr::uniform::SampleUniform,
    {
        use rand::RngExt;
        match self.rng.as_mut() {
            Some(rng) => rng.random_range(range),
            None => rand::rng().random_range(range),
        }
    }

    /// Random float in [a, b) — uses seeded RNG when set, thread RNG otherwise.
    pub fn rng_f32(&mut self, a: f32, b: f32) -> f32 {
        use rand::RngExt;
        match self.rng.as_mut() {
            Some(rng) => rng.random_range(a..b),
            None => rand::rng().random_range(a..b),
        }
    }

    pub fn new() -> Self {
        let dims = Dimensions::get_terminal_size();

        let center_x = dims.width / 2;
        let center_y = dims.height / 2;
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

        let width = dims.width;
        let height = dims.height;
        let grid = Self::build_grid(width, height);

        let mut game = Self {
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
            rng: None,
        };
        game.generate_food_items();
        game
    }

    /// Create a game with a seeded RNG for deterministic benchmarks.
    pub fn with_seed(seed: u64) -> Self {
        let dims = Dimensions::get_terminal_size();

        let center_x = dims.width / 2;
        let center_y = dims.height / 2;
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

        let width = dims.width;
        let height = dims.height;
        let grid = Self::build_grid(width, height);

        let mut game = Self {
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
            rng: Some(StdRng::seed_from_u64(seed)),
        };
        game.generate_food_items();
        game
    }

    /// Build the arena grid. Ring 0 (screen frame) is always Wall. When the
    /// terminal is big enough, ring 2 is the punchable arena wall and ring 1
    /// is the outer corridor — the pacman tunnel between punched holes.
    fn build_grid(width: u16, height: u16) -> Vec<Vec<CellType>> {
        let mut grid = vec![vec![CellType::Empty; width as usize]; height as usize];
        let corridor = width >= 10 && height >= 10;
        for y in 0..height {
            for x in 0..width {
                let frame = x == 0 || y == 0 || x == width - 1 || y == height - 1;
                let arena_wall = corridor
                    && (x == 2 || y == 2 || x == width - 3 || y == height - 3);
                if frame || arena_wall {
                    grid[y as usize][x as usize] = CellType::Wall;
                }
            }
        }
        grid
    }

    /// Whether this terminal is big enough for an outer corridor around the arena wall.
    pub fn has_corridor(&self) -> bool {
        self.width >= 10 && self.height >= 10
    }

    /// Ring 2 — the arena wall (punchable). Ring 0 is the outer frame (never punchable).
    pub fn is_arena_wall(&self, x: u16, y: u16) -> bool {
        self.has_corridor()
            && (x == 2 || y == 2 || x == self.width - 3 || y == self.height - 3)
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
            (2, self.width.saturating_sub(2), 2, self.height.saturating_sub(2))
        };
        let n = self.rng_range(1..=5);
        self.food_items.clear();
        for _ in 0..n {
            for _ in 0..200 {
                let x = self.rng_range(xlo..xhi);
                let y = self.rng_range(ylo..yhi);
                if self.grid[y as usize][x as usize] == CellType::Empty
                    && !self.food_items.iter().any(|(fx, fy, _)| *fx == x && *fy == y)
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
                if self.grid[y as usize][x as usize] == CellType::Empty {
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
            return false;
        }

        self.frame_count += 1;
        self.time += 1;

        // Update difficulty based on time
        self.difficulty = (self.time / 300 + 1) as u32;

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

        // Retract player tail first (unless owed growth cells) so the vacated
        // cell isn't a false self-collision.
        {
            let cycle = &mut self.cycles[self.player];
            if cycle.positions.len() > 1 && cycle.pending_growth == 0 {
                let tail = cycle.positions.pop().unwrap();
                self.grid[tail.1 as usize][tail.0 as usize] = CellType::Empty;
            }
        }

        // Pre-compute new positions for both cycles (immutable borrows)
        let (player_new, player_crashed) = {
            let cycle = &self.cycles[self.player];
            let (dx, dy) = cycle.direction.as_delta();
            let new_x = (cycle.head.0 as i16 + dx).max(0).min((self.width - 1) as i16) as u16;
            let new_y = (cycle.head.1 as i16 + dy).max(0).min((self.height - 1) as i16) as u16;

            let crashed = !self.passable(new_x, new_y);

            ((new_x, new_y), crashed)
        };

        // Player collision
        if player_crashed {
            // True head-on: the player rams the CPU's head cell while the CPU
            // simultaneously steps into the player's head cell. Both die -> draw
            // (the sequential player-first check would otherwise hand the CPU
            // the win). The CPU's intended move is taken from its current
            // direction; cpu_decide preserves it in every non-turn frame.
            let cpu_rams_back = self.cycles[1].alive
                && player_new == self.cycles[1].head
                && {
                    let cy = &self.cycles[1];
                    let (dx, dy) = cy.direction.as_delta();
                    let nx = (cy.head.0 as i16 + dx).max(0).min((self.width - 1) as i16) as u16;
                    let ny = (cy.head.1 as i16 + dy).max(0).min((self.height - 1) as i16) as u16;
                    (nx, ny) == self.cycles[0].head
                };
            self.add_impact_particles(player_new.0, player_new.1, self.cycles[self.player].color);
            if cpu_rams_back {
                self.add_impact_particles(self.cycles[1].head.0, self.cycles[1].head.1, self.cycles[1].color);
                self.cycles[1].alive = false;
                self.winner = None;
            } else {
                self.winner = Some(1);
            }
            self.cycles[self.player].alive = false;
            self.game_over = true;
            play_beep_sequence(&[440, 330, 220, 110], &[100, 100, 100, 200]);
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
            self.cycles[self.player].score += player_food_val as u32;
            self.score += player_food_val as u32 * 10;
        }

        // Move player head (grow by the food value eaten)
        {
            let cycle = &mut self.cycles[self.player];
            cycle.head = player_new;
            cycle.positions.insert(0, player_new);
            if player_food_val > 0 {
                cycle.pending_growth += player_food_val as u32;
            } else if cycle.pending_growth > 0 {
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
            play_beep(1560, 40);
        }

        if player_food_val > 0 {
            self.add_impact_particles(player_new.0, player_new.1, player_color);
            play_beep(880, 50);
            play_beep(1320, 50);
        }

        // CPU AI — faithful k-NN memory opponent (rps-ai mechanism).
        // The CPU fires a held power-up when the heuristic sees a good shot.
        if crate::cpu_ai::should_fire(self, 1) {
            self.fire_powerup(1);
            if self.game_over {
                return false;
            }
        }
        let cpu_dir = crate::cpu_ai::cpu_decide(self);
        self.cycles[1].change_direction(cpu_dir);

        // Retract CPU tail first (unless owed growth cells) so the vacated cell
        // isn't a false self-collision.
        {
            let cycle = &mut self.cycles[1];
            if cycle.positions.len() > 1 && cycle.pending_growth == 0 {
                let tail = cycle.positions.pop().unwrap();
                self.grid[tail.1 as usize][tail.0 as usize] = CellType::Empty;
            }
        }

        // Recompute CPU position after AI decision
        let (cpu_new, cpu_crashed) = {
            let cycle = &self.cycles[1];
            let (dx, dy) = cycle.direction.as_delta();
            let new_x = (cycle.head.0 as i16 + dx).max(0).min((self.width - 1) as i16) as u16;
            let new_y = (cycle.head.1 as i16 + dy).max(0).min((self.height - 1) as i16) as u16;

            let crashed = !self.passable(new_x, new_y);

            ((new_x, new_y), crashed)
        };

        let cpu_color = self.cycles[1].color;

        // CPU collision
        if cpu_crashed {
            // Both entered the same cell this frame: the player's crash check
            // ran first and the cell was empty then, so the player moved in,
            // and the CPU then stepped into the same cell. Both die -> draw.
            let same_cell = self.cycles[0].alive && cpu_new == self.cycles[0].head;
            // Learn: the chosen direction died immediately (reward 0).
            let obs = crate::cpu_ai::encode_situation(self, &self.cpu_brain);
            crate::cpu_ai::record_episode(&mut self.cpu_brain, obs, cpu_dir, 0, 0);
            self.add_impact_particles(cpu_new.0, cpu_new.1, self.cycles[1].color);
            if same_cell {
                self.add_impact_particles(self.cycles[0].head.0, self.cycles[0].head.1, self.cycles[0].color);
                self.cycles[0].alive = false;
            }
            self.cycles[1].alive = false;
            self.game_over = true;
            self.winner = if same_cell { None } else { Some(0) };
            play_beep_sequence(&[440, 330, 220, 110], &[100, 100, 100, 200]);
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
            self.cycles[1].score += cpu_food_val as u32;
            self.score += cpu_food_val as u32 * 10;
        }

        // If we've cleared the tray, spawn a fresh 1-5.
        if self.food_items.is_empty() {
            self.generate_food_items();
        }

        // Move CPU head (grow by the food value eaten)
        {
            let cycle = &mut self.cycles[1];
            cycle.head = cpu_new;
            cycle.positions.insert(0, cpu_new);
            if cpu_food_val > 0 {
                cycle.pending_growth += cpu_food_val as u32;
            } else if cycle.pending_growth > 0 {
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
            play_beep(1560, 40);
        }

        if cpu_food_val > 0 {
            self.add_impact_particles(cpu_new.0, cpu_new.1, cpu_color);
            play_beep(880, 50);
            play_beep(1320, 50);
        }

        // Record this round for learning — faithful to rps-ai: learn from what
        // happened, with a monotonic seq. The CPU survived `frames_since_cpu_move`
        // frames on this direction and ate cpu_food_val food.
        self.frames_since_cpu_move += 1;
        let obs = crate::cpu_ai::encode_situation(self, &self.cpu_brain);
        crate::cpu_ai::record_episode(
            &mut self.cpu_brain,
            obs,
            cpu_dir,
            self.frames_since_cpu_move,
            cpu_food_val,
        );
        self.frames_since_cpu_move = 0;

        // Player move history feeding the CPU tail (trailing-match bonus).
        self.cpu_brain
            .record_player_move(self.cycles[self.player].direction);

        // --- Opponent Model Learning ---
        // Encode the player-centric context and record the player's observed
        // next-direction so the k-NN opponent model can learn the transition.
        // (rps-ai learns from what the HUMAN played next, not what the AI did.)
        let player_ctx = crate::cpu_ai::encode_player_context(self, &self.cpu_brain.player_tail);
        crate::cpu_ai::record_player_episode(
            &mut self.cpu_brain,
            player_ctx,
            self.cycles[self.player].direction,
        );

        // Live projectiles and planted bombs (can end the game).
        self.advance_projectiles();
        self.tick_bombs();
        if self.game_over {
            return false;
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

    pub fn change_direction(&mut self, new_dir: Direction) {
        let current_dir = self.cycles[self.player].direction;
        match (current_dir, new_dir) {
            (Direction::Up, Direction::Down) | (Direction::Down, Direction::Up) => {}
            (Direction::Left, Direction::Right) | (Direction::Right, Direction::Left) => {}
            _ => self.cycles[self.player].direction = new_dir,
        }
    }

    pub fn restart(&mut self) {
        let dims = Dimensions::get_terminal_size();

        let center_x = dims.width / 2;
        let center_y = dims.height / 2;
        let spacing = 12;

        self.width = dims.width;
        self.height = dims.height;
        self.grid = Self::build_grid(dims.width, dims.height);
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
        self.winner = None;
        self.difficulty = 1;
        self.frame_count = 0;
        // Preserve the brain across restarts — this is in-game persistence.
        // rps-ai: "what someone opens with is a habit, it is stored."
        // The brain carries learned patterns from the previous game into the next,
        // so the CPU starts the new game with experience. Only reset the
        // CPU-sequence timer that gates recording (frames_since_cpu_move).
        self.frames_since_cpu_move = 0;
        self.cpu_history.clear();
        self.powerups.clear();
        self.projectiles.clear();
        self.bombs.clear();
        self.powerup_timer = 60;
        self.generate_food_items();
    }

    pub fn frame_delay(&self) -> Duration {
        // Speed increases over time (100ms → 35ms)
        let base_delay = 100u64;
        let speedup = (self.frame_count / 60) as u64 * 3;
        let delay = (base_delay - speedup).max(35);
        Duration::from_millis(delay)
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
        let (hx, hy) = self.cycles[who].head;
        let dir = self.cycles[who].direction;
        let (dx, dy) = dir.as_delta();
        match kind {
            PowerUpKind::Laser => {
                let beam = self.beam_cells(hx, hy, dx, dy);
                // The beam detonates any bombs caught in its path.
                for &(bx, by) in &beam {
                    if let Some(i) = self.bombs.iter().position(|b| b.x == bx && b.y == by) {
                        let b = self.bombs.remove(i);
                        self.detonate(b.x, b.y);
                    }
                }
                // Kill on contact with the opponent head.
                let opp = 1 - who;
                if self.cycles[opp].alive && beam.contains(&self.cycles[opp].head) {
                    let (ox, oy) = self.cycles[opp].head;
                    self.add_impact_particles(ox, oy, self.cycles[opp].color);
                    self.cycles[opp].alive = false;
                    self.game_over = true;
                    self.winner = Some(who);
                    play_beep_sequence(&[440, 330, 220, 110], &[80, 80, 80, 160]);
                }
                // Beam flash particles along the whole path.
                for &(bx, by) in &beam {
                    self.particles.push(Particle {
                        x: bx as f32,
                        y: by as f32,
                        vx: 0.0,
                        vy: 0.0,
                        lifetime: 6,
                        color: (255, 255, 120),
                    });
                }
                play_beep(1800, 30);
            }
            PowerUpKind::TriShot => {
                // Straight ahead plus the two forward diagonals.
                let dirs = [(dx, dy), (dx + dy, dy + dx), (dx - dy, dy - dx)];
                for (ddx, ddy) in dirs {
                    self.projectiles.push(Projectile {
                        x: hx,
                        y: hy,
                        dx: ddx,
                        dy: ddy,
                        steps_left: TRI_SHOT_RANGE,
                        from: who as u8,
                    });
                }
                play_beep(1200, 40);
            }
            PowerUpKind::Bomb => {
                let fuse = (BOMB_FUSE_MS / self.frame_delay().as_millis().max(1) as u64).max(8) as u32;
                self.bombs.push(Bomb { x: hx, y: hy, fuse });
                play_beep(220, 60);
            }
            PowerUpKind::WallPunch => {
                // Fly to the first wall cell; if it is the punchable arena wall, open a hole.
                let mut x = hx as i16;
                let mut y = hy as i16;
                loop {
                    x += dx;
                    y += dy;
                    if x < 0 || y < 0 || x >= self.width as i16 || y >= self.height as i16 {
                        break;
                    }
                    let (ux, uy) = (x as u16, y as u16);
                    if self.grid[uy as usize][ux as usize] == CellType::Wall {
                        if self.is_arena_wall(ux, uy) {
                            self.grid[uy as usize][ux as usize] = CellType::Hole;
                            self.add_impact_particles(ux, uy, (120, 255, 120));
                            play_beep(660, 60);
                        }
                        break;
                    }
                }
            }
        }
        true
    }

    /// Cells a straight beam passes through, stopping before the first wall/frame.
    fn beam_cells(&self, hx: u16, hy: u16, dx: i16, dy: i16) -> Vec<(u16, u16)> {
        let mut out = Vec::new();
        let mut x = hx as i16;
        let mut y = hy as i16;
        loop {
            x += dx;
            y += dy;
            if x < 0 || y < 0 || x >= self.width as i16 || y >= self.height as i16 {
                break;
            }
            let (ux, uy) = (x as u16, y as u16);
            if self.grid[uy as usize][ux as usize] == CellType::Wall {
                break;
            }
            out.push((ux, uy));
        }
        out
    }

    /// Advance live tri-shot bolts one cell; bolts die on walls or at max range,
    /// and kill any head they enter.
    pub fn advance_projectiles(&mut self) {
        let mut i = 0;
        while i < self.projectiles.len() {
            let (x, y, from) = {
                let p = &self.projectiles[i];
                (p.x as i16 + p.dx, p.y as i16 + p.dy, p.from)
            };
            let dead_cell = x < 0
                || y < 0
                || x >= self.width as i16
                || y >= self.height as i16
                || self.grid[y as usize][x as usize] == CellType::Wall;
            if dead_cell {
                self.projectiles.remove(i);
                continue;
            }
            let (ux, uy) = (x as u16, y as u16);
            let hit = (0..2).find(|&c| {
                let c = c as u8;
                c != from && self.cycles[c as usize].alive && self.cycles[c as usize].head == (ux, uy)
            });
            if let Some(c) = hit {
                self.add_impact_particles(ux, uy, self.cycles[c].color);
                self.cycles[c].alive = false;
                self.game_over = true;
                self.winner = Some(1 - c);
                play_beep_sequence(&[440, 330, 220, 110], &[80, 80, 80, 160]);
                self.projectiles.remove(i);
                continue;
            }
            let p = &mut self.projectiles[i];
            p.x = ux;
            p.y = uy;
            p.steps_left = p.steps_left.saturating_sub(1);
            if p.steps_left == 0 {
                self.projectiles.remove(i);
            } else {
                i += 1;
            }
        }
    }

    /// Tick bomb fuses and detonate any at zero (chain reactions included).
    pub fn tick_bombs(&mut self) {
        for b in &mut self.bombs {
            b.fuse = b.fuse.saturating_sub(1);
        }
        while let Some(i) = self.bombs.iter().position(|b| b.fuse == 0) {
            let b = self.bombs.remove(i);
            self.detonate(b.x, b.y);
        }
    }

    /// Detonate at (x,y): Chebyshev radius BOMB_RADIUS_CELLS kills heads, clears
    /// trails/food/power-ups, and chains into other armed bombs. Walls survive.
    fn detonate(&mut self, x: u16, y: u16) {
        self.add_impact_particles(x, y, (255, 120, 40));
        play_beep(110, 120);
        let r = BOMB_RADIUS_CELLS as i32;
        let (cx, cy) = (x as i32, y as i32);
        for yy in (cy - r)..=(cy + r) {
            for xx in (cx - r)..=(cx + r) {
                if xx < 0 || yy < 0 || xx >= self.width as i32 || yy >= self.height as i32 {
                    continue;
                }
                let (ux, uy) = (xx as u16, yy as u16);
                match self.grid[uy as usize][ux as usize] {
                    CellType::Player | CellType::CPU | CellType::Food => {
                        self.grid[uy as usize][ux as usize] = CellType::Empty;
                        self.food_items.retain(|&(fx, fy, _)| (fx, fy) != (ux, uy));
                    }
                    CellType::PowerUp => {
                        self.grid[uy as usize][ux as usize] = CellType::Empty;
                        self.powerups.retain(|&(px, py, _)| (px, py) != (ux, uy));
                    }
                    _ => {}
                }
                // Chain: other armed bombs caught in the blast go off too.
                for b in &mut self.bombs {
                    if b.x == ux && b.y == uy {
                        b.fuse = 0;
                    }
                }
            }
        }
        // Kill heads in the radius (both can die -> draw).
        let mut dead = [false; 2];
        for c in 0..2 {
            let (hx, hy) = self.cycles[c].head;
            let d = (hx as i32 - cx).abs().max((hy as i32 - cy).abs());
            if d <= r {
                dead[c] = true;
            }
        }
        if dead[0] || dead[1] {
            for c in 0..2 {
                if dead[c] && self.cycles[c].alive {
                    self.cycles[c].alive = false;
                    let (hx, hy) = self.cycles[c].head;
                    self.add_impact_particles(hx, hy, self.cycles[c].color);
                }
            }
            self.game_over = true;
            self.winner = match (dead[0], dead[1]) {
                (true, false) => Some(1),
                (false, true) => Some(0),
                _ => None,
            };
            play_beep_sequence(&[440, 330, 220, 110], &[100, 100, 100, 200]);
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
            (2, self.width.saturating_sub(2), 2, self.height.saturating_sub(2))
        };
        for _ in 0..200 {
            let x = self.rng_range(xlo..xhi);
            let y = self.rng_range(ylo..yhi);
            if self.grid[y as usize][x as usize] == CellType::Empty {
                let kind = match self.rng_range(0..4) {
                    0 => PowerUpKind::Laser,
                    1 => PowerUpKind::TriShot,
                    2 => PowerUpKind::Bomb,
                    _ => PowerUpKind::WallPunch,
                };
                self.powerups.push((x, y, kind));
                self.grid[y as usize][x as usize] = CellType::PowerUp;
                break;
            }
        }
    }

    /// Display name for a held power-up slot (HUD).
    pub fn powerup_name(kind: Option<PowerUpKind>) -> &'static str {
        match kind {
            Some(PowerUpKind::Laser) => "LASER",
            Some(PowerUpKind::TriShot) => "TRI-SHOT",
            Some(PowerUpKind::Bomb) => "BOMB",
            Some(PowerUpKind::WallPunch) => "PUNCH",
            None => "-",
        }
    }

    pub fn render(&self, stdout: &mut std::io::Stdout) {
        use crossterm::{
            cursor::MoveTo,
            execute,
            style::{Color, Print, ResetColor, SetForegroundColor},
            terminal::{Clear, ClearType},
        };
        

        execute!(stdout, Clear(ClearType::All), MoveTo(0, 0)).unwrap();

        let pulse = ((self.time as f32 * 0.2).sin() * 0.3 + 0.7) as f32;

        for y in 0..self.height as usize {
            let mut line = String::new();
            for x in 0..self.width as usize {
                let cell = self.grid[y][x];
                let (r, g, b, ch): (u8, u8, u8, char) = match cell {
                    CellType::Empty => (0, 0, 0, ' '),
                    CellType::Player => {
                        let player_head = self.cycles[0].head;
                        if (x, y) == (player_head.0 as usize, player_head.1 as usize) {
                            // Head: white core with cyan glow
                            (255, 255, 255, '●')
                        } else {
                            // Trail: cyan with varying intensity
                            (0, (255 as f32 * pulse) as u8, (255 as f32 * pulse) as u8, '█')
                        }
                    }
                    CellType::CPU => {
                        let cpu_head = self.cycles[1].head;
                        if (x, y) == (cpu_head.0 as usize, cpu_head.1 as usize) {
                            // CPU head: magenta white core
                            (255, 255, 255, '◆')
                        } else {
                            (200, 0, (200 as f32 * pulse) as u8, '▓')
                        }
                    }
                    CellType::Wall => (0, 80, 80, '·'),
                    CellType::Food => {
                        let num = self
                            .food_items
                            .iter()
                            .find(|&&(fx, fy, _)| fx as usize == x && fy as usize == y)
                            .map(|&(_, _, n)| n)
                            .unwrap_or(0);
                        let ch = char::from_digit(num as u32, 10).unwrap_or('?');
                        let hue = num as f32 * 18.0;
                        let (r, g, b) = hsv_to_rgb(hue, 1.0, 1.0);
                        let intensity = ((pulse * 255.0) as u8).max(100);
                         (r.max(intensity), g.max(intensity), b.max(intensity), ch)
                     }
                     CellType::Hole => (60, 60, 60, '·'),
                     CellType::PowerUp => {
                         let pu = self
                             .powerups
                             .iter()
                             .find(|&&(px, py, _)| px as usize == x && py as usize == y)
                             .map(|&(_, _, k)| k)
                             .unwrap_or(PowerUpKind::Laser);
                        let ch = match pu {
                            PowerUpKind::Laser => '⚡',
                            PowerUpKind::TriShot => '⚡',
                            PowerUpKind::Bomb => '💣',
                            PowerUpKind::WallPunch => '🔨',
                        };
                         (200, 200, 0, ch)
                     }
                 };

                if ch == ' ' {
                    line.push(' ');
                } else {
                    line.push_str(&format!("\x1b[38;2;{};{};{}m{}", r, g, b, ch));
                }
            }

            execute!(stdout, MoveTo(0, y as u16), Print(format!("\x1b[0m{}", line))).unwrap();
        }

        // Draw border
        execute!(
            stdout,
            SetForegroundColor(Color::Rgb { r: 0, g: 200, b: 255 }),
            MoveTo(0, 0),
            Print("╔"),
            MoveTo(self.width - 1, 0),
            Print("╗"),
            MoveTo(0, self.height - 1),
            Print("╚"),
            MoveTo(self.width - 1, self.height - 1),
            Print("╝"),
        ).unwrap();

        for x in 1..self.width - 1 {
            execute!(stdout, MoveTo(x, 0), Print("═")).unwrap();
            execute!(stdout, MoveTo(x, self.height - 1), Print("═")).unwrap();
        }
        for y in 1..self.height - 1 {
            execute!(stdout, MoveTo(0, y), Print("║")).unwrap();
            execute!(stdout, MoveTo(self.width - 1, y), Print("║")).unwrap();
        }

        // Draw particles
        for p in &self.particles {
            let x = p.x as u16;
            let y = p.y as u16;
            if x < self.width && y < self.height {
                let alpha = (p.lifetime as f32 / 40.0).min(1.0);
                let (r, g, b) = p.color;
                let fade_r = (r as f32 * alpha) as u8;
                let fade_g = (g as f32 * alpha) as u8;
                let fade_b = (b as f32 * alpha) as u8;
                if self.grid[y as usize][x as usize] == CellType::Empty {
                    execute!(stdout, MoveTo(x, y), SetForegroundColor(Color::Rgb { r: fade_r, g: fade_g, b: fade_b }), Print("·")).unwrap();
                }
            }
        }

        // Draw UI bar
        let bar_color = if self.game_over {
            Color::Rgb { r: 255, g: 85, b: 128 }
        } else {
            Color::Rgb { r: 0, g: 255, b: 255 }
        };

        execute!(
            stdout,
            SetForegroundColor(bar_color),
            MoveTo(0, self.height),
            Print(format!(
                "╔{}╗ P1 (CYAN): {:4} │ P2 (MAGENTA): {:4} │ SCORE: {:5} │ SPEED: {:3}% │ FOOD: {:2} │ TIME: {}",
                "═".repeat((self.width.saturating_sub(1)) as usize),
                self.cycles[0].score,
                self.cycles[1].score,
                self.score,
                100 - (self.frame_count / 60).min(50) as u32,
                self.food_items.len(),
                self.time,
            )),
        ).unwrap();

        // Bottom bar
        execute!(
            stdout,
            SetForegroundColor(Color::Rgb { r: 255, g: 85, b: 128 }),
            MoveTo(0, self.height + 1),
            Print("←→ ARROW KEYS or WASD: Move │ R: Restart │ Q: Quit │ EAT NUMBERS TO GROW • COLLIDE TO DIE"),
            ResetColor,
        ).unwrap();

        if self.game_over {
            let winner_text = match self.winner {
                Some(0) => "PLAYER WINS!",
                Some(1) => "CPU WINS!",
                _ => "DRAW!",
            };

            execute!(
                stdout,
                SetForegroundColor(Color::Rgb { r: 255, g: 85, b: 128 }),
                MoveTo(self.width / 2 - 8, self.height / 2 - 1),
                Print("═════════════════════"),
                MoveTo(self.width / 2 - 8, self.height / 2),
                Print(format!("  {}  ", winner_text)),
                MoveTo(self.width / 2 - 8, self.height / 2 + 1),
                Print("═════════════════════"),
                MoveTo(self.width / 2 - 10, self.height / 2 + 2),
                Print(format!("SCORE: P1={}  P2={}  │  Press R to restart, Q to quit", self.cycles[0].score, self.cycles[1].score)),
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
        let (w, h) = size().unwrap_or((120, 40));
        Self {
            width: w,
            height: h.saturating_sub(2),
        }
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

pub fn play_beep(freq: u32, duration_ms: u64) {
    // Terminal bell for sound effect
    let _ = std::io::Write::write_all(&mut std::io::stderr(), b"\x07");
    let _ = std::io::Write::flush(&mut std::io::stderr());
    let _ = freq; // Frequency would be used with actual audio lib
    if duration_ms > 0 {
        std::thread::sleep(Duration::from_millis(duration_ms));
    }
}

pub fn play_beep_sequence(freqs: &[u32], durations_ms: &[u64]) {
    for (freq, dur) in freqs.iter().zip(durations_ms.iter()) {
        play_beep(*freq, *dur);
    }
}
