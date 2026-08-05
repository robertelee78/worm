// app-smoke.mjs — headless integration test for web/app.js.
//
// Stubs the browser globals app.js touches (DOM, canvas 2d, IndexedDB,
// localStorage, rAF, AudioContext), redirects its `./pkg/worm.js` import to a
// controllable WasmGame stub, then drives the boot + game loop manually and
// asserts the sfx routing table + lifecycle sounds. Run:
//   node web/tests/app-smoke.mjs
import { register } from 'node:module';
register('./loader-hooks.mjs', import.meta.url);

/* ---------------- browser-global stubs (before any app import) ---------------- */

let failures = 0;
function assert(cond, msg) {
  if (cond) console.log('  ok  ' + msg);
  else { failures++; console.error('FAIL  ' + msg); }
}
process.on('unhandledRejection', (e) => { failures++; console.error('FAIL  unhandledRejection:', e); });

const store = new Map();
globalThis.localStorage = {
  getItem: (k) => (store.has(k) ? store.get(k) : null),
  setItem: (k, v) => store.set(k, String(v)),
  removeItem: (k) => store.delete(k),
};

globalThis.indexedDB = {
  open() {
    const rq = { onupgradeneeded: null, onsuccess: null, onerror: null, result: null, error: null };
    queueMicrotask(() => {
      const db = {
        createObjectStore() { return {}; },
        transaction() {
          return {
            objectStore() {
              const once = (result) => {
                const r = { onsuccess: null, onerror: null, result };
                queueMicrotask(() => r.onsuccess && r.onsuccess());
                return r;
              };
              return { get: () => once(null), put: () => once(undefined) };
            },
          };
        },
      };
      rq.result = db;
      if (rq.onupgradeneeded) rq.onupgradeneeded();
      if (rq.onsuccess) rq.onsuccess();
    });
    return rq;
  },
};

// Canvas 2d context: a Proxy that absorbs every method call and property set.
// Gradient factories must return a stub with addColorStop (the render path
// builds radial-gradient food orbs).
function ctxProxy(canvasEl) {
  const gradient = { addColorStop: () => {} };
  return new Proxy(function () {}, {
    get: (t, p) => {
      if (p === 'canvas') return canvasEl;
      if (p === 'createRadialGradient' || p === 'createLinearGradient' || p === 'createPattern') {
        return () => gradient;
      }
      return () => undefined;
    },
    set: () => true,
    apply: () => undefined,
  });
}

function makeEl(id) {
  const listeners = {};
  const classes = new Set();
  if (id === 'over-overlay' || id === 'champion-overlay') classes.add('hidden'); // matches index.html
  const el = {
    id, textContent: '', innerHTML: '', className: '', dataset: {},
    children: [], lastChild: null, width: 0, height: 0, style: {},
    classList: {
      add: (c) => classes.add(c),
      remove: (c) => classes.delete(c),
      contains: (c) => classes.has(c),
    },
    addEventListener: (type, fn) => { listeners[type] = fn; },
    _listeners: listeners,
    getContext: () => ctxProxy(el),
    prepend(child) {
      this.children.unshift(child);
      this.lastChild = this.children[this.children.length - 1] ?? null;
    },
    removeChild(child) {
      const i = this.children.indexOf(child);
      if (i >= 0) this.children.splice(i, 1);
      this.lastChild = this.children[this.children.length - 1] ?? null;
    },
  };
  return el;
}

const elements = new Map();
let createdCount = 0;
globalThis.document = {
  getElementById(id) {
    if (!elements.has(id)) elements.set(id, makeEl(id));
    return elements.get(id);
  },
  createElement(tag) { return makeEl(`${tag}#${++createdCount}`); },
  querySelectorAll() { return []; }, // no touch buttons headless
};

const winListeners = {};
globalThis.window = {
  innerWidth: 1200, innerHeight: 900,
  addEventListener(type, fn) { winListeners[type] = fn; },
};

let rafQueue = [];
globalThis.requestAnimationFrame = (cb) => { rafQueue.push(cb); return rafQueue.length; };
globalThis.setInterval = () => 0; // autosave timer: never fire, never hold the loop open

