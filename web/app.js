// TRON Light Cycles — browser driver.
// wasm game loop (fixed-timestep), canvas render + phosphor bloom, WebAudio
// sfx, IndexedDB per-player brain keyed by a long-lived deviceId cookie.
import init, { WasmGame } from './pkg/worm.js';
import { Sfx } from './audio.js';
import { computeBoardLayout } from './layout.js';

let CELL = 14; // recomputed by boardDims() to fit the viewport
const MATCH_TARGET = 3;
const MODEL_NAMES = ['rep', 'pat', 'frq', 'due', 'wlR', 'wlL', 'knn'];
const DIR_GLYPH = ['▲', '▼', '◀', '▶'];
const CELL_EMPTY = 0, CELL_WALL = 1, CELL_PLAYER = 2, CELL_CPU = 3, CELL_FOOD = 4, CELL_HOLE = 5, CELL_POWERUP = 6;

/* ---------------- per-player identity + brain store ---------------- */

let deviceId = localStorage.getItem('worm_device_id');
if (!deviceId) {
  deviceId = (crypto.randomUUID ? crypto.randomUUID() : String(Date.now()) + Math.random());
  localStorage.setItem('worm_device_id', deviceId);
}

const dbPromise = new Promise((resolve, reject) => {
  const rq = indexedDB.open('worm_brain_db', 1);
  rq.onupgradeneeded = () => rq.result.createObjectStore('brains');
  rq.onsuccess = () => resolve(rq.result);
  rq.onerror = () => reject(rq.error);
});

async function brainRead() {
  try {
    const db = await dbPromise;
    return await new Promise((resolve) => {
      const rq = db.transaction('brains', 'readonly').objectStore('brains').get(deviceId);
      rq.onsuccess = () => resolve(rq.result || null);
      rq.onerror = () => resolve(null);
    });
  } catch { return null; }
}

async function brainWrite(bytes) {
  try {
    const db = await dbPromise;
    db.transaction('brains', 'readwrite').objectStore('brains').put(bytes, deviceId);
  } catch { /* persistence is best-effort */ }
}

/* ---------------- boot ---------------- */

const sfx = new Sfx();
const canvas = document.getElementById('game-canvas');
const ctx = canvas.getContext('2d');
const off = document.createElement('canvas');
const offCtx = off.getContext('2d');

let game = null;
let cols = 0, rows = 0;
let state = null;
let overHandled = false;
let roundMemoryStart = null;
const bombMaxFuse = new Map(); // "x,y" → max fuse seen (fuse arc countdown)

function boardDims() {
  // Reserve the brain column when it sits beside the arena. The resulting
  // logical board stays fixed for this game; CSS owns live browser resizing so
  // an orientation/viewport change never destroys the active round.
  const viewport = window.visualViewport || window;
  const layout = computeBoardLayout(viewport.width || window.innerWidth, viewport.height || window.innerHeight);
  ({ cols, rows, cell: CELL } = layout);
  return layout;
}

async function boot() {
  await init();
  const layout = boardDims();
  const seed = BigInt((Date.now() ^ (Math.random() * 0xffffffff)) >>> 0);
  game = new WasmGame(cols, rows, seed);
  canvas.width = off.width = cols * CELL;
  canvas.height = off.height = rows * CELL;
  // The screen, canvas, CRT layers, and overlays share one responsive box.
  // CSS scales this aspect-ratio box continuously without touching WasmGame.
  document.getElementById('screen').style.aspectRatio = `${layout.cols} / ${layout.rows}`;

  const saved = await brainRead();
  if (saved && game.brain_load(saved)) {
    setBrainStatus('brain restored from this device');
  } else {
    setBrainStatus('fresh brain — teach it by playing');
  }

  sfx.insertCoin(); // no-op pre-gesture; tryCoin() replays it after unlock
  roundStartAudio();
  requestAnimationFrame(loop);
}

/* ---------------- input ---------------- */

