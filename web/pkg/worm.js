/* @ts-self-types="./worm.d.ts" */

export class WasmGame {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        WasmGameFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_wasmgame_free(ptr, 0);
    }
    /**
     * Restore a previously exported brain (deviceId-keyed corpus).
     *
     * A brain written by an older build is MIGRATED, not rejected: sections
     * whose schema this build no longer understands are dropped individually
     * while the player's habit priors and head-to-head record carry forward.
     * Call [`brain_restore_summary`](Self::brain_restore_summary) afterwards
     * for a line to show the player.
     * @param {Uint8Array} bytes
     * @returns {boolean}
     */
    brain_load(bytes) {
        const ptr0 = passArray8ToWasm0(bytes, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.wasmgame_brain_load(this.__wbg_ptr, ptr0, len0);
        return ret !== 0;
    }
    /**
     * Human-readable outcome of the last `brain_load` (empty before one).
     * Shown in the brain panel so a returning player can see the opponent
     * still remembers them — and, after a schema change, exactly what was
     * carried forward versus reset.
     * @returns {string}
     */
    brain_restore_summary() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.wasmgame_brain_restore_summary(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * True when the last `brain_load` had to discard some learned state.
     * @returns {boolean}
     */
    brain_restore_was_partial() {
        const ret = wasm.wasmgame_brain_restore_was_partial(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Per-player brain export → IndexedDB.
     * @returns {Uint8Array}
     */
    brain_save() {
        const ret = wasm.wasmgame_brain_save(this.__wbg_ptr);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * @returns {boolean}
     */
    fire() {
        const ret = wasm.wasmgame_fire(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * @returns {bigint}
     */
    frame_delay_ms() {
        const ret = wasm.wasmgame_frame_delay_ms(this.__wbg_ptr);
        return BigInt.asUintN(64, ret);
    }
    /**
     * Flat row-major cell grid (CellType as u8).
     * @returns {Uint8Array}
     */
    grid() {
        const ret = wasm.wasmgame_grid(this.__wbg_ptr);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * @returns {boolean}
     */
    is_over() {
        const ret = wasm.wasmgame_is_over(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * @param {number} width
     * @param {number} height
     * @param {bigint} seed
     */
    constructor(width, height, seed) {
        const ret = wasm.wasmgame_new(width, height, seed);
        this.__wbg_ptr = ret;
        WasmGameFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * The finished round's ghost log — seed, board size, and both worms'
     * input streams. Enough to replay the round bit-identically offline,
     * which is how a real player's games become evaluation data (ADR-016).
     * Read at game over, before the next restart wipes it.
     * @returns {string}
     */
    replay_json() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.wasmgame_replay_json(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * New match: wipe the scoreboard too (the brain still persists — rps-ai
     * keeps its corpus across everything). The current winner is cleared
     * first so restart() doesn't bank the finished game into the fresh match.
     */
    reset_match() {
        wasm.wasmgame_reset_match(this.__wbg_ptr);
    }
    /**
     * New match using the browser space available at this boundary.
     * @param {number} width
     * @param {number} height
     */
    reset_match_with_size(width, height) {
        wasm.wasmgame_reset_match_with_size(this.__wbg_ptr, width, height);
    }
    /**
     * Next game in the match (banks the session scoreboard, keeps the brain).
     */
    restart() {
        wasm.wasmgame_restart(this.__wbg_ptr);
    }
    /**
     * Next game using the browser space available at this round boundary.
     * @param {number} width
     * @param {number} height
     */
    restart_with_size(width, height) {
        wasm.wasmgame_restart_with_size(this.__wbg_ptr, width, height);
    }
    /**
     * All-time rounds this brain has played against its human — persisted
     * with the portfolio, so it survives every session and schema change.
     * @returns {number}
     */
    rounds_remembered() {
        const ret = wasm.wasmgame_rounds_remembered(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * 0=Up 1=Down 2=Left 3=Right (180s rejected game-side).
     * @param {number} dir
     */
    set_direction(dir) {
        wasm.wasmgame_set_direction(this.__wbg_ptr, dir);
    }
    /**
     * Drain queued sound events as JSON [[kind, freq_hz, duration_ms, delay_ms], ...].
     * `kind` is `game::SfxKind` as u8 — the wire protocol is documented in game.rs.
     * @returns {string}
     */
    sfx_json() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.wasmgame_sfx_json(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Versioned per-frame entities, HUD, and frame-coherent brain telemetry.
     * @returns {string}
     */
    state_json() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.wasmgame_state_json(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * One frame. False when the game is over.
     * @returns {boolean}
     */
    update() {
        const ret = wasm.wasmgame_update(this.__wbg_ptr);
        return ret !== 0;
    }
}
if (Symbol.dispose) WasmGame.prototype[Symbol.dispose] = WasmGame.prototype.free;
function __wbg_get_imports() {
    const import0 = {
        __proto__: null,
        __wbg___wbindgen_throw_344f42d3211c4765: function(arg0, arg1) {
            throw new Error(getStringFromWasm0(arg0, arg1));
        },
        __wbindgen_init_externref_table: function() {
            const table = wasm.__wbindgen_externrefs;
            const offset = table.grow(4);
            table.set(0, undefined);
            table.set(offset + 0, undefined);
            table.set(offset + 1, null);
            table.set(offset + 2, true);
            table.set(offset + 3, false);
        },
    };
    return {
        __proto__: null,
        "./worm_bg.js": import0,
    };
}

const WasmGameFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_wasmgame_free(ptr, 1));

function getArrayU8FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint8ArrayMemory0().subarray(ptr / 1, ptr / 1 + len);
}

function getStringFromWasm0(ptr, len) {
    return decodeText(ptr >>> 0, len);
}

let cachedUint8ArrayMemory0 = null;
function getUint8ArrayMemory0() {
    if (cachedUint8ArrayMemory0 === null || cachedUint8ArrayMemory0.byteLength === 0) {
        cachedUint8ArrayMemory0 = new Uint8Array(wasm.memory.buffer);
    }
    return cachedUint8ArrayMemory0;
}

function passArray8ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 1, 1) >>> 0;
    getUint8ArrayMemory0().set(arg, ptr / 1);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}

let cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
cachedTextDecoder.decode();
const MAX_SAFARI_DECODE_BYTES = 2146435072;
let numBytesDecoded = 0;
function decodeText(ptr, len) {
    numBytesDecoded += len;
    if (numBytesDecoded >= MAX_SAFARI_DECODE_BYTES) {
        cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
        cachedTextDecoder.decode();
        numBytesDecoded = len;
    }
    return cachedTextDecoder.decode(getUint8ArrayMemory0().subarray(ptr, ptr + len));
}

let WASM_VECTOR_LEN = 0;

let wasmModule, wasmInstance, wasm;
function __wbg_finalize_init(instance, module) {
    wasmInstance = instance;
    wasm = instance.exports;
    wasmModule = module;
    cachedUint8ArrayMemory0 = null;
    wasm.__wbindgen_start();
    return wasm;
}

async function __wbg_load(module, imports) {
    if (typeof Response === 'function' && module instanceof Response) {
        if (typeof WebAssembly.instantiateStreaming === 'function') {
            try {
                return await WebAssembly.instantiateStreaming(module, imports);
            } catch (e) {
                const validResponse = module.ok && expectedResponseType(module.type);

                if (validResponse && module.headers.get('Content-Type') !== 'application/wasm') {
                    console.warn("`WebAssembly.instantiateStreaming` failed because your server does not serve Wasm with `application/wasm` MIME type. Falling back to `WebAssembly.instantiate` which is slower. Original error:\n", e);

                } else { throw e; }
            }
        }

        const bytes = await module.arrayBuffer();
        return await WebAssembly.instantiate(bytes, imports);
    } else {
        const instance = await WebAssembly.instantiate(module, imports);

        if (instance instanceof WebAssembly.Instance) {
            return { instance, module };
        } else {
            return instance;
        }
    }

    function expectedResponseType(type) {
        switch (type) {
            case 'basic': case 'cors': case 'default': return true;
        }
        return false;
    }
}

function initSync(module) {
    if (wasm !== undefined) return wasm;


    if (module !== undefined) {
        if (Object.getPrototypeOf(module) === Object.prototype) {
            ({module} = module)
        } else {
            console.warn('using deprecated parameters for `initSync()`; pass a single object instead')
        }
    }

    const imports = __wbg_get_imports();
    if (!(module instanceof WebAssembly.Module)) {
        module = new WebAssembly.Module(module);
    }
    const instance = new WebAssembly.Instance(module, imports);
    return __wbg_finalize_init(instance, module);
}

async function __wbg_init(module_or_path) {
    if (wasm !== undefined) return wasm;


    if (module_or_path !== undefined) {
        if (Object.getPrototypeOf(module_or_path) === Object.prototype) {
            ({module_or_path} = module_or_path)
        } else {
            console.warn('using deprecated parameters for the initialization function; pass a single object instead')
        }
    }

    if (module_or_path === undefined) {
        module_or_path = new URL('worm_bg.wasm', import.meta.url);
    }
    const imports = __wbg_get_imports();

    if (typeof module_or_path === 'string' || (typeof Request === 'function' && module_or_path instanceof Request) || (typeof URL === 'function' && module_or_path instanceof URL)) {
        module_or_path = fetch(module_or_path);
    }

    const { instance, module } = await __wbg_load(await module_or_path, imports);

    return __wbg_finalize_init(instance, module);
}

export { initSync, __wbg_init as default };
