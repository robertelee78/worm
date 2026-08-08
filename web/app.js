// TRON Light Cycles — browser driver.
// wasm game loop (fixed-timestep), canvas render + phosphor bloom, WebAudio
// sfx, IndexedDB per-player brain keyed by a long-lived deviceId cookie.
// The ?v= must match BUILD below (and index.html's) — an unversioned glue
// import could pair a cached old worm.js with a fresh wasm on the next
// rebuild that changes the bindings.
import init, { WasmGame } from './pkg/worm.js?v=26';
import { Sfx } from './audio.js';
import { computeBoardLayout, VIEWPORT_BLOCK_GUTTER } from './layout.js';

let CELL = 14; // recomputed by applyBoardLayout() to fit the measured stage
// Bump together with the ?v= in index.html whenever the wasm bundle is
// rebuilt — it keys the cache-busting query on the .wasm fetch.
const BUILD = 26;
const MATCH_TARGET = 3;
const STATE_SCHEMA_VERSION = 2;
const ROUND_SCHEMA_VERSION = 1;
const MAX_ROUND_HISTORY = 200;
const MODEL_NAMES = ['rep', 'pat', 'frq', 'due', 'wlR', 'wlL', 'knn', 'eat', 'hunt', 'arm', 'eatW', 'huntW', 'armW', 'alt'];
const DIR_GLYPH = ['▲', '▼', '◀', '▶'];
const CELL_EMPTY = 0, CELL_WALL = 1, CELL_PLAYER = 2, CELL_CPU = 3, CELL_FOOD = 4, CELL_HOLE = 5, CELL_POWERUP = 6;

/* ---------------- per-player identity + brain store ---------------- */

// Resolved by resolveIdentity() before anything reads it. Deliberately not
// initialised from localStorage at module scope any more — see below.
let deviceId = null;

const dbPromise = new Promise((resolve, reject) => {
  const rq = indexedDB.open('worm_brain_db', 3);
  rq.onupgradeneeded = () => {
    const db = rq.result;
    if (!db.objectStoreNames.contains('brains')) db.createObjectStore('brains');
    if (!db.objectStoreNames.contains('rounds')) {
      const rounds = db.createObjectStore('rounds', { keyPath: 'id' });
      rounds.createIndex('deviceEnded', ['deviceId', 'endedAt']);
    }
    // v3: identity moves in here, beside the data it keys.
    if (!db.objectStoreNames.contains('meta')) db.createObjectStore('meta');
  };
  let timedOut = false;
  rq.onsuccess = () => {
    // If a LATER build bumps the version while this tab is open, close so the
    // other tab's upgrade isn't blocked forever.
    rq.result.onversionchange = () => rq.result.close();
    if (timedOut) {
      // The 3s timeout already rejected this promise: no consumer can ever
      // reach this connection, so holding it open would only block future
      // upgrades from other tabs.
      rq.result.close();
      return;
    }
    resolve(rq.result);
  };
  rq.onerror = () => reject(rq.error);
  // A v2→v3 upgrade fires `blocked` when any other tab still holds the old
  // version open. iOS Safari keeps background tabs alive and restores
  // prior-session tabs on launch, so a phone hits this where a desktop never
  // does — and without these two handlers the promise never settles and
  // boot() awaits forever: a silent black cabinet.
  rq.onblocked = () => reject(new Error('brain store blocked by another open tab — close other Worm tabs, then reload'));
  setTimeout(() => { timedOut = true; reject(new Error('brain store open timed out — reload to retry')); }, 3000);
});
// Degraded persistence must be SAID, not swallowed: every consumer catches
// and falls back, so without this line a dead brain store looks identical to
// a fresh player — and the defining feature (it remembers you) silently
// stops working for the whole session.
let dbDead = null;
dbPromise.catch((e) => { dbDead = e?.message || 'brain store unavailable'; });

/**
 * Resolve the player identity, preferring IndexedDB.
 *
 * The id used to live in localStorage while the brain it keys lives in
 * IndexedDB. Those have DIFFERENT EVICTION POLICIES — Safari's ITP clears
 * localStorage after seven days without interaction, IndexedDB survives — so a
 * player returning after a break got a freshly minted id, while the brain they
 * had spent twenty matches teaching sat unreachable under the old key,
 * consuming quota forever. No corruption required, just time. For a game whose
 * premise is that the opponent remembers you, that is the worst bug in the
 * codebase.
 *
 * Identity now lives beside the brain so the two share a fate. The
 * localStorage read is kept as a ONE-TIME ADOPTION: shipping this without it
 * would orphan every existing player in the very release that fixes orphaning.
 */
async function resolveIdentity() {
  const mint = () =>
    crypto.randomUUID ? crypto.randomUUID() : String(Date.now()) + Math.random();
  try {
    const db = await dbPromise;
    const stored = await new Promise((resolve) => {
      const rq = db.transaction('meta', 'readonly').objectStore('meta').get('profileId');
      rq.onsuccess = () => resolve(rq.result || null);
      rq.onerror = () => resolve(null);
    });
    if (stored) {
      deviceId = stored;
    } else {
      deviceId = localStorage.getItem('worm_device_id') || mint();
      db.transaction('meta', 'readwrite').objectStore('meta').put(deviceId, 'profileId');
    }
  } catch {
    // No IndexedDB at all — degrade rather than lose the session.
    deviceId = localStorage.getItem('worm_device_id') || mint();
  }
  // Mirrored for diagnostics and for the adoption path above. Never the
  // source of truth.
  try { localStorage.setItem('worm_device_id', deviceId); } catch { /* private mode */ }
  // Ask the browser not to evict us under storage pressure.
  try { navigator.storage?.persist?.(); } catch { /* not supported */ }
  return deviceId;
}

async function brainRead() {
  if (!deviceId) return null;
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
  if (!deviceId) return;
  try {
    const db = await dbPromise;
    db.transaction('brains', 'readwrite').objectStore('brains').put(bytes, deviceId);
  } catch { /* persistence is best-effort */ }
}

function validRound(record) {
  return record && record.schemaVersion === ROUND_SCHEMA_VERSION &&
    record.deviceId === deviceId && typeof record.id === 'string' &&
    Number.isFinite(record.endedAt) && Number.isInteger(record.frames) &&
    Array.isArray(record.foodEaten) && record.foodEaten.length === 2 &&
    record.accuracy && Number.isInteger(record.accuracy.samples) &&
    Array.isArray(record.models) && record.models.length >= 7;
}

async function roundsRead() {
  try {
    const db = await dbPromise;
    const records = await new Promise((resolve) => {
      const rq = db.transaction('rounds', 'readonly').objectStore('rounds').getAll();
      rq.onsuccess = () => resolve(rq.result || []);
      rq.onerror = () => resolve([]);
    });
    return records
      .filter(validRound)
      .map((record) => ({
        // Legacy normalization (review #13): old eras lack fields newer
        // render math assumes; NaN%/+undefined must never reach the DOM.
        ...record,
        memoryDelta: Number.isFinite(record.memoryDelta) ? record.memoryDelta : 0,
        accuracy: {
          rate: Number.isFinite(record.accuracy?.rate) ? record.accuracy.rate : 0,
          hits: Number.isFinite(record.accuracy?.hits) ? record.accuracy.hits : 0,
          samples: Number.isFinite(record.accuracy?.samples) ? record.accuracy.samples : 0,
        },
      }))
      .sort((a, b) => b.endedAt - a.endedAt)
      .slice(0, MAX_ROUND_HISTORY);
  } catch { return []; }
}