// Minimal AudioContext: state 'running' so jingles execute their bodies.
class FakeParam {
  constructor(v = 0) { this.value = v; }
  setValueAtTime() {} linearRampToValueAtTime() {} exponentialRampToValueAtTime() {} setTargetAtTime() {}
}
class FakeNode {
  constructor() { this.gain = new FakeParam(1); this.frequency = new FakeParam(0); this.Q = new FakeParam(0); this.onended = null; }
  connect() {} disconnect() {} start() {} stop() {}
}
globalThis.AudioContext = class {
  constructor() { this.state = 'running'; this.currentTime = 0; this.sampleRate = 8000; this.destination = {}; }
  resume() { this.state = 'running'; return Promise.resolve(); }
  createGain() { return new FakeNode(); }
  createOscillator() { const n = new FakeNode(); n.type = ''; return n; }
  createBuffer(ch, len) { return { getChannelData: () => new Float32Array(len) }; }
  createBufferSource() { const n = new FakeNode(); n.buffer = null; n.loop = false; return n; }
  createBiquadFilter() { const n = new FakeNode(); n.type = ''; return n; }
};

/* ---------------- sfx spies (prototype wrap — shared module instance) ---------------- */

const { Sfx } = await import('../audio.js');
const sfxCalls = {
  food: [], powerup: 0, laser: 0, trishot: 0, bombPlant: 0, detonate: 0,
  wallPunch: 0, deathRiff: 0, roundStart: 0, champion: [], insertCoin: [],
  engineHum: [], play: [], unlock: 0,
};
const wrap = (m, rec) => {
  const orig = Sfx.prototype[m];
  Sfx.prototype[m] = function (...a) { rec(a, this); return orig.apply(this, a); };
};
wrap('food', (a) => sfxCalls.food.push(a));
wrap('champion', (a) => sfxCalls.champion.push(a));
// insertCoin is invoked at boot as a designed no-op (ctx locked) — record
// readiness at call time so the test can tell silent calls from sounded ones.
wrap('insertCoin', (a, self) => sfxCalls.insertCoin.push(self.ready));
wrap('engineHum', (a, self) => sfxCalls.engineHum.push({ args: a, hum: self.hum })); // hum seen BEFORE the call acts
wrap('play', (a) => sfxCalls.play.push(a));
wrap('unlock', () => sfxCalls.unlock++);
for (const m of ['powerup', 'laser', 'trishot', 'bombPlant', 'detonate', 'wallPunch', 'deathRiff', 'roundStart']) {
  wrap(m, () => sfxCalls[m]++);
}

const { stub } = await import('./wasm-stub.mjs');

/* ---------------- boot app.js and drive ---------------- */

await import('../app.js');
const tick = () => new Promise((r) => setTimeout(r, 0));
await tick(); await tick(); await tick(); // boot(): init() → brainRead() → rAF

let now = 1000;
function frames(n, stepMs = 100) {
  for (let i = 0; i < n; i++) {
    now += stepMs;
    for (const cb of rafQueue.splice(0)) cb(now);
  }
}

console.log('— boot lifecycle —');
assert(rafQueue.length === 1, 'game loop scheduled after boot');
assert(/^\d+ \/ \d+$/.test(elements.get('screen').style.aspectRatio), 'arena screen receives the logical board aspect ratio');
assert(sfxCalls.roundStart === 1, 'roundStart fired at boot');
assert(sfxCalls.insertCoin.length === 1 && sfxCalls.insertCoin[0] === false, 'boot insertCoin invoked but silent (AudioContext still locked)');
assert(sfxCalls.engineHum.some((c) => c.args[0] === true), 'engineHum(true) requested at round start');

frames(3);
assert(stub.calls.update > 0, 'game.update() driven by rAF loop');
assert(elements.get('bp-action').textContent === 'cutting off your next corner', 'brain panel separates final CPU action from prediction source');
assert(elements.get('bp-accnum').textContent.includes('n=10'), 'round prediction accuracy includes its sample size');
assert(elements.get('bp-lifetime').textContent.includes('n=200'), 'lifetime accuracy is labeled with its separate sample size');

console.log('— typed sfx routing —');
stub.sfx = [
  [2, 1800, 30, 0],   // Laser
  [0, 920, 70, 0],    // Food pair, first: value = (920-880)/40 = 1
  [0, 1360, 0, 70],   // Food pair, second: must be skipped
  [1, 660, 90, 0],    // PowerUp
  [3, 1200, 40, 0],   // TriShot
  [4, 180, 120, 0],   // BombPlant
  [5, 90, 300, 0],    // Detonate
  [6, 300, 60, 0],    // WallPunch
  [7, 440, 100, 0],   // DeathRiff (ONE event)
  [99, 555, 50, 0],   // unknown kind → raw fallback
  [440, 80, 0],       // legacy kindless triple → raw fallback
];
frames(1);
assert(sfxCalls.laser === 1, 'kind 2 → laser()');
assert(sfxCalls.food.length === 1 && sfxCalls.food[0][0] === 1, 'kind 0 pair → food(1) once (second of pair skipped)');
assert(sfxCalls.powerup === 1, 'kind 1 → powerup()');
assert(sfxCalls.trishot === 1, 'kind 3 → trishot()');
assert(sfxCalls.bombPlant === 1, 'kind 4 → bombPlant()');
assert(sfxCalls.detonate === 1, 'kind 5 → detonate()');
assert(sfxCalls.wallPunch === 1, 'kind 6 → wallPunch()');
assert(sfxCalls.deathRiff === 1, 'kind 7 → deathRiff()');
assert(sfxCalls.play.length === 2, 'raw play() only for unknown kind + legacy triple');
assert(sfxCalls.play[0].join(',') === '555,50,0', 'unknown kind 99 → play(555, 50, 0)');
assert(sfxCalls.play[1].join(',') === '440,80,0', 'legacy triple → play(440, 80, 0)');