const KEYMAP = {
  ArrowUp: 0, KeyW: 0, KeyK: 0,
  ArrowDown: 1, KeyS: 1, KeyJ: 1,
  ArrowLeft: 2, KeyA: 2, KeyH: 2,
  ArrowRight: 3, KeyD: 3, KeyL: 3,
};

// PowerUpKind wire values from wasm state_json cycles[i].held (null = none).
const PU_NAMES = ['LASER', 'TRI-SHOT', 'BOMB', 'WALL-PUNCH'];

window.addEventListener('keydown', (e) => {
  sfx.unlock();
  tryCoin();
  if (e.code in KEYMAP) {
    game.set_direction(KEYMAP[e.code]);
    e.preventDefault();
  } else if (e.code === 'Space') {
    game.fire();
    e.preventDefault();
  } else if (e.code === 'KeyR' || e.code === 'Enter') {
    if (game.is_over() && !championVisible()) nextRound();
  } else if (e.code === 'KeyM') {
    if (sfx.bgm) sfx.bgm.toggleMute();
  }
});
window.addEventListener('pointerdown', () => { sfx.unlock(); tryCoin(); }, { once: true });

document.querySelectorAll('.tbtn[data-dir]').forEach((btn) => {
  btn.addEventListener('pointerdown', (e) => {
    e.preventDefault();
    game.set_direction(Number(btn.dataset.dir));
  });
});
document.getElementById('fire-btn').addEventListener('pointerdown', (e) => {
  e.preventDefault();
  game.fire();
});
document.getElementById('new-match-btn').addEventListener('click', () => {
  game.reset_match();
  document.getElementById('champion-overlay').classList.add('hidden');
  document.getElementById('history-body').innerHTML = '';
  overHandled = false;
  roundMemoryStart = null;
  roundStartAudio();
});

function championVisible() {
  return !document.getElementById('champion-overlay').classList.contains('hidden');
}

function nextRound() {
  game.restart();
  overHandled = false;
  roundMemoryStart = null;
  document.getElementById('over-overlay').classList.add('hidden');
  roundStartAudio();
}

/* ---------------- game loop ---------------- */

let last = performance.now();
let acc = 0;

function loop(now) {
  const dt = Math.min(now - last, 250);
  last = now;
  acc += dt;
  let delay = Number(game.frame_delay_ms());
  let steps = 0;
  while (acc >= delay && steps < 8) {
    if (!game.is_over()) {
      game.update();
      playSfx();
    }
    acc -= delay;
    steps++;
    delay = Number(game.frame_delay_ms());
    if (game.is_over() && !overHandled) onGameOver();
  }
  state = JSON.parse(game.state_json());
  updateHum(state);
  render(state);
  hud(state);
  requestAnimationFrame(loop);
}

// Typed sfx protocol (game.rs): [kind, freq_hz, dur_ms, delay_ms] quads.
// kind 0=Food (TWO events per pickup: 880+v*40 then 1320+v*40 — route only
// the first of each pair, recovering the value from its freq), 1=PowerUp,
// 2=Laser, 3=TriShot, 4=BombPlant, 5=Detonate, 6=WallPunch, 7=DeathRiff
// (ONE event — audio.js sequences the riff itself). Kind-less legacy triples
// and unknown kinds fall back to the raw square voice.
function playSfx() {
  const events = JSON.parse(game.sfx_json());
  let foodSeen = 0;
  for (const ev of events) {
    if (!Array.isArray(ev) || ev.length < 3) continue;
    if (ev.length < 4) { // legacy [freq, durMs, delayMs]
      sfx.play(ev[0], ev[1], ev[2]);
      continue;
    }
    const [kind, freq, dur, delay] = ev;
    switch (kind) {
      case 0: // Food: skip the second event of each pickup pair
        if (foodSeen % 2 === 0) sfx.food(Math.round((freq - 880) / 40));
        foodSeen++;
        break;
      case 1: sfx.powerup(); break;
      case 2: sfx.laser(); break;
      case 3: sfx.trishot(); break;
      case 4: sfx.bombPlant(); break;
      case 5: sfx.detonate(); break;
      case 6: sfx.wallPunch(); break;
      case 7: sfx.deathRiff(); break;
      default: sfx.play(freq, dur, delay); // unknown kind: raw voice
    }
  }
}