async function roundWrite(record) {
  if (!validRound(record)) return;
  try {
    const db = await dbPromise;
    const tx = db.transaction('rounds', 'readwrite');
    const store = tx.objectStore('rounds');
    store.put(record);
    const rq = store.getAll();
    rq.onsuccess = () => {
      rq.result
        .filter((candidate) => candidate.deviceId === deviceId)
        .sort((a, b) => b.endedAt - a.endedAt)
        .slice(MAX_ROUND_HISTORY)
        .forEach((candidate) => store.delete(candidate.id));
    };
  } catch { /* persistence is best-effort */ }
}

/* ---------------- the CPU's notebook (ADR-019): on-request deep explains ---------------- */
document.getElementById('tell-more-btn')?.addEventListener('click', async () => {
  const btn = document.getElementById('tell-more-btn');
  const notebook = document.getElementById('over-notebook');
  if (!lastRoundRecord || !btn || !notebook) return;
  btn.disabled = true;
  notebook.hidden = false;
  notebook.textContent = 'the CPU is opening its notebook… (about 15 seconds — it writes from its full record of you, every round you\u2019ve ever played)';
  try {
    const res = await fetch('/explain', { method: 'POST', body: JSON.stringify(lastRoundRecord) });
    notebook.textContent = res.ok
      ? await res.text()
      : 'the notebook is unavailable right now — the instant summary above is the measured truth anyway';
  } catch {
    notebook.textContent = 'the notebook is unavailable right now — the instant summary above is the measured truth anyway';
  }
  btn.textContent = '📓 notebook read';
});

/* ---------------- round collection (ADR-017, disclosed in the footer) ----------------
   Finished rounds send their ghost logs to the arcade owner's /collect
   endpoint — the moves of both worms and the round seed, nothing else.
   sendBeacon survives tab-close; fetch(keepalive) is the fallback; offline
   simply skips. Returning visitors backfill rounds saved by older builds
   once, tracked by id in the same database. */
function uploadRound(record) {
  // Fire-and-forget for the game-over moment: sendBeacon survives tab
  // close. Delivery is NOT confirmed here — the ledger is only written by
  // the confirmed backfill path, and ingestion dedups (deviceId, id) offline, so
  // a duplicate costs nothing and a dropped beacon retries next boot.
  try {
    const payload = JSON.stringify(record);
    if (!(navigator.sendBeacon &&
        navigator.sendBeacon('/collect', new Blob([payload], { type: 'application/json' })))) {
      fetch('/collect', { method: 'POST', body: payload, keepalive: true }).catch(() => {});
    }
  } catch { /* offline is fine */ }
}

// Merge ids into the uploaded-ledger inside ONE read-modify-write
// transaction — a plain get→put lost concurrent additions (review #12).
async function markUploaded(ids) {
  if (!ids.length) return;
  const db = await dbPromise;
  await new Promise((resolve) => {
    const tx = db.transaction('meta', 'readwrite');
    const store = tx.objectStore('meta');
    const rq = store.get('uploadedRounds');
    rq.onsuccess = () => {
      const merged = new Set(rq.result || []);
      ids.forEach((id) => merged.add(id));
      store.put([...merged].slice(-MAX_ROUND_HISTORY * 2), 'uploadedRounds');
    };
    tx.oncomplete = resolve;
    tx.onerror = resolve;
    tx.onabort = resolve;
  });
}

async function backfillUploads() {
  // Sequential, awaited, delivery-CONFIRMED: only a 2xx marks a round sent
  // (sendBeacon "true" means queued, and Safari's ~64KB in-flight quota
  // silently drops bulk beacons — review #9). One at a time also keeps a
  // 200-round history from flooding the collector.
  try {
    const db = await dbPromise;
    const sent = await new Promise((resolve) => {
      const rq = db.transaction('meta', 'readonly').objectStore('meta').get('uploadedRounds');
      rq.onsuccess = () => resolve(new Set(rq.result || []));
      rq.onerror = () => resolve(new Set());
    });
    const confirmed = [];
    for (const record of await roundsRead()) {
      if (sent.has(record.id)) continue;
      // Pre-ghost records have no replay — nothing evaluable to collect.
      if (!record.replay) { confirmed.push(record.id); continue; }
      try {
        const res = await fetch('/collect', {
          method: 'POST', body: JSON.stringify(record),
        });
        if (res.ok) confirmed.push(record.id);
      } catch { break; /* offline — retry whole tail next boot */ }
    }
    await markUploaded(confirmed);
  } catch { /* collection is best-effort, always */ }
}

/* ---------------- boot ---------------- */

const sfx = new Sfx();
const canvas = document.getElementById('game-canvas');
const ctx = canvas.getContext('2d');
const off = document.createElement('canvas');
const offCtx = off.getContext('2d');
const playColumn = document.getElementById('play-column');
const bezel = document.getElementById('arena-bezel');
const screen = document.getElementById('screen');
const touchControls = document.getElementById('touch-controls');

let game = null;
let cols = 0, rows = 0;
let state = null;
let overHandled = false;
let roundMemoryStart = null;
let lastRoundRecord = null;
let roundHistory = [];
const bombMaxFuse = new Map(); // "x,y" → max fuse seen (fuse arc countdown)

function px(value) {
  return Number.parseFloat(value) || 0;
}

function viewportBox() {
  const visual = window.visualViewport;
  return {
    width: Number(visual?.width) || document.documentElement.clientWidth || window.innerWidth,
    height: Number(visual?.height) || window.innerHeight,
    offsetTop: Number(visual?.offsetTop) || 0,
  };
}

// Measure the stage after CSS has resolved its real grid column. In particular,
// do not infer this from the outer browser window: Safari's visual viewport and
// embedded/responsive browser views can be materially smaller.
function measureArenaSpace() {
  const viewport = viewportBox();
  const playRect = playColumn.getBoundingClientRect();
  const playWidth = playRect.width || playColumn.clientWidth || viewport.width;
  const bezelStyle = getComputedStyle(bezel);
  const inlineChrome = px(bezelStyle.paddingLeft) + px(bezelStyle.paddingRight) +
    px(bezelStyle.borderLeftWidth) + px(bezelStyle.borderRightWidth);
  const blockChrome = px(bezelStyle.paddingTop) + px(bezelStyle.paddingBottom) +
    px(bezelStyle.borderTopWidth) + px(bezelStyle.borderBottomWidth);
  const controlsStyle = getComputedStyle(touchControls);
  const controlsHeight = controlsStyle.display === 'none'
    ? 0
    : touchControls.getBoundingClientRect().height;
  const visualWidth = Math.max(1, viewport.width - 20);

  return {
    viewport,
    playWidth,
    inlineChrome,
    blockChrome,
    controlsHeight,
    availableWidth: Math.max(1, Math.min(playWidth, visualWidth) - inlineChrome),
    availableHeight: Math.max(
      1,
      viewport.height - blockChrome - controlsHeight - VIEWPORT_BLOCK_GUTTER,
    ),
  };
}

