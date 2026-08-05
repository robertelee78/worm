//! Browser bindings (wasm-pack, `--features wasm`). One `WasmGame` per page:
//! JS drives `update()` on a frame_delay_ms cadence, reads `grid()` +
//! `state_json()` to draw to canvas, drains `sfx_json()` for WebAudio, and
//! persists `brain_save()` bytes into IndexedDB under the deviceId cookie
//! (the per-player memory corpus — see AGENTS.md).

use wasm_bindgen::prelude::*;

use crate::{CpuBrain, Direction, WormGame};

fn dir_from_u8(v: u8) -> Option<Direction> {
    Some(match v {
        0 => Direction::Up,
        1 => Direction::Down,
        2 => Direction::Left,
        3 => Direction::Right,
        _ => return None,
    })
}

#[wasm_bindgen]
pub struct WasmGame {
    game: WormGame,
    /// Outcome of the last `brain_load` — what the migration kept vs reset.
    brain_restore: Option<crate::BrainRestore>,
}

#[wasm_bindgen]
impl WasmGame {
    #[wasm_bindgen(constructor)]
    pub fn new(width: u16, height: u16, seed: u64) -> Self {
        Self {
            game: WormGame::with_size_seed(width, height, seed),
            brain_restore: None,
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

    /// Next game using the browser space available at this round boundary.
    pub fn restart_with_size(&mut self, width: u16, height: u16) {
        self.game.restart_with_size(width, height);
    }

    /// New match: wipe the scoreboard too (the brain still persists — rps-ai
    /// keeps its corpus across everything). The current winner is cleared
    /// first so restart() doesn't bank the finished game into the fresh match.
    pub fn reset_match(&mut self) {
        self.game.winner = None;
        self.game.session_wins = [0, 0];
        self.game.restart();
    }

    /// New match using the browser space available at this boundary.
    pub fn reset_match_with_size(&mut self, width: u16, height: u16) {
        self.game.winner = None;
        self.game.session_wins = [0, 0];
        self.game.restart_with_size(width, height);
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

    /// Versioned per-frame entities, HUD, and frame-coherent brain telemetry.
    pub fn state_json(&self) -> String {
        crate::web_state::to_json(&self.game)
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
    ///
    /// A brain written by an older build is MIGRATED, not rejected: sections
    /// whose schema this build no longer understands are dropped individually
    /// while the player's habit priors and head-to-head record carry forward.
    /// Call [`brain_restore_summary`](Self::brain_restore_summary) afterwards
    /// for a line to show the player.
    pub fn brain_load(&mut self, bytes: &[u8]) -> bool {
        match CpuBrain::from_bytes_report(bytes) {
            Some((mut b, report)) => {
                self.brain_restore = Some(report);
                // A persisted corpus is longitudinal memory, not an unfinished
                // round. Never score its pending forecast against a new board.
                b.ensemble.reset_scores();
                b.last_opp_prediction = None;
                self.game.cpu_brain = b;
                self.game.cpu_telemetry = crate::CpuFrameTelemetry::default();
                self.game.round_last_cpu_decision = None;
                // A returning player must not face a CPU reset to tier 1.
                self.game.refresh_read_rate();
                true
            }
            None => false,
        }
    }

    /// Human-readable outcome of the last `brain_load` (empty before one).
    /// Shown in the brain panel so a returning player can see the opponent
    /// still remembers them — and, after a schema change, exactly what was
    /// carried forward versus reset.
    pub fn brain_restore_summary(&self) -> String {
        self.brain_restore
            .map(|r| r.summary())
            .unwrap_or_else(String::new)
    }

    /// True when the last `brain_load` had to discard some learned state.
    pub fn brain_restore_was_partial(&self) -> bool {
        self.brain_restore.map(|r| r.is_partial()).unwrap_or(false)
    }
}