/* ---------------- lifecycle sounds (coin / round / engine hum) ---------------- */

// The boot-time insertCoin() is a no-op until a user gesture unlocks the
// AudioContext — retry inside the unlock-triggering listeners until it sounds.
let coinPlayed = false;
function tryCoin() {
  if (!coinPlayed && sfx.ready) {
    sfx.insertCoin();
    coinPlayed = true;
  }
}

// Round start (boot / nextRound / new match): jingle + engine hum at the
// round's speed. The hum call is a no-op pre-unlock; updateHum() re-asserts
// it on later frames via the !sfx.hum check.
let lastSpeed = -1; // state.speed last seen (100 down to 50), -1 = unknown
let humOn = false;
function roundStartAudio() {
  sfx.roundStart();
  sfx.engineHum(true, (lastSpeed >= 0 ? lastSpeed : 100) / 100);
  humOn = true;
  if (sfx.bgm) sfx.bgm.start(lastSpeed >= 0 ? lastSpeed : 60);
}

// Keep the engine hum tracking state.speed (audio.js takes a 0-1 fraction;
// state.speed is a 50-100 percentage). Idempotent — audio.js reuses a single
// oscillator; the !sfx.hum clause recovers from the pre-unlock no-op window.
function updateHum(s) {
  if (s.over) {
    if (humOn) {
      sfx.engineHum(false);
      humOn = false;
    }
  } else if (!humOn || !sfx.hum || s.speed !== lastSpeed) {
    sfx.engineHum(true, s.speed / 100);
    humOn = true;
  }
  if (s.over && sfx.bgm) sfx.bgm.stop();
  else if (sfx.bgm && s.speed !== lastSpeed) sfx.bgm.setSpeed(s.speed);
  lastSpeed = s.speed;
}

/* ---------------- game over / match ---------------- */

function onGameOver() {
  overHandled = true;
  sfx.engineHum(false); // covers both the round-over and champion overlays
  humOn = false;
  state = JSON.parse(game.state_json());
  brainWrite(game.brain_save());
  pushHistory(state);

  const youWon = state.winner === 0;
  const cpuWon = state.winner === 1;
  document.getElementById('over-text').textContent = youWon ? 'PLAYER WINS' : cpuWon ? 'CPU WINS' : 'DRAW';
  document.getElementById('over-text').className = 'over-text ' + (youWon ? 'glow-cyan' : 'glow-red');
  document.getElementById('over-sub').textContent =
    `FOOD P1=${state.foodEaten[0]} P2=${state.foodEaten[1]} · ${state.frame} frames` +
    (state.cause ? ` — ${state.cause}` : '');
  document.getElementById('over-brain').textContent =
    `BRAIN this round ${(state.brain.roundAcc * 100).toFixed(0)}%/${state.brain.samples[0]} · ` +
    `${MODEL_INFO[state.brain.active].name} · ${state.brain.action}`;

  if (state.wins[0] >= MATCH_TARGET || state.wins[1] >= MATCH_TARGET) {
    document.getElementById('champ-text').textContent =
      state.wins[0] >= MATCH_TARGET ? 'YOU are the champion!' : 'The COMPUTER is the champion!';
    document.getElementById('champ-text').className =
      'champ-text ' + (state.wins[0] >= MATCH_TARGET ? 'glow-cyan' : 'glow-red');
    document.getElementById('champion-overlay').classList.remove('hidden');
    sfx.champion(state.wins[0] >= MATCH_TARGET);
  } else {
    document.getElementById('over-overlay').classList.remove('hidden');
  }
}

/* ---------------- HUD ---------------- */

/* ---------------- CPU BRAIN panel (plain-language learning display) ---------------- */