function measureBoardLayout() {
  const space = measureArenaSpace();
  const layoutWidth = document.documentElement.clientWidth || window.innerWidth;
  return computeBoardLayout(layoutWidth, space.viewport.height, space);
}

// A live resize changes only CSS presentation. The logical board remains
// stable until the next round boundary, but its existing aspect ratio is fit
// into the exact width/height currently available to the complete play stage.
function refitArenaPresentation() {
  if (!cols || !rows) return;
  const space = measureArenaSpace();
  const aspect = cols / rows;
  const displayWidth = Math.min(space.availableWidth, space.availableHeight * aspect);
  const outerWidth = Math.min(space.playWidth, displayWidth + space.inlineChrome);
  bezel.style.width = `${Math.max(1, outerWidth)}px`;
  screen.dataset.displayWidth = String(Math.round(displayWidth));
  screen.dataset.displayHeight = String(Math.round(displayWidth / aspect));
}

function arenaIntersectsViewport() {
  const viewport = viewportBox();
  const rect = playColumn.getBoundingClientRect();
  return rect.bottom > viewport.offsetTop && rect.top < viewport.offsetTop + viewport.height;
}

function arenaFitsViewport() {
  const viewport = viewportBox();
  const rect = playColumn.getBoundingClientRect();
  const gutter = VIEWPORT_BLOCK_GUTTER / 2;
  return rect.top >= viewport.offsetTop + gutter - 1 &&
    rect.bottom <= viewport.offsetTop + viewport.height - gutter + 1;
}

function focusArena() {
  refitArenaPresentation();
  const viewport = viewportBox();
  const rect = playColumn.getBoundingClientRect();
  const desiredTop = viewport.offsetTop + Math.max(
    VIEWPORT_BLOCK_GUTTER / 2,
    (viewport.height - rect.height) / 2,
  );
  const delta = rect.top - desiredTop;
  if (Math.abs(delta) > 1) {
    window.scrollBy({ top: delta, left: 0, behavior: 'auto' });
  }
  playColumn.dataset.autoFocused = 'true';
}

let focusFrame = 0;
function scheduleArenaFocus() {
  if (focusFrame) return;
  focusFrame = requestAnimationFrame(() => {
    focusFrame = 0;
    focusArena();
  });
}

let refitFrame = 0;
function scheduleArenaRefit() {
  const wasVisible = arenaIntersectsViewport();
  if (refitFrame) return;
  refitFrame = requestAnimationFrame(() => {
    refitFrame = 0;
    refitArenaPresentation();
    if (wasVisible && !arenaFitsViewport()) focusArena();
  });
}

function applyBoardLayout(layout) {
  ({ cols, rows, cell: CELL } = layout);
  canvas.width = off.width = cols * CELL;
  canvas.height = off.height = rows * CELL;
  canvas.dataset.cols = String(cols);
  canvas.dataset.rows = String(rows);
  canvas.dataset.cell = String(CELL);
  screen.style.aspectRatio = `${cols} / ${rows}`;
  bombMaxFuse.clear();
  refitArenaPresentation();
}

async function boot() {
  // Cache-bust the wasm fetch alongside app.js's own ?v= (index.html): a
  // phone that cached an old bundle before a rebuild otherwise runs stale
  // code against a fresh wasm (or vice versa) with no way to hard-reload.
  await init({ module_or_path: `./pkg/worm_bg.wasm?v=${BUILD}` });
  // Must resolve before anything reads or writes a keyed store.
  await resolveIdentity();
  const layout = measureBoardLayout();
  applyBoardLayout(layout);
  const seed = BigInt((Date.now() ^ (Math.random() * 0xffffffff)) >>> 0);
  game = new WasmGame(cols, rows, seed);

  const saved = await brainRead();
  if (saved && game.brain_load(saved)) {
    // A brain saved by an older build is migrated, not discarded — the
    // summary says what carried forward so a returning player is never
    // silently handed a blank opponent.
    // Say the quiet part out loud: the memory is THIS BROWSER'S, and it
    // accumulates. A returning player must know they are facing everything
    // they have ever taught it — that IS the product, and a status line that
    // buries it reads as if every session starts cold.
    const remembered = game.rounds_remembered();
    setBrainStatus(
      remembered > 0
        ? `it remembers you — round ${remembered + 1} in this browser · ` +
          (game.brain_restore_summary() || 'memory restored')
        : (game.brain_restore_summary() || 'brain restored from this device'),
    );
  } else if (dbDead) {
    setBrainStatus(`⚠ memory unavailable — this session will not be remembered (${dbDead})`);
  } else {
    setBrainStatus('fresh brain — teach it by playing');
  }
  roundHistory = await roundsRead();
  renderHistory();
  backfillUploads();
  scheduleArenaFocus();

  sfx.insertCoin(); // no-op pre-gesture; tryCoin() replays it after unlock
  roundStartAudio();
  requestAnimationFrame(loop);
}

// A phone has no console. Any failure that would otherwise leave a silent
// black cabinet must land on screen instead — that turns "it's not working"
// into a bug report that names the actual cause.
function showFatal(message) {
  try {
    const mid = document.getElementById('mid-status');
    if (mid) mid.textContent = `ERROR: ${message}`;
    setBrainStatus(`boot failed: ${message}`);
  } catch { /* the DOM itself is broken — nothing left to do */ }
  // A phone has no console and a friend won't file a bug: the page reports
  // its own death (error text + build + user agent, nothing else).
  try {
    navigator.sendBeacon?.('/errors', new Blob([JSON.stringify({
      v: 1, build: BUILD, error: String(message).slice(0, 500),
      ua: navigator.userAgent.slice(0, 200), at: Date.now(),
    })], { type: 'application/json' }));
  } catch { /* reporting is best-effort */ }
}
window.addEventListener('error', (e) => showFatal(e.message || 'script error'));
window.addEventListener('unhandledrejection', (e) =>
  showFatal(e.reason?.message || String(e.reason || 'async failure')));

/* ---------------- input ---------------- */

const KEYMAP = {
  ArrowUp: 0, KeyW: 0, KeyK: 0,
  ArrowDown: 1, KeyS: 1, KeyJ: 1,
  ArrowLeft: 2, KeyA: 2, KeyH: 2,
  ArrowRight: 3, KeyD: 3, KeyL: 3,
};

// PowerUpKind wire values from wasm state_json cycles[i].held (null = none).
const PU_NAMES = ['LASER', 'TRI-SHOT', 'BOMB'];

window.addEventListener('keydown', (e) => {
  sfx.unlock();
  tryCoin();
  if (!game) return; // wasm module may still be booting
  if (e.code in KEYMAP) {
    game.set_direction(KEYMAP[e.code]);
    e.preventDefault();
  } else if (e.code === 'Space') {
    // Frozen-time laser kills are forbidden in the terminal client for a
    // reason; the browser now agrees (kimi-k3 #5).
    if (!paused) game.fire();
    e.preventDefault();
  } else if (e.code === 'KeyR' || e.code === 'Enter') {
    if (game.is_over() && !championVisible()) nextRound();
  } else if (e.code === 'KeyM') {
    if (sfx.bgm) sfx.bgm.toggleMute();
  } else if (e.code === 'KeyP') {
    paused = !paused;
  }
});
window.addEventListener('pointerdown', () => { sfx.unlock(); tryCoin(); }, { once: true });

