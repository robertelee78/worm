//! Browser bindings (wasm-pack, `--features wasm`). One `WasmGame` per page:
//! JS drives `update()` on a frame_delay_ms cadence, reads `grid()` +
//! `state_json()` to draw to canvas, drains `sfx_json()` for WebAudio, and
//! persists `brain_save()` bytes into IndexedDB under the deviceId cookie
//! (the per-player memory corpus — see AGENTS.md).

use wasm_bindgen::prelude::*;

use crate::game::PowerUpKind;
use crate::{CpuBrain, Direction, WormGame};

fn dir_u8(d: Direction) -> u8 {
    match d {
        Direction::Up => 0,
        Direction::Down => 1,
        Direction::Left => 2,
        Direction::Right => 3,
    }
}

fn dir_from_u8(v: u8) -> Option<Direction> {
    Some(match v {
        0 => Direction::Up,
        1 => Direction::Down,
        2 => Direction::Left,
        3 => Direction::Right,
        _ => return None,
    })
}

fn powerup_u8(k: PowerUpKind) -> u8 {
    match k {
        PowerUpKind::Laser => 0,
        PowerUpKind::TriShot => 1,
        PowerUpKind::Bomb => 2,
        PowerUpKind::WallPunch => 3,
    }
}

#[wasm_bindgen]
pub struct WasmGame {
    game: WormGame,
}

#[wasm_bindgen]
impl WasmGame {
    #[wasm_bindgen(constructor)]
    pub fn new(width: u16, height: u16, seed: u64) -> Self {
        Self {
            game: WormGame::with_size_seed(width, height, seed),
        }
    }

    /// One frame. False when the game is over.
    pub fn update(&mut self) -> bool {
        self.game.update()
    }

    pub fn frame_delay_ms(&self) -> u64 {
        self.game.frame_delay().as_millis() as u64
    }

    /// 0=Up 1=Down 2=Left 3=Right (180s rejected game-side).
    pub fn set_direction(&mut self, dir: u8) {
        if let Some(d) = dir_from_u8(dir) {
            self.game.change_direction(d);
        }
    }

    pub fn fire(&mut self) -> bool {
        self.game.fire_powerup(0)
    }

    /// Next game in the match (banks the session scoreboard, keeps the brain).
    pub fn restart(&mut self) {
        self.game.restart();
    }

    /// New match: wipe the scoreboard too (the brain still persists — rps-ai
    /// keeps its corpus across everything). The current winner is cleared
    /// first so restart() doesn't bank the finished game into the fresh match.
    pub fn reset_match(&mut self) {
        self.game.winner = None;
        self.game.session_wins = [0, 0];
        self.game.restart();
    }

    pub fn is_over(&self) -> bool {
        self.game.game_over
    }