const MODEL_INFO = MODEL_NAMES.map((key) => ({
  rep: { name: 'Streak reader', blurb: 'spots your repeats & turns' },
  pat: { name: 'Pattern hunter', blurb: 'spots your move patterns' },
  frq: { name: 'Habit tracker', blurb: 'your favourite move' },
  due: { name: 'Rotation guesser', blurb: 'your least-used move' },
  wlR: { name: 'Wall reader · R', blurb: 'right-hand wall habit' },
  wlL: { name: 'Wall reader · L', blurb: 'left-hand wall habit' },
  knn: { name: 'Deep memory', blurb: 'remembers similar situations' },
}[key]));

let modelRows = null;
let lastDriver = -1;

function buildBrainPanel() {
  const host = document.getElementById('bp-models');
  modelRows = MODEL_INFO.map((m) => {
    const row = document.createElement('div');
    row.className = 'bp-model';
    const name = document.createElement('div');
    name.className = 'bp-mname';
    const title = document.createElement('div');
    title.textContent = m.name;
    const blurb = document.createElement('div');
    blurb.className = 'blurb';
    blurb.textContent = m.blurb;
    name.prepend(blurb);
    name.prepend(title);
    const bar = document.createElement('div');
    bar.className = 'bp-mbar';
    const mark = document.createElement('div');
    mark.className = 'bp-mmark';
    bar.prepend(mark);
    const hit = document.createElement('div');
    hit.className = 'bp-mhit';
    const pred = document.createElement('div');
    pred.className = 'bp-mpred';
    const score = document.createElement('div');
    score.className = 'bp-mscore';
    // prepend stacks last-first: final order = name, pred, bar, score, hit
    row.prepend(hit);
    row.prepend(score);
    row.prepend(bar);
    row.prepend(pred);
    row.prepend(name);
    host.prepend(row);
    return { row, mark, hit, pred, score };
  });
  modelRows.reverse(); // prepend stacked the rows backwards
}