document.querySelectorAll('.tbtn[data-dir]').forEach((btn) => {
  btn.addEventListener('pointerdown', (e) => {
    e.preventDefault();
    if (!game) return;
    game.set_direction(Number(btn.dataset.dir));
  });
});
document.getElementById('fire-btn').addEventListener('pointerdown', (e) => {
  e.preventDefault();
  if (!game || paused) return;
  game.fire();
});
// Touch players have no keyboard: without this button a phone/tablet player
// was permanently stuck on the first round-over screen (R/Enter only).
document.getElementById('next-round-btn').addEventListener('click', () => {
  if (game && game.is_over() && !championVisible()) nextRound();
});
document.getElementById('new-match-btn').addEventListener('click', () => {
  const layout = measureBoardLayout();
  game.reset_match_with_size(layout.cols, layout.rows);
  applyBoardLayout(layout);
  document.getElementById('champion-overlay').classList.add('hidden');
  overHandled = false;
  roundMemoryStart = null;
  roundStartAudio();
  scheduleArenaFocus();
});

function championVisible() {
  return !document.getElementById('champion-overlay').classList.contains('hidden');
}

function nextRound() {
  const layout = measureBoardLayout();
  game.restart_with_size(layout.cols, layout.rows);
  applyBoardLayout(layout);
  overHandled = false;
  roundMemoryStart = null;
  document.getElementById('over-overlay').classList.add('hidden');
  roundStartAudio();
  scheduleArenaFocus();
}

/* ---------------- game loop ---------------- */

function readState() {
  const parsed = JSON.parse(game.state_json());
  const models = parsed?.brain?.models;
  if (parsed?.schemaVersion !== STATE_SCHEMA_VERSION || !Array.isArray(models) ||
      models.length !== MODEL_NAMES.length ||
      models.some((model, index) => model.key !== MODEL_NAMES[index])) {
    throw new Error(`Unsupported Worm state schema: ${parsed?.schemaVersion ?? 'missing'}`);
  }
  return parsed;
}

let last = performance.now();
let acc = 0;
let paused = false;

function loop(now) {
  const dt = Math.min(now - last, 250);
  last = now;
  if (paused) {
    acc = 0; // don't bank frozen time — resume without a step burst
  } else {
    acc += dt;
  }
  let delay = Number(game.frame_delay_ms());
  let steps = 0;
  // At most TWO game-steps per painted frame. The old cap of 8 meant a
  // phone hitch (GC, audio, tab switch) could resolve most of a second of
  // gameplay between two paints — you could die in a configuration that
  // never appeared on screen, which play-tested as "killed by a tail that
  // didn't actually happen". Excess banked time is dropped below: game time
  // briefly slows instead of teleporting, and what renders is what happened.
  while (acc >= delay && steps < 2) {
    // Post-game, update() is a false-returning no-op EXCEPT that it keeps
    // aging the beam layer — the killing shot cools from core to embers
    // under the round-over overlay instead of glowing hot forever
    // (ADR-023 renderer contract; k3/codex v7 verify round 2).
    game.update();
    if (!game.is_over()) {
      playSfx();
    }
    acc -= delay;
    steps++;
    delay = Number(game.frame_delay_ms());
    if (steps === 2) acc = Math.min(acc, delay); // drop the backlog, don't teleport
    if (game.is_over() && !overHandled) {
      try {
        onGameOver();
      } catch (e) {
        showFatal(`${e.message} — please reload the page`);
        return;
      }
    }
  }
  // A schema mismatch here means a cached app.js is running against a newer
  // wasm (or vice versa). Throwing would kill the loop on frame 1 with a
  // frozen arena and no message — say "reload" instead.
  try {
    state = readState();
  } catch (e) {
    showFatal(`${e.message} — please reload the page`);
    return;
  }
  updateHum(state);
  // SLIPSTREAM (corridor half-time): the world slows and the screen says
  // so — cool, cheap, GPU-composited. Class toggle only; CSS owns the look.
  {
    const wrap = document.getElementById('game-canvas');
    const label = document.getElementById('slipstream-label');
    const on = !!state.slipstream;
    if (wrap) wrap.classList.toggle('slipstream', on);
    if (label) label.classList.toggle('hidden', !on);
  }
  render(state);
  hud(state);
  if (paused) {
    document.getElementById('mid-status').textContent = 'PAUSED — press P to resume';
  }
  requestAnimationFrame(loop);
}

// Typed sfx protocol (game.rs): [kind, freq_hz, dur_ms, delay_ms] quads.
// kind 0=Food (TWO events per pickup: 880+v*40 then 1320+v*40 — route only
// the first of each pair, recovering the value from its freq), 1=PowerUp,
// 2=Laser, 3=TriShot, 4=BombPlant, 5=Detonate, 6=Breach, 7=DeathRiff
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
  state = readState();
  brainWrite(game.brain_save());
  const record = roundRecord(state);
  roundHistory.unshift(record);
  roundHistory = roundHistory.slice(0, MAX_ROUND_HISTORY);
  renderHistory();
  roundWrite(record);
  uploadRound(record); // ledger updates only on CONFIRMED backfill delivery
  lastRoundRecord = record;
  const notebook = document.getElementById('over-notebook');
  const moreBtn = document.getElementById('tell-more-btn');
  if (notebook) { notebook.hidden = true; notebook.textContent = ''; }
  if (moreBtn) { moreBtn.disabled = false; moreBtn.textContent = '📓 TELL ME MORE'; }

  const youWon = state.winner === 0;
  const cpuWon = state.winner === 1;
  document.getElementById('over-text').textContent = youWon ? 'PLAYER WINS' : cpuWon ? 'CPU WINS' : 'DRAW';
  document.getElementById('over-text').className = 'over-text ' + (youWon ? 'glow-cyan' : 'glow-red');
  document.getElementById('over-sub').textContent =
    `FOOD P1=${state.foodEaten[0]} P2=${state.foodEaten[1]} · ${state.frame} frames` +
    (state.cause ? ` — ${state.cause}` : '');
  document.getElementById('over-brain').textContent =
    `BRAIN this round ${(state.brain.accuracy.round.rate * 100).toFixed(0)}%/${state.brain.accuracy.round.samples} · ` +
    `${state.brain.lastDecision?.forecast?.sourceName || 'no forecast'} · ${state.brain.lastDecision?.reason || 'no CPU decision'}`;

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