console.log('— unlock + insert coin —');
winListeners.keydown({ code: 'ArrowUp', preventDefault() {} });
assert(sfxCalls.unlock === 1, 'AudioContext unlocked on first keydown');
assert(sfxCalls.insertCoin.filter((r) => r === true).length === 1, 'insertCoin actually sounded on the unlock gesture');
winListeners.keydown({ code: 'ArrowDown', preventDefault() {} });
assert(sfxCalls.insertCoin.filter((r) => r === true).length === 1, 'insertCoin not repeated on later keydowns');
assert(stub.calls.set_direction.join(',') === '0,1', 'direction input still routed');

console.log('— engine hum tracks speed, single oscillator —');
stub.state.speed = 90; frames(1); // creates the hum (first post-unlock frame)
stub.state.speed = 70; frames(1); // glide; spy now sees the created hum
stub.state.speed = 80; frames(1); // glide again
const humRefs = sfxCalls.engineHum.map((c) => c.hum).filter(Boolean);
assert(humRefs.length >= 2 && new Set(humRefs).size === 1, 'engineHum reuses one persistent oscillator (idempotent)');
assert(sfxCalls.engineHum.some((c) => c.args[0] === true && Math.abs(c.args[1] - 0.9) < 1e-9), 'speed 90 → engineHum(true, 0.9)');
assert(sfxCalls.engineHum.some((c) => c.args[0] === true && Math.abs(c.args[1] - 0.7) < 1e-9), 'speed 70 → engineHum(true, 0.7)');

console.log('— game over (non-champion) —');
stub.state.over = true; stub.state.winner = 1; stub.state.wins = [0, 1]; stub.over = true;
frames(1);
assert(!elements.get('over-overlay').classList.contains('hidden'), 'over overlay shown');
assert(sfxCalls.engineHum.some((c) => c.args[0] === false), 'engineHum(false) on game over');
assert(elements.get('history-body').children.length === 1, 'history row appended');
assert(elements.get('history-body').children[0].innerHTML.includes('n=10'), 'history preserves round prediction evidence');
assert(elements.get('history-body').children[0].innerHTML.includes('cutting off your next corner'), 'history preserves the final CPU action');

console.log('— next round —');
stub.state.over = false;
winListeners.keydown({ code: 'Enter', preventDefault() {} });
assert(sfxCalls.roundStart === 2, 'roundStart fired on nextRound');
assert(elements.get('over-overlay').classList.contains('hidden'), 'over overlay hidden again');
frames(2);
assert(sfxCalls.engineHum.filter((c) => c.args[0] === true).length >= 2, 'engine hum resumed in the new round');

console.log('— champion —');
stub.state.over = true; stub.over = true; stub.state.winner = 0; stub.state.wins = [3, 0];
frames(1);
assert(sfxCalls.champion.length === 1 && sfxCalls.champion[0][0] === true, 'champion fanfare with playerWon=true');
assert(!elements.get('champion-overlay').classList.contains('hidden'), 'champion overlay shown');
assert(elements.get('over-overlay').classList.contains('hidden'), 'plain over overlay NOT shown for champion');

console.log('— new match reset —');
elements.get('new-match-btn')._listeners.click();
assert(stub.calls.reset_match === 1, 'new-match resets the match');
assert(elements.get('champion-overlay').classList.contains('hidden'), 'champion overlay dismissed');
assert(sfxCalls.roundStart === 3, 'roundStart fired on new match');
assert(elements.get('history-body').innerHTML === '', 'history cleared');

console.log('— fire button —');
elements.get('fire-btn')._listeners.pointerdown({ preventDefault() {} });
assert(stub.calls.fire === 1, 'fire button still routes to game.fire()');

console.log('');
if (failures) {
  console.error(`SMOKE FAIL — ${failures} assertion(s) failed`);
  process.exit(1);
}
console.log('SMOKE PASS — all assertions held');
process.exit(0);