function hud(s) {
  document.getElementById('you-wins').textContent = s.wins[0];
  document.getElementById('cpu-wins').textContent = s.wins[1];

  if (!modelRows) buildBrainPanel();
  const b = s.brain;

  // Prediction in plain language.
  document.getElementById('bp-pred').textContent = b.pred === null ? '·' : DIR_GLYPH[b.pred];
  document.getElementById('bp-conf').style.width = `${(b.conf * 100).toFixed(0)}%`;
  document.getElementById('bp-confnum').textContent =
    `${(b.conf * 100).toFixed(0)}% · n=${b.total[b.active]}`;

  const last = b.last || { pred: null, actual: null, hit: null };
  const lastEl = document.getElementById('bp-last');
  if (last.hit === null) {
    lastEl.textContent = 'No prediction scored yet';
    lastEl.className = 'bp-last';
  } else {
    lastEl.textContent =
      `${last.hit ? '✓' : '✗'} predicted ${DIR_GLYPH[last.pred]} · you went ${DIR_GLYPH[last.actual]}`;
    lastEl.className = `bp-last ${last.hit ? 'hit' : 'miss'}`;
  }

  // Prediction source — the final movement reason is shown separately below.
  const driverEl = document.getElementById('bp-driver');
  driverEl.textContent = MODEL_INFO[b.active].name;
  if (b.active !== lastDriver && lastDriver !== -1) {
    const wrap = document.getElementById('bp-driver-wrap');
    wrap.classList.remove('flash');
    void wrap.offsetWidth; // retrigger the CSS animation
    wrap.classList.add('flash');
  }
  lastDriver = b.active;
  document.getElementById('bp-action').textContent = b.action;

  // Per-model forecast, effective selection score, and current-round hit rate.
  for (let i = 0; i < modelRows.length; i++) {
    const { row, mark, hit, pred, score: scoreEl } = modelRows[i];
    const rawScore = b.scores[i]; // -1..+1
    const rankScore = b.rank[i];  // includes Deep Memory's warm bonus
    mark.style.left = `${((rawScore + 1) / 2 * 100).toFixed(0)}%`;
    mark.className = `bp-mmark ${rawScore >= 0 ? 'pos' : 'neg'}`;
    hit.textContent = b.total[i] > 0 ? `${((b.hits[i] / b.total[i]) * 100).toFixed(0)}% · ${b.total[i]}` : '—';
    pred.textContent = b.preds[i] === null ? '·' : DIR_GLYPH[b.preds[i]];
    scoreEl.textContent = `${rankScore >= 0 ? '+' : ''}${rankScore.toFixed(2)}`;
    scoreEl.title = i === 6 && rankScore !== rawScore
      ? `raw ${rawScore.toFixed(2)} + warm-memory bonus`
      : 'quadratic recent score';
    row.className = `bp-model${i === b.active ? ' active' : ''}`;
  }

  const [warmNow, warmAt] = b.warm;
  document.getElementById('bp-warm').textContent = warmNow >= warmAt
    ? 'Deep memory READY'
    : `Deep memory warming — ${warmNow}/${warmAt} situations`;
  document.getElementById('bp-mem').textContent =
    `Lifetime moves observed: ${b.observed[1]} · retained: ${b.mem[1]}/${b.cap}`;
  const habitIdx = b.habits.indexOf(Math.max(...b.habits));
  document.getElementById('bp-habit').textContent =
    `Your strongest direction habit: ${DIR_GLYPH[habitIdx]} ${(b.habits[habitIdx] * 100).toFixed(0)}%`;
  document.getElementById('bp-accnum').textContent =
    `${(b.roundAcc * 100).toFixed(0)}% · n=${b.samples[0]}`;
  document.getElementById('bp-acc').style.width = `${(b.roundAcc * 100).toFixed(0)}%`;
  document.getElementById('bp-lifetime').textContent =
    `Lifetime ${(b.lifetimeAcc * 100).toFixed(0)}% · n=${b.samples[1]} · chance 25%`;

  if (roundMemoryStart === null) roundMemoryStart = b.observed[1];

  // Held power-up: without this the browser player fires blind while the
  // terminal HUD shows PWR continuously (native render parity).
  const held = s.cycles[0].held;
  document.getElementById('fire-btn').textContent =
    held == null ? 'FIRE' : `FIRE ${PU_NAMES[held]}`;

  if (s.over) return;
  const heldTxt = held == null ? '' : ` │ PWR ${PU_NAMES[held]} — SPACE fires`;
  document.getElementById('mid-status').textContent =
    `FOOD you ${s.foodEaten[0]} : cpu ${s.foodEaten[1]} │ frame ${s.frame} │ speed ${s.speed}%${heldTxt}`;
}

function pushHistory(s) {
  const tbody = document.getElementById('history-body');
  const tr = document.createElement('tr');
  const winner = s.winner === 0 ? 'You' : s.winner === 1 ? 'CPU' : 'Draw';
  const cls = s.winner === 0 ? 'you' : 'cpu';
  const memoryDelta = Math.max(0, s.brain.observed[1] - (roundMemoryStart ?? s.brain.observed[1]));
  tr.innerHTML =
    `<td>${tbody.children.length + 1}</td>` +
    `<td class="${cls}">${winner}</td>` +
    `<td>${s.cause || '—'}</td>` +
    `<td>${s.frame}</td>` +
    `<td>${s.foodEaten[0]}</td>` +
    `<td>${s.foodEaten[1]}</td>` +
    `<td>${(s.brain.roundAcc * 100).toFixed(0)}% · n=${s.brain.samples[0]}</td>` +
    `<td>${MODEL_INFO[s.brain.active].name}</td>` +
    `<td>${s.brain.action}</td>` +
    `<td>+${memoryDelta}</td>`;
  tbody.prepend(tr);
  while (tbody.children.length > 20) tbody.removeChild(tbody.lastChild);
}