// Every key in MODEL_NAMES MUST have a row here. The `??` fallback is
// load-bearing: this table missing a key made `m.name` throw during panel
// build, which killed the module before the first frame — a completely
// frozen cabinet on every device, with the error handlers never installed.
const MODEL_INFO = MODEL_NAMES.map((key) => ({
  rep: { name: 'Streak reader', blurb: 'spots your repeats & turns' },
  pat: { name: 'Pattern hunter', blurb: 'spots your move patterns' },
  frq: { name: 'Habit tracker', blurb: 'your favourite move' },
  due: { name: 'Rotation guesser', blurb: 'your least-used move' },
  wlR: { name: 'Wall reader · R', blurb: 'right-hand wall habit' },
  wlL: { name: 'Wall reader · L', blurb: 'left-hand wall habit' },
  knn: { name: 'Deep memory', blurb: 'remembers similar situations' },
  eat: { name: 'Food-seeker', blurb: 'you\'re going for that food' },
  hunt: { name: 'Hunter', blurb: 'you\'re coming for me' },
  arm: { name: 'Arming-up', blurb: 'you\'re going for that weapon' },
  eatW: { name: 'Food-seeker · weaving', blurb: 'same errand, weaving there' },
  huntW: { name: 'Hunter · weaving', blurb: 'same hunt, weaving in' },
  armW: { name: 'Arming-up · weaving', blurb: 'same errand, weaving there' },
  alt: { name: 'Rhythm reader', blurb: 'your swerve cadence and which way next' },
}[key] ?? { name: key, blurb: '' }));

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
  const next = b.nextForecast;
  const decision = b.decision;

  // The forward-looking forecast is explicitly separate from the action that
  // already happened on this frame.
  document.getElementById('bp-pred').textContent = next?.predicted == null ? '·' : DIR_GLYPH[next.predicted];
  document.getElementById('bp-conf').style.width = `${((next?.confidence || 0) * 100).toFixed(0)}%`;
  document.getElementById('bp-confnum').textContent =
    `${((next?.confidence || 0) * 100).toFixed(0)}% · n=${next ? b.models[next.sourceIndex].samples : 0}`;

  const last = b.scored;
  const lastEl = document.getElementById('bp-last');
  if (!last) {
    lastEl.textContent = 'No prediction scored yet';
    lastEl.className = 'bp-last';
  } else {
    lastEl.textContent =
      `${last.hit ? '✓' : '✗'} ${last.sourceName} predicted ${DIR_GLYPH[last.predicted]} · you went ${DIR_GLYPH[last.actual]}`;
    lastEl.className = `bp-last ${last.hit ? 'hit' : 'miss'}`;
  }

  // Prediction source — the final movement reason is shown separately below.
  const driverEl = document.getElementById('bp-driver');
  driverEl.textContent = next?.sourceName || '—';
  const nextDriver = next?.sourceIndex ?? -1;
  if (nextDriver !== lastDriver && lastDriver !== -1) {
    const wrap = document.getElementById('bp-driver-wrap');
    wrap.classList.remove('flash');
    void wrap.offsetWidth; // retrigger the CSS animation
    wrap.classList.add('flash');
  }
  lastDriver = nextDriver;
  const displayedDecision = decision || b.lastDecision;
  const decisionEvidence = displayedDecision?.forecast?.sourceName
    ? ` · used ${displayedDecision.forecast.sourceName}`
    : '';
  const decisionAge = !decision && displayedDecision ? `last frame ${displayedDecision.frame} · ` : '';
  document.getElementById('bp-action').textContent = displayedDecision
    ? `${decisionAge}${displayedDecision.reason} ${DIR_GLYPH[displayedDecision.heading]}${decisionEvidence}`
    : 'no CPU decision this frame';

  // Per-model forecast, effective selection score, and current-round hit rate.
  for (let i = 0; i < modelRows.length; i++) {
    const { row, mark, hit, pred, score: scoreEl } = modelRows[i];
    const model = b.models[i];
    const rawScore = model.rawScore;
    const rankScore = model.effectiveScore;
    mark.style.left = `${((rawScore + 1) / 2 * 100).toFixed(0)}%`;
    mark.className = `bp-mmark ${rawScore >= 0 ? 'pos' : 'neg'}`;
    hit.textContent = model.samples > 0 ? `${((model.hits / model.samples) * 100).toFixed(0)}% · ${model.samples}` : '—';
    pred.textContent = model.predicted === null ? '·' : DIR_GLYPH[model.predicted];
    scoreEl.textContent = `${rankScore >= 0 ? '+' : ''}${rankScore.toFixed(2)}`;
    scoreEl.title = i === 6 && rankScore !== rawScore
      ? `raw ${rawScore.toFixed(2)} + warm-memory bonus`
      : 'quadratic recent score';
    row.className = `bp-model${i === nextDriver ? ' active' : ''}`;
  }

  const warmNow = b.memory.warmSamples;
  const warmAt = b.memory.warmAt;
  document.getElementById('bp-warm').textContent = b.memory.ready
    ? 'Deep memory READY'
    : `Deep memory warming — ${warmNow}/${warmAt} situations`;
  document.getElementById('bp-mem').textContent =
    `Lifetime moves observed: ${b.memory.opponentObserved} · retained: ${b.memory.opponentRetained}/${b.memory.capacity}`;
  const habitIdx = b.habits.indexOf(Math.max(...b.habits));
  document.getElementById('bp-habit').textContent =
    `Your strongest direction habit: ${DIR_GLYPH[habitIdx]} ${(b.habits[habitIdx] * 100).toFixed(0)}%`;
  // READ RATE — measured against the player's OWN base rate, not uniform
  // chance. Against a straight-driving player "always predict straight" scores
  // 98%, which against a 33% baseline would read as "it found a pattern". It
  // found nothing. Nothing renders until there are enough DISAGREEMENTS with
  // that baseline to say anything at all.
  const rr = b.readRate.round;
  const life = b.readRate.lifetime;
  const accEl = document.getElementById('bp-accnum');
  const barEl = document.getElementById('bp-acc');
  const chanceEl = document.getElementById('bp-chance');

  if (rr.discordant === 0) {
    accEl.textContent = 'no reads yet';
    barEl.style.width = '0%';
  } else if (!rr.ready) {
    accEl.textContent = `reading you — ${rr.discordant}/${rr.minDiscordant} disagreements`;
    barEl.style.width = `${((rr.discordant / rr.minDiscordant) * 100).toFixed(0)}%`;
  } else {
    accEl.textContent =
      `${(rr.rate * 100).toFixed(0)}% vs your usual ${(rr.baseRate * 100).toFixed(0)}%` +
      (rr.significant ? ' — beating your habits' : ' — not yet past your habits');
    barEl.style.width = `${(rr.rate * 100).toFixed(0)}%`;
  }
  // The marker is MEASURED, never the hardcoded 25% that used to sit here —
  // a reversal is never legal, so uniform chance is at most 1/3.
  if (rr.ready) {
    chanceEl.style.left = `${(rr.baseRate * 100).toFixed(1)}%`;
    chanceEl.title = `${(rr.baseRate * 100).toFixed(0)}% — what always guessing your commonest move would score`;
    chanceEl.classList.remove('hidden');
  } else {
    chanceEl.classList.add('hidden');
  }

  // The earned-evidence line names its SOURCE: difficulty may be funded by
  // the published forecast's channels OR by the turn book's precommitted
  // side calls — claiming "forecast performance" when the book earned it
  // would be a small lie in the one panel whose job is being believed.
  const bk = b.book || {};
  const earnedBits = bk.earned > 0
    ? ` · earned ${(bk.earned * 100).toFixed(0)}% via ${bk.earnedSource === 'book'
        ? `its turn book (${(bk.sideAccuracy * 100).toFixed(0)}% on your real turns)`
        : 'its published forecasts'}`
    : '';
  document.getElementById('bp-lifetime').textContent =
    !life.ready
      ? `Lifetime — reading you, ${life.discordant}/${life.minDiscordant} disagreements` + earnedBits
      : `Lifetime ${(life.rate * 100).toFixed(0)}% vs usual ${(life.baseRate * 100).toFixed(0)}%` +
        ` · lift ${(life.lift * 100).toFixed(0)}% · tier ${b.difficulty}` +
        (life.significant ? ` (1 in ${Math.max(1, Math.round(1 / Math.max(life.pValue, 1e-9)))} this is luck)` : '') +
        earnedBits;

  // PLAIN-ENGLISH EXPLAINER. Every line cites a LIVE number — an explainer
  // full of static prose is marketing, not explanation.
  const ex = document.getElementById('bp-explain-body');
  if (ex) {
    const pct = (v) => `${(v * 100).toFixed(0)}%`;
    ex.innerHTML = [
      `<p><b>What it remembers.</b> The moments you had a real choice — corners,
        forced turns, and the moves that got you killed — with the situation you
        were in. Routine corridor frames are mostly skipped: they describe the
        board, not you. ${b.memory.opponentRetained} situations held.</p>`,
      `<p><b>How it guesses.</b> A panel of simple assumptions about you all
        guess at once; whichever has been right most lately drives (currently
        ${b.nextForecast?.sourceName || '—'}) — except when you are forced to
        turn, where it bets directly on your measured turning habit. Guesses
        you could not actually make are corrected to your likeliest legal
        move; an assumption with nothing to say stays silent rather than
        being scored on a guess it never made.</p>`,
      `<p><b>Why you move.</b> Three of those assumptions are about your
        errand, not your habits: you're going for that food, you're coming
        for me, you're going for that weapon. Each comes in two travelling
        styles — you hold your line on the way, or you weave — and the
        weights work out which style is yours purely by watching which one
        keeps being right.</p>`,
      `<p><b>What the read rate means.</b> It is measured against <i>your own
        habits</i>, not against random: a rival that always guesses your
        commonest move would score ${rr.ready ? pct(rr.baseRate) : '—'}, and
        only the moves where they disagree count as evidence. Beating that
        rival is a read; matching it proves nothing.</p>`,
      `<p><b>What the seal proves.</b> Before you press a key it publishes a
        hash of its guess, and reveals it after — so the guess provably came
        first. It runs on your machine, so this is tamper-evidence, not proof
        against a hostile host.</p>`,
      `<p><b>What it tracks in plain sight.</b> Mines are planted with a flash
        and a sound, then disguise themselves as food. It remembers every
        plant it saw — and so could you. Tracking them is part of the
        game.</p>`,
      `<p><b>What it never does.</b> It never reads your keypress before
        predicting, never sees the board's future, and never sees anyone else's
        games. This brain only knows you, and it lives in this browser.</p>`,
    ].join('');
  }

  // SEAL — the prediction was published before your input was read.
  const sealEl = document.getElementById('bp-seal');
  const nf = b.nextForecast;
  if (b.scored) {
    sealEl.textContent = `SEAL ✓ ${b.seal.frames} verified · chain ${b.seal.chain.slice(0, 10)}…`;
    sealEl.className = 'bp-seal ok';
  } else if (nf) {
    sealEl.textContent = 'SEALED — prediction committed';
    sealEl.className = 'bp-seal';
  }

  if (roundMemoryStart === null) roundMemoryStart = b.memory.opponentObserved;

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

