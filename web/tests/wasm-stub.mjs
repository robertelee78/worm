// Test double for web/pkg/worm.js (the wasm-bindgen bundle). app-smoke.mjs
// imports this module directly to script sfx queues / game-over transitions;
// app.js receives the same instance via the loader redirect, so the control
// surface is shared.
export const stub = {
  sfx: [], // quads drained by the next sfx_json() call
  over: false, // drives is_over()
  calls: { update: 0, restart: 0, reset_match: 0, fire: 0, set_direction: [] },
  state: null, // full state object returned by state_json()
};

export function makeState(w, h) {
  return {
    w, h, frame: 1, time: 0, over: false, winner: null,
    score: 0, scores: [0, 0], foodEaten: [2, 3], wins: [0, 0], speed: 100,
    cycles: [
      { color: [0, 255, 255], pos: [[1, 1], [2, 1]], head: [2, 1], alive: true, dir: 3, score: 0 },
      { color: [255, 60, 60], pos: [[w - 2, h - 2], [w - 3, h - 2]], head: [w - 3, h - 2], alive: true, dir: 2, score: 0 },
    ],
    food: [[3, 3, 5]], powerups: [], bolts: [], bombs: [], particles: [],
    brain: {
      mem: [100, 256], observed: [100, 300], cap: 4000,
      acc: 0.5, lifetimeAcc: 0.5, roundAcc: 0.6, samples: [10, 200],
      conf: 0.5, active: 6, driver: 'knn', action: 'cutting off your next corner', pred: 3,
      last: { pred: 3, actual: 3, hit: true }, warm: [60, 60],
      habits: [0.2, 0.2, 0.2, 0.4], path: [[4, 4], [5, 4]],
      scores: [0, 0, 0, 0, 0, 0, 0.5], rank: [0, 0, 0, 0, 0, 0, 0.65],
      preds: [3, 3, 3, 2, 3, 2, 3],
      hits: [5, 5, 5, 4, 5, 4, 5], total: [10, 10, 10, 10, 10, 10, 10],
    },
  };
}

export default async function init() { /* wasm instantiation: no-op in the stub */ }

export class WasmGame {
  constructor(cols, rows, seed) {
    this.w = cols;
    this.h = rows;
    this.seed = seed;
    if (!stub.state) stub.state = makeState(cols, rows);
  }
  brain_load() { return false; } // fresh-brain path
  brain_save() { return new Uint8Array([1, 2, 3]); }
  set_direction(d) { stub.calls.set_direction.push(d); }
  fire() { stub.calls.fire++; return true; }
  is_over() { return stub.over; }
  reset_match() { stub.calls.reset_match++; stub.over = false; }
  restart() { stub.calls.restart++; stub.over = false; }
  frame_delay_ms() { return 50; }
  update() { stub.calls.update++; }
  sfx_json() {
    const q = stub.sfx;
    stub.sfx = [];
    return JSON.stringify(q);
  }
  state_json() { return JSON.stringify(stub.state); }
  grid() { return new Uint8Array(this.w * this.h); }
}
