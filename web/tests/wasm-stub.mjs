// Test double for web/pkg/worm.js (the wasm-bindgen bundle). app-smoke.mjs
// imports this module directly to script sfx queues / game-over transitions;
// app.js receives the same instance via the loader redirect, so the control
// surface is shared.
export const stub = {
  sfx: [], // quads drained by the next sfx_json() call
  over: false, // drives is_over()
  calls: {
    update: 0, restart: 0, restart_with_size: [], reset_match: 0,
    reset_match_with_size: [], fire: 0, set_direction: [],
  },
  state: null, // full state object returned by state_json()
};

export function makeState(w, h) {
  return {
    schemaVersion: 1,
    w, h, frame: 1, time: 0, over: false, winner: null,
    score: 0, scores: [0, 0], foodEaten: [2, 3], wins: [0, 0], speed: 100,
    cycles: [
      { color: [0, 255, 255], pos: [[1, 1], [2, 1]], head: [2, 1], alive: true, dir: 3, held: null, score: 0 },
      { color: [255, 60, 60], pos: [[w - 2, h - 2], [w - 3, h - 2]], head: [w - 3, h - 2], alive: true, dir: 2, held: null, score: 0 },
    ],
    food: [[3, 3, 5]], powerups: [], bolts: [], bombs: [], particles: [],
    cause: null,
    brain: {
      frame: 1,
      scored: { targetFrame: 1, sourceKey: 'knn', sourceName: 'Deep memory', sourceIndex: 6, predicted: 3, actual: 3, hit: true },
      decision: {
        frame: 1, heading: 2, reason: 'cutting off your next corner',
        forecast: { targetFrame: 1, sourceKey: 'knn', sourceName: 'Deep memory', sourceIndex: 6, predicted: 3, confidence: 0.5 },
        projection: { direction: 3, path: [[4, 4], [5, 4]] },
      },
      lastDecision: {
        frame: 1, heading: 2, reason: 'cutting off your next corner',
        forecast: { targetFrame: 1, sourceKey: 'knn', sourceName: 'Deep memory', sourceIndex: 6, predicted: 3, confidence: 0.5 },
        projection: { direction: 3, path: [[4, 4], [5, 4]] },
      },
      nextForecast: { targetFrame: 2, sourceKey: 'knn', sourceName: 'Deep memory', sourceIndex: 6, predicted: 3, confidence: 0.5 },
      accuracy: {
        round: { hits: 6, samples: 10, rate: 0.6 },
        lifetime: { hits: 100, samples: 200, rate: 0.5 },
      },
      memory: {
        survivalRetained: 100, opponentRetained: 256,
        survivalObserved: 100, opponentObserved: 300,
        capacity: 4000, warmSamples: 60, warmAt: 60, ready: true,
      },
      habits: [0.2, 0.2, 0.2, 0.4],
      models: [
        ['rep', 'Streak reader', 3, 0, 0, 5],
        ['pat', 'Pattern hunter', 3, 0, 0, 5],
        ['frq', 'Habit tracker', 3, 0, 0, 5],
        ['due', 'Rotation guesser', 2, 0, 0, 4],
        ['wlR', 'Wall reader · R', 3, 0, 0, 5],
        ['wlL', 'Wall reader · L', 2, 0, 0, 4],
        ['knn', 'Deep memory', 3, 0.5, 0.65, 5],
      ].map(([key, name, predicted, rawScore, effectiveScore, hits]) => ({
        key, name, predicted, rawScore, effectiveScore, hits, samples: 10,
      })),
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
  reset_match_with_size(w, h) {
    stub.calls.reset_match_with_size.push([w, h]);
    this.resize(w, h);
  }
  restart() { stub.calls.restart++; stub.over = false; }
  restart_with_size(w, h) {
    stub.calls.restart_with_size.push([w, h]);
    this.resize(w, h);
  }
  resize(w, h) {
    this.w = w;
    this.h = h;
    stub.over = false;
    stub.state = makeState(w, h);
  }
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