function roundRecord(s) {
  const endedAt = Date.now();
  const decision = s.brain.lastDecision;
  return {
    schemaVersion: ROUND_SCHEMA_VERSION,
    id: `${deviceId}:${endedAt}:${crypto.randomUUID ? crypto.randomUUID() : Math.random()}`,
    deviceId,
    endedAt,
    winner: s.winner,
    cause: s.cause,
    frames: s.frame,
    foodEaten: [...s.foodEaten],
    accuracy: { ...s.brain.accuracy.round },
    decisionSourceKey: decision?.forecast?.sourceKey || null,
    decisionSourceName: decision?.forecast?.sourceName || null,
    decisionReason: decision?.reason || null,
    decisionHeading: decision?.heading ?? null,
    memoryDelta: Math.max(0, s.brain.memory.opponentObserved -
      (roundMemoryStart ?? s.brain.memory.opponentObserved)),
    driftDetected: !!(s.brain.book && s.brain.book.driftDetected),
    mapUnseen: s.brain.book?.mapUnseen ?? null,
    mapThin: s.brain.book?.mapThin ?? null,
    lifetime: s.brain.readRate ? {
      rate: s.brain.readRate.lifetime?.rate ?? null,
      baseRate: s.brain.readRate.lifetime?.baseRate ?? null,
      lift: s.brain.readRate.lifetime?.lift ?? null,
      significant: !!s.brain.readRate.lifetime?.significant,
      samples: s.brain.readRate.lifetime?.samples ?? 0,
    } : null,
    cumulative: s.brain.book ? {
      roundsObserved: s.brain.book.roundsObserved ?? 0,
      driftZ: s.brain.book.driftZ ?? 0,
      rhythmEvents: s.brain.book.rhythmEvents ?? 0,
      rhythmPLeft: s.brain.book.rhythmPLeft ?? null,
      boxerAversion: s.brain.book.boxerAversion ?? 0,
      tactics: s.brain.book.tactics ?? [],
      weapons: s.brain.book.weapons ?? [],
      cpuLosses: s.brain.book.cpuLosses ?? [],
      mapUnseen: s.brain.book.mapUnseen ?? null,
      mapThin: s.brain.book.mapThin ?? null,
    } : null,
    book: s.brain.book ? {
      sideAccuracy: Number.isFinite(s.brain.book.sideAccuracy) ? s.brain.book.sideAccuracy : null,
      sideEvents: s.brain.book.sideEvents ?? 0,
      coverage: s.brain.book.coverage ?? 0,
      earned: s.brain.book.earned ?? 0,
      earnedSource: s.brain.book.earnedSource ?? 'none',
    } : null,
    models: s.brain.models.map((model) => ({
      key: model.key,
      name: model.name,
      rawScore: model.rawScore,
      effectiveScore: model.effectiveScore,
      hits: model.hits,
      samples: model.samples,
    })),
    // The ghost log (ADR-016): seed + both worms' input streams — enough to
    // replay this round bit-for-bit offline. This is how YOUR games become
    // the evaluation data future CPU candidates are measured against.
    replay: (() => {
      try { return JSON.parse(game.replay_json()); } catch { return null; }
    })(),
  };
}

// EXPORT MY ROUNDS — downloads every saved round (with ghost logs) as JSON
// for the offline evaluator (cargo run --release --example ghost_eval).
document.getElementById('export-rounds-btn')?.addEventListener('click', async () => {
  const rounds = await roundsRead();
  const blob = new Blob(
    [JSON.stringify({ v: 1, deviceId, exportedAt: Date.now(), rounds }, null, 1)],
    { type: 'application/json' },
  );
  const a = document.createElement('a');
  a.href = URL.createObjectURL(blob);
  a.download = `worm-rounds-${new Date().toISOString().slice(0, 10)}.json`;
  a.click();
  URL.revokeObjectURL(a.href);
});

function appendCell(row, value, className = '') {
  const cell = document.createElement('td');
  cell.textContent = String(value);
  if (className) cell.className = className;
  row.appendChild(cell);
}