    /// Flat row-major cell grid (CellType as u8).
    pub fn grid(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.game.width as usize * self.game.height as usize);
        for row in &self.game.grid {
            for cell in row {
                out.push(*cell as u8);
            }
        }
        out
    }

    /// Per-frame entities + HUD + brain state as JSON (positions, food,
    /// power-ups, bolts, bombs, particles, scores, ensemble panel).
    pub fn state_json(&self) -> String {
        let g = &self.game;
        let mut s = String::with_capacity(4096);
        s.push_str(&format!(
            "{{\"w\":{},\"h\":{},\"frame\":{},\"time\":{},\"over\":{},\"winner\":{},\"score\":{},\"scores\":[{},{}],\"foodEaten\":[{},{}],\"wins\":[{},{}],\"speed\":{},",
            g.width,
            g.height,
            g.frame_count,
            g.time,
            g.game_over,
            match g.winner {
                Some(w) => w.to_string(),
                None => "null".to_string(),
            },
            g.score,
            g.cycles[0].score,
            g.cycles[1].score,
            g.food_eaten_by[0],
            g.food_eaten_by[1],
            g.displayed_wins()[0],
            g.displayed_wins()[1],
            g.speed_pct(),
        ));
        s.push_str("\"cycles\":[");
        for (i, c) in g.cycles.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&format!(
                "{{\"head\":[{},{}],\"dir\":{},\"alive\":{},\"held\":{},\"color\":[{},{},{}],\"pos\":[",
                c.head.0,
                c.head.1,
                dir_u8(c.direction),
                c.alive,
                match c.held_powerup {
                    Some(k) => powerup_u8(k).to_string(),
                    None => "null".to_string(),
                },
                c.color.0,
                c.color.1,
                c.color.2,
            ));
            for (j, p) in c.positions.iter().enumerate() {
                if j > 0 {
                    s.push(',');
                }
                s.push_str(&format!("[{},{}]", p.0, p.1));
            }
            s.push_str("]}");
        }
        s.push_str("],\"food\":[");
        for (i, f) in g.food_items.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&format!("[{},{},{}]", f.0, f.1, f.2));
        }
        s.push_str("],\"powerups\":[");
        for (i, p) in g.powerups.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&format!("[{},{},{}]", p.0, p.1, powerup_u8(p.2)));
        }
        s.push_str("],\"bolts\":[");
        for (i, p) in g.projectiles.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&format!("[{},{},{},{}]", p.x, p.y, p.dx, p.dy));
        }
        s.push_str("],\"bombs\":[");
        for (i, b) in g.bombs.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&format!("[{},{},{}]", b.x, b.y, b.fuse));
        }
        s.push_str("],\"particles\":[");
        for (i, p) in g.particles.iter().take(300).enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&format!(
                "[{:.1},{:.1},{},{},{},{}]",
                p.x, p.y, p.color.0, p.color.1, p.color.2, p.lifetime
            ));
        }
        // Brain panel: explicitly-scoped accuracy, prediction source, final CPU
        // action, counterfactual forecasts, warm-up, memory, and player habits.
        let b = &g.cpu_brain;
        let e = &b.ensemble;
        let dir_json = |d: Option<Direction>| match d {
            Some(value) => dir_u8(value).to_string(),
            None => "null".to_string(),
        };
        let bool_json = |value: Option<bool>| match value {
            Some(value) => value.to_string(),
            None => "null".to_string(),
        };
        let habits = b.opp_brain.prior_distribution();
        s.push_str(&format!(
            "],\"cause\":{},\"brain\":{{",
            match g.death_cause {
                Some(c) => format!("\"{}\"", c.as_str()),
                None => "null".to_string(),
            },
        ));
        s.push_str(&format!(
            "\"mem\":[{},{}],\"observed\":[{},{}],\"cap\":{},\"acc\":{:.3},\"lifetimeAcc\":{:.3},\"roundAcc\":{:.3},\"samples\":[{},{}],\"conf\":{:.3},\"active\":{},\"driver\":\"{}\",\"action\":\"{}\",\"pred\":{},\"last\":{{\"pred\":{},\"actual\":{},\"hit\":{}}},\"warm\":[{},{}],\"habits\":[{:.4},{:.4},{:.4},{:.4}],\"path\":[",
            b.episodes.len(),
            b.opp_brain.episodes.len(),
            b.cpu_seq,
            b.opp_brain.seq,
            crate::cpu_ai::MAX_EPISODES,
            b.opp_pred_accuracy(),
            b.opp_pred_accuracy(),
            g.round_pred_accuracy(),
            g.round_pred_total,
            b.opp_pred_total,
            e.confidence,
            e.active,
            crate::cpu_ai::MODEL_NAMES[e.active],
            g.cpu_decision_reason.as_str(),
            dir_json(e.predicted_dir),
            dir_json(g.last_scored_prediction),
            dir_json(g.last_player_actual),
            bool_json(g.last_prediction_hit),
            b.opp_brain.episodes.len().min(crate::cpu_ai::COLD_START_EPISODES),
            crate::cpu_ai::COLD_START_EPISODES,
            habits[0], habits[1], habits[2], habits[3],
        ));
        for (i, (x, y)) in g.cpu_predicted_path.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&format!("[{},{}]", x, y));
        }
        s.push_str("],\"scores\":[");
        for i in 0..crate::cpu_ai::ENSEMBLE_MODELS {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&format!("{:.3}", e.score(i)));
        }
        s.push_str("],\"rank\":[");
        for i in 0..crate::cpu_ai::ENSEMBLE_MODELS {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&format!("{:.3}", crate::cpu_ai::ensemble_rank_score(b, i)));
        }
        s.push_str("],\"preds\":[");
        for i in 0..crate::cpu_ai::ENSEMBLE_MODELS {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&dir_json(e.pending[i]));
        }
        s.push_str("],\"hits\":[");
        for i in 0..crate::cpu_ai::ENSEMBLE_MODELS {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&format!("{}", e.hits[i]));
        }
        s.push_str("],\"total\":[");
        for i in 0..crate::cpu_ai::ENSEMBLE_MODELS {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&format!("{}", e.total[i]));
        }
        s.push_str("]}}");
        s
    }

    /// Drain queued sound events as JSON [[kind, freq_hz, duration_ms, delay_ms], ...].
    /// `kind` is `game::SfxKind` as u8 — the wire protocol is documented in game.rs.
    pub fn sfx_json(&mut self) -> String {
        crate::game::format_sfx_json(&crate::game::drain_sfx_events())
    }

    /// Per-player brain export → IndexedDB.
    pub fn brain_save(&self) -> Vec<u8> {
        self.game.cpu_brain.to_bytes()
    }

    /// Restore a previously exported brain (deviceId-keyed corpus).
    pub fn brain_load(&mut self, bytes: &[u8]) -> bool {
        match CpuBrain::from_bytes(bytes) {
            Some(b) => {
                self.game.cpu_brain = b;
                true
            }
            None => false,
        }
    }
}