function setBrainStatus(msg) {
  document.getElementById('brain-status').textContent = `brain: ${msg}`;
  document.getElementById('device-id').textContent = deviceId.slice(0, 8);
}
setInterval(() => {
  if (game && !game.is_over()) {
    brainWrite(game.brain_save());
    if (state) setBrainStatus(`saved • observed ${state.brain.observed[1]} • lifetime ${(state.brain.lifetimeAcc * 100).toFixed(0)}%/${state.brain.samples[1]}`);
  }
}, 10000);
window.addEventListener('pagehide', () => { if (game) brainWrite(game.brain_save()); });

/* ---------------- render ---------------- */

function render(s) {
  const g = game.grid();
  const w = s.w, h = s.h;
  offCtx.fillStyle = '#030604';
  offCtx.fillRect(0, 0, w * CELL, h * CELL);

  // walls / holes from the flat grid
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const c = g[y * w + x];
      if (c === CELL_WALL) {
        const frameWall = x === 0 || y === 0 || x === w - 1 || y === h - 1;
        offCtx.fillStyle = frameWall ? '#06201f' : '#0a3d3d';
        offCtx.fillRect(x * CELL, y * CELL, CELL, CELL);
        if (!frameWall) {
          offCtx.fillStyle = '#0f5757';
          offCtx.fillRect(x * CELL + 2, y * CELL + 2, CELL - 4, CELL - 4);
        }
      } else if (c === CELL_HOLE) {
        offCtx.strokeStyle = '#666';
        offCtx.beginPath();
        offCtx.arc(x * CELL + CELL / 2, y * CELL + CELL / 2, CELL / 3, 0, Math.PI * 2);
        offCtx.stroke();
      }
    }
  }

  // food: glowing morsel ORBS — radius proportional to value (bigger number,
  // bigger image; no digit shown). Hue tracks value like the terminal.
  for (const [fx, fy, v] of s.food) {
    const cx = fx * CELL + CELL / 2, cy = fy * CELL + CELL / 2;
    const r = CELL * (0.18 + 0.30 * (v / 9));
    const grad = offCtx.createRadialGradient(cx, cy, 1, cx, cy, r * 2.2);
    grad.addColorStop(0, `hsl(${v * 18}, 100%, 72%)`);
    grad.addColorStop(0.45, `hsl(${v * 18}, 100%, 45%)`);
    grad.addColorStop(1, 'rgba(0,0,0,0)');
    offCtx.fillStyle = grad;
    offCtx.beginPath();
    offCtx.arc(cx, cy, r * 2.2, 0, Math.PI * 2);
    offCtx.fill();
    offCtx.fillStyle = `hsl(${v * 18}, 100%, 78%)`;
    offCtx.beginPath();
    offCtx.arc(cx, cy, r, 0, Math.PI * 2);
    offCtx.fill();
  }

  // power-ups: procedural icons per kind (bolt / triple shot / bomb / punch)
  offCtx.textAlign = 'center';
  offCtx.textBaseline = 'middle';
  for (const [px, py, kind] of s.powerups) {
    const cx = px * CELL + CELL / 2, cy = py * CELL + CELL / 2, R = CELL / 2 - 1;
    if (kind === 0) {
      // LASER: jagged lightning bolt
      offCtx.fillStyle = '#fff34d';
      offCtx.beginPath();
      offCtx.moveTo(cx + R * 0.35, cy - R);
      offCtx.lineTo(cx - R * 0.45, cy + R * 0.15);
      offCtx.lineTo(cx - R * 0.05, cy + R * 0.15);
      offCtx.lineTo(cx - R * 0.35, cy + R);
      offCtx.lineTo(cx + R * 0.45, cy - R * 0.15);
      offCtx.lineTo(cx + R * 0.05, cy - R * 0.15);
      offCtx.closePath();
      offCtx.fill();
    } else if (kind === 1) {
      // TRI-SHOT: three fanning bolts
      offCtx.fillStyle = '#ffd24d';
      for (const a of [-0.6, 0, 0.6]) {
        offCtx.beginPath();
        offCtx.moveTo(cx, cy + R * 0.8);
        offCtx.lineTo(cx + Math.sin(a) * R - 2, cy - Math.cos(a) * R * 0.7);
        offCtx.lineTo(cx + Math.sin(a) * R + 2, cy - Math.cos(a) * R * 0.7);
        offCtx.closePath();
        offCtx.fill();
      }
    } else if (kind === 2) {
      // BOMB: dark sphere + spark
      offCtx.fillStyle = '#333';
      offCtx.beginPath(); offCtx.arc(cx, cy + 1, R * 0.75, 0, Math.PI * 2); offCtx.fill();
      offCtx.strokeStyle = '#888'; offCtx.beginPath();
      offCtx.moveTo(cx + R * 0.35, cy - R * 0.45); offCtx.quadraticCurveTo(cx + R * 0.6, cy - R, cx + R * 0.9, cy - R * 0.9);
      offCtx.stroke();
      offCtx.fillStyle = `hsl(${(performance.now() / 8) % 60}, 100%, 60%)`;
      offCtx.beginPath(); offCtx.arc(cx + R * 0.9, cy - R * 0.9, 2, 0, Math.PI * 2); offCtx.fill();
    } else {
      // WALLPUNCH: cracked brick
      offCtx.fillStyle = '#a0522d';
      offCtx.fillRect(cx - R * 0.8, cy - R * 0.55, R * 1.6, R * 1.1);
      offCtx.strokeStyle = '#ffd24d';
      offCtx.beginPath();
      offCtx.moveTo(cx - R * 0.3, cy - R * 0.55);
      offCtx.lineTo(cx - R * 0.05, cy - R * 0.1);
      offCtx.lineTo(cx - R * 0.25, cy + R * 0.15);
      offCtx.lineTo(cx + R * 0.15, cy + R * 0.55);
      offCtx.stroke();
    }
    // pickup halo so the tile reads from across the arena
    offCtx.strokeStyle = 'rgba(255, 230, 80, 0.5)';
    offCtx.strokeRect(px * CELL + 1, py * CELL + 1, CELL - 2, CELL - 2);
  }

  // BOMB DANGER ZONES — the whole point of a bomb is its 21×21 blast. Draw
  // the radius as a pulsing zone that intensifies as the fuse runs down, so
  // "I didn't see the bomb" never happens again.
  const BLAST = 10; // BOMB_RADIUS_CELLS (engine constant)
  for (const [bx, by, fuse] of s.bombs) {
    const key = `${bx},${by}`;
    const maxF = Math.max(bombMaxFuse.get(key) || 0, fuse);
    bombMaxFuse.set(key, maxF);
    const frac = fuse / maxF;
    const urgency = 1 - frac;
    const pulse = Math.sin(performance.now() * (0.005 + urgency * 0.022)) * 0.5 + 0.5;
    const x0 = (bx - BLAST) * CELL, y0 = (by - BLAST) * CELL, side = (2 * BLAST + 1) * CELL;
    offCtx.fillStyle = `rgba(255, 60, 0, ${0.04 + urgency * 0.15 * pulse})`;
    offCtx.fillRect(x0, y0, side, side);
    offCtx.strokeStyle = `rgba(255, 90, 0, ${0.28 + 0.6 * urgency * pulse})`;
    offCtx.lineWidth = 2;
    offCtx.strokeRect(x0, y0, side, side);
  }
  // forget detonated bombs (they leave the list)
  for (const key of bombMaxFuse.keys()) {
    if (!s.bombs.some(([bx, by]) => `${bx},${by}` === key)) bombMaxFuse.delete(key);
  }

  // The same five-frame path the CPU hunt layers consume. Cyan ghost cells
  // make the live prediction visible and falsifiable on the arena itself.
  for (let i = 0; i < s.brain.path.length; i++) {
    const [x, y] = s.brain.path[i];
    const alpha = Math.max(0.1, 0.34 - i * 0.045);
    offCtx.fillStyle = `rgba(0, 255, 255, ${alpha})`;
    offCtx.fillRect(x * CELL + CELL * 0.28, y * CELL + CELL * 0.28, CELL * 0.44, CELL * 0.44);
  }

  // trails with head→tail gradient
  for (let ci = 0; ci < 2; ci++) {
    const c = s.cycles[ci];
    const [r, gg, b] = c.color;
    const len = c.pos.length;
    for (let i = 1; i < len; i++) {
      const t = i / Math.max(len - 1, 1);
      const f = 1 - t * 0.8;
      offCtx.fillStyle = `rgb(${(r * f) | 0}, ${(gg * f) | 0}, ${(b * f) | 0})`;
      const [x, y] = c.pos[i];
      offCtx.fillRect(x * CELL + 1, y * CELL + 1, CELL - 2, CELL - 2);
    }
    // head: white triangle pointing along travel
    if (c.alive) {
      const [hx, hy] = c.head;
      const cx = hx * CELL + CELL / 2, cy = hy * CELL + CELL / 2, R = CELL / 2 - 1;
      const a = [ -Math.PI / 2, Math.PI / 2, Math.PI, 0 ][c.dir];
      offCtx.fillStyle = '#fff';
      offCtx.beginPath();
      offCtx.moveTo(cx + Math.cos(a) * R, cy + Math.sin(a) * R);
      offCtx.lineTo(cx + Math.cos(a + 2.5) * R, cy + Math.sin(a + 2.5) * R);
      offCtx.lineTo(cx + Math.cos(a - 2.5) * R, cy + Math.sin(a - 2.5) * R);
      offCtx.closePath();
      offCtx.fill();
    }
  }

  // bolts
  offCtx.fillStyle = '#ffff3c';
  for (const [bx, by] of s.bolts) {
    offCtx.fillRect(bx * CELL + 3, by * CELL + 3, CELL - 6, CELL - 6);
  }

  // bomb bodies: pulsing glow + dark core + fuse countdown arc (the ring is
  // the timer — it empties as detonation approaches)
  for (const [bx, by, fuse] of s.bombs) {
    const key = `${bx},${by}`;
    const maxF = bombMaxFuse.get(key) || fuse || 1;
    const frac = fuse / maxF;
    const urgency = 1 - frac;
    const hot = Math.sin(performance.now() * (0.004 + urgency * 0.02)) * 0.4 + 0.6;
    const cx = bx * CELL + CELL / 2, cy = by * CELL + CELL / 2, R = CELL / 2 - 1;
    offCtx.fillStyle = `rgba(255, ${(120 * hot) | 0}, 0, ${0.22 + 0.38 * urgency})`;
    offCtx.beginPath();
    offCtx.arc(cx, cy, R * 1.5, 0, Math.PI * 2);
    offCtx.fill();
    offCtx.fillStyle = '#1a1a1a';
    offCtx.beginPath();
    offCtx.arc(cx, cy, R * 0.7, 0, Math.PI * 2);
    offCtx.fill();
    offCtx.strokeStyle = `hsl(${frac * 120}, 100%, 55%)`;
    offCtx.lineWidth = 2.5;
    offCtx.beginPath();
    offCtx.arc(cx, cy, R * 1.05, -Math.PI / 2, -Math.PI / 2 + Math.PI * 2 * frac);
    offCtx.stroke();
  }

  // particles (additive)
  offCtx.globalCompositeOperation = 'lighter';
  for (const [px, py, pr, pg, pb, life] of s.particles) {
    const alpha = Math.min(life / 40, 1);
    offCtx.fillStyle = `rgba(${pr}, ${pg}, ${pb}, ${alpha})`;
    offCtx.fillRect(px * CELL - 1, py * CELL - 1, 3, 3);
  }
  offCtx.globalCompositeOperation = 'source-over';

  // present: crisp pass + phosphor bloom
  ctx.clearRect(0, 0, canvas.width, canvas.height);
  ctx.drawImage(off, 0, 0);
  ctx.globalAlpha = 0.30;
  ctx.filter = 'blur(3px)';
  ctx.drawImage(off, 0, 0);
  ctx.filter = 'none';
  ctx.globalAlpha = 1;
}

boot();