function renderHistory() {
  const tbody = document.getElementById('history-body');
  while (tbody.children.length) tbody.removeChild(tbody.lastChild);
  roundHistory.slice(0, 20).forEach((record, index) => {
    const row = document.createElement('tr');
    const winner = record.winner === 0 ? 'You' : record.winner === 1 ? 'CPU' : 'Draw';
    appendCell(row, roundHistory.length - index);
    appendCell(row, winner, record.winner === 0 ? 'you' : 'cpu');
    appendCell(row, record.cause || '—');
    appendCell(row, record.frames);
    appendCell(row, record.foodEaten[0]);
    appendCell(row, record.foodEaten[1]);
    appendCell(row, `${(record.accuracy.rate * 100).toFixed(0)}% · n=${record.accuracy.samples}`);
    appendCell(row, record.decisionSourceName || '—');
    appendCell(row, record.decisionReason || 'no decision');
    appendCell(row, `+${record.memoryDelta}`);
    tbody.appendChild(row);
  });

  const aggregateHits = roundHistory.reduce((sum, record) => sum + record.accuracy.hits, 0);
  const aggregateSamples = roundHistory.reduce((sum, record) => sum + record.accuracy.samples, 0);
  const cpuWins = roundHistory.filter((record) => record.winner === 1).length;
  // Aggregate BY KEY, never by index: the model roster has changed size
  // across builds (7 -> 10 -> 13), and saved history spans every era. An
  // index walk crashed the whole boot for returning visitors ('reading
  // name' on undefined past a legacy record's end) and silently
  // misattributed stats even when it didn't. A record simply doesn't vote
  // for models it never knew about.
  const modelTotals = MODEL_NAMES.map((key, index) => {
    let hits = 0, samples = 0;
    for (const record of roundHistory) {
      const model = (record.models || []).find((m) => m && m.key === key);
      if (model) { hits += model.hits || 0; samples += model.samples || 0; }
    }
    return { key, name: MODEL_INFO[index]?.name || key, hits, samples };
  });
  const strongest = modelTotals
    .filter((model) => model.samples > 0)
    .sort((a, b) => (b.hits / b.samples) - (a.hits / a.samples))[0];
  document.getElementById('history-summary').textContent = roundHistory.length
    ? `${roundHistory.length} saved rounds · CPU wins ${((cpuWins / roundHistory.length) * 100).toFixed(0)}% · ` +
      `prediction ${aggregateSamples ? ((aggregateHits / aggregateSamples) * 100).toFixed(0) : 0}%/${aggregateSamples} · ` +
      `strongest ${strongest ? `${strongest.name} ${((strongest.hits / strongest.samples) * 100).toFixed(0)}%/${strongest.samples}` : '—'}`
    : 'No saved rounds yet — your longitudinal evidence will appear here.';
}

function setBrainStatus(msg) {
  document.getElementById('brain-status').textContent = `brain: ${msg}`;
  document.getElementById('device-id').textContent = (deviceId || '········').slice(0, 8);
}
setInterval(() => {
  if (game && !game.is_over()) {
    brainWrite(game.brain_save());
    if (state) setBrainStatus(`saved • observed ${state.brain.memory.opponentObserved} • lifetime ${(state.brain.accuracy.lifetime.rate * 100).toFixed(0)}%/${state.brain.accuracy.lifetime.samples}`);
  }
}, 10000);
window.addEventListener('pagehide', () => { if (game) brainWrite(game.brain_save()); });
// pagehide is not reliably delivered on mobile Safari when the app is
// backgrounded or the tab is discarded; visibilitychange is. Belt and braces —
// losing a round's learning is exactly the failure this game cannot afford.
document.addEventListener('visibilitychange', () => {
  if (document.visibilityState === 'hidden' && game) brainWrite(game.brain_save());
});

// Safari changes visualViewport dimensions as browser chrome expands/collapses.
// Keep presentation fitted without reconstructing the active WasmGame.
if (window.history && 'scrollRestoration' in window.history) {
  window.history.scrollRestoration = 'manual';
}
window.addEventListener('resize', scheduleArenaRefit);
window.addEventListener('orientationchange', scheduleArenaFocus);
window.addEventListener('load', scheduleArenaFocus, { once: true });
if (window.visualViewport) {
  window.visualViewport.addEventListener('resize', scheduleArenaRefit);
  window.visualViewport.addEventListener('scroll', scheduleArenaRefit);
}

/* ---------------- render ---------------- */

// Fixed constellation of streak seeds so the field is stable frame-to-
// frame (only phase animates) — flicker reads as noise, flow reads as
// SPEED.
const SLIP_STREAKS = Array.from({ length: 64 }, (_, i) => ({
  angle: (i / 64) * Math.PI * 2 + (Math.sin(i * 12.9898) * 43758.5453) % 0.09,
  speed: 0.6 + ((i * 7919) % 100) / 100 * 1.4,
  jitter: ((i * 104729) % 100) / 100,
}));

