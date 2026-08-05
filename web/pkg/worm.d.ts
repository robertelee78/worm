/* tslint:disable */
/* eslint-disable */

export class WasmGame {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Restore a previously exported brain (deviceId-keyed corpus).
     */
    brain_load(bytes: Uint8Array): boolean;
    /**
     * Per-player brain export → IndexedDB.
     */
    brain_save(): Uint8Array;
    fire(): boolean;
    frame_delay_ms(): bigint;
    /**
     * Flat row-major cell grid (CellType as u8).
     */
    grid(): Uint8Array;
    is_over(): boolean;
    constructor(width: number, height: number, seed: bigint);
    /**
     * New match: wipe the scoreboard too (the brain still persists — rps-ai
     * keeps its corpus across everything). The current winner is cleared
     * first so restart() doesn't bank the finished game into the fresh match.
     */
    reset_match(): void;
    /**
     * Next game in the match (banks the session scoreboard, keeps the brain).
     */
    restart(): void;
    /**
     * 0=Up 1=Down 2=Left 3=Right (180s rejected game-side).
     */
    set_direction(dir: number): void;
    /**
     * Drain queued sound events as JSON [[kind, freq_hz, duration_ms, delay_ms], ...].
     * `kind` is `game::SfxKind` as u8 — the wire protocol is documented in game.rs.
     */
    sfx_json(): string;
    /**
     * Per-frame entities + HUD + brain state as JSON (positions, food,
     * power-ups, bolts, bombs, particles, scores, ensemble panel).
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
    readonly wasmgame_brain_save: (a: number) => [number, number];
    readonly wasmgame_fire: (a: number) => number;
    readonly wasmgame_frame_delay_ms: (a: number) => bigint;
    readonly wasmgame_grid: (a: number) => [number, number];
    readonly wasmgame_is_over: (a: number) => number;
    readonly wasmgame_new: (a: number, b: number, c: bigint) => number;
    readonly wasmgame_reset_match: (a: number) => void;
    readonly wasmgame_restart: (a: number) => void;
    readonly wasmgame_set_direction: (a: number, b: number) => void;
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
