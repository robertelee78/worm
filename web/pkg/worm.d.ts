/* tslint:disable */
/* eslint-disable */

export class WasmGame {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Restore a previously exported brain (deviceId-keyed corpus).
     *
     * A brain written by an older build is MIGRATED, not rejected: sections
     * whose schema this build no longer understands are dropped individually
     * while the player's habit priors and head-to-head record carry forward.
     * Call [`brain_restore_summary`](Self::brain_restore_summary) afterwards
     * for a line to show the player.
     */
    brain_load(bytes: Uint8Array): boolean;
    /**
     * Human-readable outcome of the last `brain_load` (empty before one).
     * Shown in the brain panel so a returning player can see the opponent
     * still remembers them — and, after a schema change, exactly what was
     * carried forward versus reset.
     */
    brain_restore_summary(): string;
    /**
     * True when the last `brain_load` had to discard some learned state.
     */
    brain_restore_was_partial(): boolean;
    /**
     * Per-player brain export → IndexedDB. Finalizes the finished
     * round's ledgers first (codex verification: the browser saves at
     * game over, BEFORE any restart — without this the session's last
     * round never persisted).
     */
    brain_save(): Uint8Array;
    fire(): boolean;
    fire_p2(): boolean;
    frame_delay_ms(): bigint;
    /**
     * Flat row-major cell grid (CellType as u8).
     */
    grid(): Uint8Array;
    is_over(): boolean;
    constructor(width: number, height: number, seed: bigint);
    /**
     * The finished round's ghost log — seed, board size, and both worms'
     * input streams. Enough to replay the round bit-identically offline,
     * which is how a real player's games become evaluation data (ADR-016).
     * Read at game over, before the next restart wipes it.
     */
    replay_json(): string;
    /**
     * New match: wipe the scoreboard too (the brain still persists — rps-ai
     * keeps its corpus across everything). The current winner is cleared
     * first so restart() doesn't bank the finished game into the fresh match.
     */
    reset_match(): void;
    /**
     * New match using the browser space available at this boundary.
     */
    reset_match_with_size(width: number, height: number): void;
    /**
     * Next game in the match (banks the session scoreboard, keeps the brain).
     */
    restart(): void;
    /**
     * Next game using the browser space available at this round boundary.
     */
    restart_with_size(width: number, height: number): void;
    /**
     * All-time rounds this brain has played against its human — persisted
     * with the portfolio, so it survives every session and schema change.
     */
    rounds_remembered(): number;
    /**
     * 0=Up 1=Down 2=Left 3=Right (180s rejected game-side).
     */
    set_direction(dir: number): void;
    set_direction_p2(dir: number): void;
    /**
     * HUMAN VS HUMAN: hand cycle 1 to a second keyboard. `learn` keeps
     * the observation pipeline running for player 0 (shadow learning —
     * the CPU studies from the bench; its strategy portfolio is
     * correctly NOT credited for rounds it never steered). learn=false
     * records nothing.
     */
    set_versus(on: boolean, learn: boolean): void;
    /**
     * Drain queued sound events as JSON [[kind, freq_hz, duration_ms, delay_ms], ...].
     * `kind` is `game::SfxKind` as u8 — the wire protocol is documented in game.rs.
     */
    sfx_json(): string;
    /**
     * Versioned per-frame entities, HUD, and frame-coherent brain telemetry.
     */
    state_json(): string;
    /**
     * One frame. False when the game is over.
     */
    update(): boolean;
}

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_wasmgame_free: (a: number, b: number) => void;
    readonly wasmgame_brain_load: (a: number, b: number, c: number) => number;
    readonly wasmgame_brain_restore_summary: (a: number) => [number, number];
    readonly wasmgame_brain_restore_was_partial: (a: number) => number;
    readonly wasmgame_brain_save: (a: number) => [number, number];
    readonly wasmgame_fire: (a: number) => number;
    readonly wasmgame_fire_p2: (a: number) => number;
    readonly wasmgame_frame_delay_ms: (a: number) => bigint;
    readonly wasmgame_grid: (a: number) => [number, number];
    readonly wasmgame_is_over: (a: number) => number;
    readonly wasmgame_new: (a: number, b: number, c: bigint) => number;
    readonly wasmgame_replay_json: (a: number) => [number, number];
    readonly wasmgame_reset_match: (a: number) => void;
    readonly wasmgame_reset_match_with_size: (a: number, b: number, c: number) => void;
    readonly wasmgame_restart: (a: number) => void;
    readonly wasmgame_restart_with_size: (a: number, b: number, c: number) => void;
    readonly wasmgame_rounds_remembered: (a: number) => number;
    readonly wasmgame_set_direction: (a: number, b: number) => void;
    readonly wasmgame_set_direction_p2: (a: number, b: number) => void;
    readonly wasmgame_set_versus: (a: number, b: number, c: number) => void;
    readonly wasmgame_sfx_json: (a: number) => [number, number];
    readonly wasmgame_state_json: (a: number) => [number, number];
    readonly wasmgame_update: (a: number) => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