function drawSlipstreamFx(s) {
  const [hx, hy] = s.cycles[0].head;
  const cx = hx * CELL + CELL / 2;
  const cy = hy * CELL + CELL / 2;
  const W = s.w * CELL, H = s.h * CELL;
  const t = performance.now() / 1000;
  const maxR = Math.hypot(W, H);

  offCtx.save();
  offCtx.globalCompositeOperation = 'lighter';

  // Hyperspace streaks: each runs OUTWARD from the worm; phase cycles so
  // they continuously tear past. Length grows with radius (near-field
  // short, far-field long) — the classic light-speed starfield.
  for (const sk of SLIP_STREAKS) {
    const phase = (t * sk.speed + sk.jitter) % 1;
    const r0 = 18 + phase * maxR * 0.75;
    const len = 6 + phase * phase * 90;
    const x0 = cx + Math.cos(sk.angle) * r0;
    const y0 = cy + Math.sin(sk.angle) * r0;
    const x1 = cx + Math.cos(sk.angle) * (r0 + len);
    const y1 = cy + Math.sin(sk.angle) * (r0 + len);
    const a = (1 - phase) * 0.5;
    const grad = offCtx.createLinearGradient(x0, y0, x1, y1);
    grad.addColorStop(0, `rgba(160, 255, 235, 0)`);
    grad.addColorStop(0.5, `rgba(120, 255, 220, ${a})`);
    grad.addColorStop(1, `rgba(255, 255, 255, ${a * 0.9})`);
    offCtx.strokeStyle = grad;
    offCtx.lineWidth = 1 + phase * 1.6;
    offCtx.beginPath();
    offCtx.moveTo(x0, y0);
    offCtx.lineTo(x1, y1);
    offCtx.stroke();
  }

  // Chromatic warp rings: three expanding circles, RGB-split by a few
  // pixels — the lensing shimmer.
  for (let ring = 0; ring < 3; ring++) {
    const phase = ((t * 0.9) + ring / 3) % 1;
    const r = 12 + phase * 130;
    const a = (1 - phase) * 0.35;
    const chans = [
      [255, 80, 120, -2],
      [120, 255, 190, 0],
      [110, 160, 255, 2],
    ];
    for (const [cr, cg, cb, off] of chans) {
      offCtx.strokeStyle = `rgba(${cr}, ${cg}, ${cb}, ${a})`;
      offCtx.lineWidth = 1.5;
      offCtx.beginPath();
      offCtx.arc(cx + off, cy, Math.max(1, r + off), 0, Math.PI * 2);
      offCtx.stroke();
    }
  }

  offCtx.globalCompositeOperation = 'source-over';
  // Focus vignette: the world beyond your bubble dims — you are the
  // still point the light bends around.
  const vg = offCtx.createRadialGradient(cx, cy, 40, cx, cy, maxR * 0.7);
  vg.addColorStop(0, 'rgba(0, 12, 10, 0)');
  vg.addColorStop(1, 'rgba(0, 12, 10, 0.45)');
  offCtx.fillStyle = vg;
  offCtx.fillRect(0, 0, W, H);
  offCtx.restore();
}

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
      // BOMB: dark sphere + spark. Explicit `kind === 2`, not a bare `else`:
      // a bare else silently draws a bomb for any future power-up kind.
      offCtx.fillStyle = '#333';
      offCtx.beginPath(); offCtx.arc(cx, cy + 1, R * 0.75, 0, Math.PI * 2); offCtx.fill();
      offCtx.strokeStyle = '#888'; offCtx.beginPath();
      offCtx.moveTo(cx + R * 0.35, cy - R * 0.45); offCtx.quadraticCurveTo(cx + R * 0.6, cy - R, cx + R * 0.9, cy - R * 0.9);
      offCtx.stroke();
      offCtx.fillStyle = `hsl(${(performance.now() / 8) % 60}, 100%, 60%)`;
      offCtx.beginPath(); offCtx.arc(cx + R * 0.9, cy - R * 0.9, 2, 0, Math.PI * 2); offCtx.fill();
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

  // The path is owned by this frame's decision, never by the separately
  // computed next-frame forecast.
  const decisionPath = s.brain.decision?.projection?.path || [];
  for (let i = 0; i < decisionPath.length; i++) {
    const [x, y] = decisionPath[i];
    const alpha = Math.max(0.1, 0.34 - i * 0.045);
    offCtx.fillStyle = `rgba(0, 255, 255, ${alpha})`;
    offCtx.fillRect(x * CELL + CELL * 0.28, y * CELL + CELL * 0.28, CELL * 0.44, CELL * 0.44);
  }

  // NAPALM (world v9): flame patches — three flickering layers whose
  // alpha tracks remaining life; drawn under worms so a body crossing
  // fire reads as IN it.
  if (s.flames && s.flames.length) {
    const t = performance.now();
    for (const [fx, fy, lifePct] of s.flames) {
      const base = 0.25 + 0.65 * (lifePct / 100);
      const flick = 0.75 + 0.25 * Math.sin(t / 47 + fx * 3.1 + fy * 1.7);
      offCtx.fillStyle = `rgba(255, 90, 20, ${(base * flick).toFixed(3)})`;
      offCtx.fillRect(fx * CELL, fy * CELL, CELL, CELL);
      offCtx.fillStyle = `rgba(255, 170, 30, ${(base * flick * 0.7).toFixed(3)})`;
      offCtx.fillRect(fx * CELL + 2, fy * CELL + 2, CELL - 4, CELL - 4);
      offCtx.fillStyle = `rgba(255, 240, 120, ${(base * flick * 0.5).toFixed(3)})`;
      offCtx.fillRect(fx * CELL + CELL / 2 - 2, fy * CELL + CELL / 2 - 2, 4, 4);
    }
  }


  // LASER BEAMS (ADR-023 contract): the sim's own cells, full-cell
  // quads, drawn UNDER the worms. Age 0 = solid lethal core; 1-5 =
  // rapidly dimming afterimage; 6-20 = sparse embers, visibly residue.
  // Solid == hot, faded == inert — the visual says when lethality ended.
  if (s.beams) {
    for (const [cells, age] of s.beams) {
      if (age === 0) {
        offCtx.fillStyle = 'rgba(255, 255, 160, 0.92)';
        for (const [bx, by] of cells) {
          offCtx.fillRect(bx * CELL, by * CELL, CELL, CELL);
        }
      } else if (age <= 5) {
        const a5 = 0.55 * (1 - age / 6);
        offCtx.fillStyle = `rgba(255, 240, 120, ${a5.toFixed(3)})`;
        const inset = 1 + age;
        for (const [bx, by] of cells) {
          offCtx.fillRect(
            bx * CELL + inset, by * CELL + inset,
            Math.max(CELL - 2 * inset, 2), Math.max(CELL - 2 * inset, 2)
          );
        }
      } else {
        offCtx.fillStyle = 'rgba(200, 170, 80, 0.25)';
        for (let i = 0; i < cells.length; i += 3) {
          const [bx, by] = cells[i];
          offCtx.fillRect(bx * CELL + CELL / 2 - 1, by * CELL + CELL / 2 - 1, 2, 2);
        }
      }
    }
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
    // BURNING (world v9): while the sticky schedule runs, the body
    // sheds embers — the "it's getting burned/shrunk" effect.
    if (s.burning && s.burning[ci] && c.alive) {
      const t2 = performance.now();
      for (let i = Math.max(1, len - 6); i < len; i++) {
        const [bx2, by2] = c.pos[i];
        const j = Math.sin(t2 / 31 + i * 2.7);
        offCtx.fillStyle = `rgba(255, ${120 + 80 * Math.abs(j) | 0}, 30, 0.85)`;
        offCtx.fillRect(
          bx2 * CELL + CELL / 2 - 2 + j * 3,
          by2 * CELL + CELL / 2 - 2 - Math.abs(j) * 4,
          4, 4
        );
      }
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

  // DECOY FLASH (world v8): the last two seconds of a planted bomb
  // strobe — tier 1 at ~6Hz in warning orange, tier 2 at ~14Hz in hard
  // white. The channel only carries flashing bombs, so nothing here can
  // reveal a calm decoy.
  if (s.bombFlash) {
    const now = performance.now();
    for (const [bx, by, tier] of s.bombFlash) {
      const hz = tier >= 2 ? 14 : 6;
      const on = Math.floor(now / (1000 / hz / 2)) % 2 === 0;
      if (!on) continue;
      offCtx.fillStyle = tier >= 2 ? 'rgba(255,255,255,0.95)' : 'rgba(255,150,40,0.85)';
      offCtx.fillRect(bx * CELL, by * CELL, CELL, CELL);
    }
  }

  // particles (additive)
  offCtx.globalCompositeOperation = 'lighter';
  for (const [px, py, pr, pg, pb, life] of s.particles) {
    const alpha = Math.min(life / 40, 1);
    offCtx.fillStyle = `rgba(${pr}, ${pg}, ${pb}, ${alpha})`;
    offCtx.fillRect(px * CELL + CELL / 2 - 1.5, py * CELL + CELL / 2 - 1.5, 3, 3);
  }
  offCtx.globalCompositeOperation = 'source-over';

  // SLIPSTREAM FX — light-speed distortion anchored on YOUR worm:
  // hyperspace streaks tearing outward, chromatic warp rings, and a
  // focus vignette. Additive, time-animated, all on the game canvas.
  if (s.slipstream && s.cycles && s.cycles[0] && s.cycles[0].alive) {
    drawSlipstreamFx(s);
  }

  // present: crisp pass + phosphor bloom
  ctx.clearRect(0, 0, canvas.width, canvas.height);
  ctx.drawImage(off, 0, 0);
  ctx.globalAlpha = 0.30;
  ctx.filter = 'blur(3px)';
  ctx.drawImage(off, 0, 0);
  ctx.filter = 'none';
  ctx.globalAlpha = 1;
}

boot().catch((e) => showFatal(e?.message || String(e)));
