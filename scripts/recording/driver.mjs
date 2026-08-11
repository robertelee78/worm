// CLAUDE PLAYS WORM, TAKE 4 — the legible cut. Two first-to-3 matches.
//
// Owner verdicts on takes 2-3: "played like crap" → "you both need DUI
// write-ups … spinning for 74 minutes == boring … nothing for the CPU
// to learn". Root causes, owned: per-frame random noise (visible
// wobble), a reckless collision tier (staged drunk driving), no turn
// commitment (zigzag), and entropy phases that made me statistically
// unreadable — which lobotomized the CPU's hunts (its compelling play
// only activates on an earned read) and left it circling + mining.
//
// TAKE 4 SPINE (owner spec): "eat food, get powerups, use them, etc
// etc trap each other."
//  - GOAL COMMITMENT: pick a target (powerup > hunt > safe food >
//    roam), BFS a path, FOLLOW it until done or invalid. No per-frame
//    re-scoring, no noise, no reckless tier. Lines look intentional.
//  - READABLE BY DESIGN: BFS neighbor order prefers left-of-heading at
//    ties — a real habit the brain can read, expressed structurally.
//  - WEAPONS WITH INTENT: laser only on a true beam crossing; tri-shot
//    on ray/brush solutions (v12 supercover); bombs planted as bait on
//    food cells or dropped on a chaser.
//  - TRAPS: a materially choking move overrides the path (boxing), and
//    bait mines are laid where the CPU forages.
//  - MINE DEFENSE, the owner's own rule: food older than one fuse
//    (~13s) is provably real; fresh arrivals are suspect.
import { chromium } from 'playwright';
import fs from 'fs';

const ROUNDS = 40; // owner: "we should play like 15 though — 3-0, 3-1 not enough"
const OUT = process.cwd();

const b = await chromium.launch();
const ctx = await b.newContext({
  viewport: { width: 1280, height: 900 },
  recordVideo: { dir: `${OUT}/video`, size: { width: 1280, height: 900 } },
});
const p = await ctx.newPage();
p.on('pageerror', e => console.log('[pageerror]', e.message.slice(0, 160)));
await p.goto('http://localhost:8082/', { waitUntil: 'load' });
await p.evaluate(() => { window.__sfx = []; });
await p.mouse.click(640, 450);
await p.waitForFunction(() => window.game && !JSON.parse(window.game.state_json()).over, { timeout: 15000 });
const videoT0 = Date.now();
await p.evaluate(() => { window.__t0 = performance.now(); });

await p.evaluate(() => {
  const DELTA = [[0, -1], [0, 1], [-1, 0], [1, 0]]; // up down left right
  const OPP = [1, 0, 3, 2];
  const LEFTOF = [2, 3, 1, 0];
  const RIGHTOF = [3, 2, 0, 1];
  const WALL = 1;

  let lastFrame = -1, lastSentDir = -1, lastSeenDir = -1;
  let goal = null;            // {type, target:[x,y], path:[[x,y],...]}
  let lastFireT = 0;
  let wasOver = true;
  let foodSeen = new Map();    // "x,y" -> ms first seen
  let roundT0 = 0;
  // HUMAN-STYLE ADAPTATION (owner: "not deliberately static … more
  // human like"): structural responses to being punished, never noise.
  // Get intercepted -> flip which side I favor (the drift alarm must
  // re-learn me). Eat a mine -> distrust young food for longer.
  let habitSide = 'left';
  let foodAgeMin = 14000;
  let deathSeen = false;

  const K = (x, y) => x + ',' + y;

  window.__onFrame = (s) => {
    const g = window.game;
    if (!g) return;
    if (s.over) {
      if (!deathSeen) {
        deathSeen = true;
        // VIDEO-CLARITY MODE (owner: the learning arc must be VISIBLE):
        // the bot stays STATIC — consistent readable habits all session —
        // so the CPU's conversion curve shows cleanly instead of being
        // reset by counter-adaptation every time it earns a kill.
      }
      wasOver = true; return;
    }
    if (s.frame === lastFrame) return;
    lastFrame = s.frame;
    if (wasOver) {
      wasOver = false; deathSeen = false; goal = null; foodSeen = new Map();
      roundT0 = performance.now(); lastSentDir = -1; lastSeenDir = -1;
    }
    const me = s.cycles[0], cpu = s.cycles[1];
    if (!me || !me.alive) return;
    const W = s.w, H = s.h;
    const grid = g.grid();
    const [hx, hy] = me.head;
    const myDir = me.dir;
    if (myDir !== lastSeenDir) { lastSeenDir = myDir; lastSentDir = -1; }
    const now = performance.now();

    // ---- board truth + hazards ----
    const solid = (x, y) => {
      if (x < 0 || y < 0 || x >= W || y >= H) return true;
      const c = grid[y * W + x];
      return c === WALL || c === 2 || c === 3;
    };
    const hot = new Set();
    for (const [fx, fy] of (s.flames || [])) hot.add(K(fx, fy));
    for (const [bx, by] of (s.bombFlash || [])) {
      for (let dx = -2; dx <= 2; dx++) for (let dy = -2; dy <= 2; dy++) hot.add(K(bx + dx, by + dy));
      for (let t = 3; t <= 10; t++) { hot.add(K(bx + t, by)); hot.add(K(bx - t, by)); hot.add(K(bx, by + t)); hot.add(K(bx, by - t)); }
    }
    for (const [bx, by, bdx, bdy] of (s.bolts || []))
      for (let t = 0; t <= 6; t++) {
        hot.add(K(bx + bdx * t, by + bdy * t));
        if (bdx !== 0 && bdy !== 0) { hot.add(K(bx + bdx * (t + 1), by + bdy * t)); hot.add(K(bx + bdx * t, by + bdy * (t + 1))); }
      }
    for (const [cells, age] of (s.beams || [])) if (age === 0) for (const [bx, by] of cells) hot.add(K(bx, by));
    const free = (x, y) => !solid(x, y) && !hot.has(K(x, y));

    const [cx, cy] = cpu && cpu.alive ? cpu.head : [W >> 1, H >> 1];
    const cd = cpu ? cpu.dir : 0;
    const cpuDist = Math.abs(cx - hx) + Math.abs(cy - hy);
    const cpuNextCells = cpu && cpu.alive
      ? [cd, LEFTOF[cd], RIGHTOF[cd]].map(d => K(cx + DELTA[d][0], cy + DELTA[d][1]))
      : [];

    // ---- flood fill (survival + choke metric) ----
    const space = (sx, sy, extraBlocked) => {
      if (!free(sx, sy) || (extraBlocked && extraBlocked.has(K(sx, sy)))) return 0;
      const seen = new Set([K(sx, sy)]);
      const q = [[sx, sy]];
      let n = 0, qi = 0;
      while (qi < q.length && n < 500) {
        const [x, y] = q[qi++]; n++;
        for (const [dx, dy] of DELTA) {
          const nx = x + dx, ny = y + dy, k = K(nx, ny);
          if (!seen.has(k) && free(nx, ny) && !(extraBlocked && extraBlocked.has(k))) { seen.add(k); q.push([nx, ny]); }
        }
      }
      return n;
    };

    // ---- food ledger: age makes food provably real (13s fuse) ----
    const safeFood = [];
    for (const [fx, fy] of (s.food || [])) {
      const k = K(fx, fy);
      if (!foodSeen.has(k)) foodSeen.set(k, now);
      const age = now - foodSeen.get(k);
      const fromStart = foodSeen.get(k) - roundT0 < 1200; // on the board at spawn
      if (fromStart || age > foodAgeMin) safeFood.push([fx, fy]);
    }

    // ---- BFS with the HABIT in the neighbor order (left at ties) ----
    const bfsPath = (tx, ty) => {
      if (!free(tx, ty) && !(s.food || []).some(([fx, fy]) => fx === tx && fy === ty)
          && !(s.powerups || []).some(([px, py]) => px === tx && py === ty)) return null;
      const prev = new Map();
      const q = [[hx, hy, myDir]];
      prev.set(K(hx, hy), null);
      let qi = 0, found = false;
      while (qi < q.length && qi < 2600) {
        const [x, y, d] = q[qi++];
        if (x === tx && y === ty) { found = true; break; }
        const order = habitSide === 'left' ? [LEFTOF[d], d, RIGHTOF[d]] : [RIGHTOF[d], d, LEFTOF[d]];
        for (const nd of order) { // the readable habit, adaptively sided
          const nx = x + DELTA[nd][0], ny = y + DELTA[nd][1], k = K(nx, ny);
          if (prev.has(k)) continue;
          const isTarget = nx === tx && ny === ty;
          if (!isTarget && !free(nx, ny)) continue;
          if (isTarget && solid(nx, ny) && grid[ny * W + nx] !== 0) { /* target may be food/powerup cell */ }
          prev.set(k, [x, y]);
          q.push([nx, ny, nd]);
        }
      }
      if (!found) return null;
      const path = [];
      let cur = [tx, ty];
      while (cur && !(cur[0] === hx && cur[1] === hy)) { path.push(cur); cur = prev.get(K(cur[0], cur[1])); }
      path.reverse();
      return path.length ? path : null;
    };

    // ---- goal selection (only when missing/invalid) ----
    const goalValid = () => {
      if (!goal || !goal.path || !goal.path.length) return false;
      const [tx, ty] = goal.target;
      if (goal.type === 'powerup' && !(s.powerups || []).some(([px, py]) => px === tx && py === ty)) return false;
      if (goal.type === 'food' && !(s.food || []).some(([fx, fy]) => fx === tx && fy === ty)) return false;
      if (goal.type === 'hunt' && (!cpu || !cpu.alive)) return false;
      if (goal.type === 'hunt' && goal.age++ > 40) return false; // re-aim on a moving target
      return true;
    };
    if (!goalValid()) {
      goal = null;
      // 1) arm up
      let best = null, bestD = 1e9;
      for (const [px, py] of (s.powerups || [])) {
        const d = Math.abs(px - hx) + Math.abs(py - hy);
        if (d < bestD) { bestD = d; best = [px, py]; }
      }
      if (best) {
        const path = bfsPath(best[0], best[1]);
        if (path) goal = { type: 'powerup', target: best, path, age: 0 };
      }
      // 2) armed: create the shot
      if (!goal && me.held != null && cpu && cpu.alive && cpuDist < 30) {
        let aim = null;
        if (me.held === 0) {
          // laser: reach the CPU's row or column at standoff range
          aim = Math.abs(hx - cx) < Math.abs(hy - cy) ? [cx, hy] : [hx, cy];
          if (Math.abs(aim[0] - cx) + Math.abs(aim[1] - cy) < 4) aim = null; // too close
        } else if (me.held === 1) {
          // tri-shot: close to a forward cone position ~6 ahead of its nose
          aim = [cx + DELTA[cd][0] * 4, cy + DELTA[cd][1] * 4];
        } // bomb: no travel goal — it plants en route (below)
        if (aim && aim[0] > 0 && aim[1] > 0 && aim[0] < W - 1 && aim[1] < H - 1) {
          const path = bfsPath(aim[0], aim[1]);
          if (path && path.length > 2) goal = { type: 'hunt', target: aim, path, age: 0 };
        }
      }
      // 3) eat (provably-real food only)
      if (!goal) {
        let bf = null, bd = 1e9;
        for (const [fx, fy] of safeFood) {
          const d = Math.abs(fx - hx) + Math.abs(fy - hy);
          if (d < bd) { bd = d; bf = [fx, fy]; }
        }
        if (bf) {
          const path = bfsPath(bf[0], bf[1]);
          if (path) goal = { type: 'food', target: bf, path, age: 0 };
        }
      }
      // 4) roam toward open space near the middle
      if (!goal) {
        const tx = Math.max(5, Math.min(W - 6, hx + (hx < W / 2 ? 8 : -8)));
        const ty = Math.max(5, Math.min(H - 6, hy + (hy < H / 2 ? 6 : -6)));
        const path = bfsPath(tx, ty);
        if (path) goal = { type: 'roam', target: [tx, ty], path, age: 0 };
      }
    }

    // ---- next step: follow the path, with a boxing override + safety veto ----
    let nextDir = null;
    // Boxing override: if a legal move materially chokes the CPU, take it.
    if (cpu && cpu.alive && me.pos.length >= cpu.pos.length && cpuDist <= 12) {
      const cpuRoom = space(cx + DELTA[cd][0], cy + DELTA[cd][1]);
      if (cpuRoom > 0 && cpuRoom < 250) {
        let choke = null;
        for (const d of [myDir, LEFTOF[myDir], RIGHTOF[myDir]]) {
          const nx = hx + DELTA[d][0], ny = hy + DELTA[d][1];
          if (!free(nx, ny) || space(nx, ny) < me.pos.length + 6) continue;
          const blocked = new Set([K(nx, ny)]);
          const after = space(cx + DELTA[cd][0], cy + DELTA[cd][1], blocked);
          if (after < cpuRoom - 10 && (!choke || after < choke.after)) choke = { d, after };
        }
        if (choke) { nextDir = choke.d; goal = null; } // trap play trumps the errand
      }
    }
    if (nextDir == null && goal && goal.path.length) {
      const [nx, ny] = goal.path[0];
      const d = DELTA.findIndex(([dx, dy]) => hx + dx === nx && hy + dy === ny);
      if (d >= 0 && d !== OPP[myDir] && free(nx, ny) && !cpuNextCells.includes(K(nx, ny))) {
        nextDir = d;
        goal.path.shift();
      } else {
        goal = null; // stale path — re-plan next frame
      }
    }
    if (nextDir == null) {
      // No plan this frame: safest committed step (prefer straight — smooth).
      let bestD2 = null, bestRoom = -1;
      for (const d of [myDir, LEFTOF[myDir], RIGHTOF[myDir]]) {
        const nx = hx + DELTA[d][0], ny = hy + DELTA[d][1];
        if (!free(nx, ny)) continue;
        const room = space(nx, ny) + (d === myDir ? 2 : 0);
        if (room > bestRoom) { bestRoom = room; bestD2 = d; }
      }
      nextDir = bestD2 != null ? bestD2 : myDir;
    }
    // Safety veto: never follow a path step into a trap the board grew.
    {
      const nx = hx + DELTA[nextDir][0], ny = hy + DELTA[nextDir][1];
      if (free(nx, ny) && space(nx, ny) < Math.min(60, me.pos.length)) {
        let alt = null, altRoom = -1;
        for (const d of [myDir, LEFTOF[myDir], RIGHTOF[myDir]]) {
          const ax = hx + DELTA[d][0], ay = hy + DELTA[d][1];
          if (!free(ax, ay)) continue;
          const room = space(ax, ay);
          if (room > altRoom) { altRoom = room; alt = d; }
        }
        if (alt != null && altRoom > space(nx, ny)) { nextDir = alt; goal = null; }
      }
    }
    if (nextDir !== myDir && nextDir !== lastSentDir && nextDir !== OPP[myDir]) {
      g.set_direction(nextDir);
      lastSentDir = nextDir;
    }

    // ---- weapons, deliberately ----
    if (me.held != null && cpu && cpu.alive && now - lastFireT > 2000) {
      const aimDir = nextDir != null ? nextDir : myDir;
      const [adx, ady] = DELTA[aimDir];
      const cpuCells = new Set(cpu.pos.map(([x, y]) => K(x, y)));
      cpuCells.add(K(cx + DELTA[cd][0], cy + DELTA[cd][1]));
      let fire = false;
      if (me.held === 0) {
        let x = hx, y = hy;
        for (let t = 0; t < 60; t++) {
          x += adx; y += ady;
          if (x < 0 || y < 0 || x >= W || y >= H || grid[y * W + x] === WALL) break;
          if (cpuCells.has(K(x, y))) { fire = true; break; }
        }
      } else if (me.held === 1 && cpuDist <= 18) {
        for (const [ddx, ddy] of [[adx, ady], [adx + ady, ady + adx], [adx - ady, ady - adx]]) {
          let x = hx, y = hy;
          for (let t = 0; t < 22; t++) {
            x += ddx; y += ddy;
            if (x < 0 || y < 0 || x >= W || y >= H || grid[y * W + x] === WALL) break;
            if (cpuCells.has(K(x, y))) { fire = true; break; }
            // v12 supercover: corner brushes are real hits too
            if (ddx !== 0 && ddy !== 0 && (cpuCells.has(K(x, y - ddy)) || cpuCells.has(K(x - ddx, y)))) { fire = true; break; }
          }
          if (fire) break;
        }
      } else if (me.held === 2) {
        const behind = (cx - hx) * adx + (cy - hy) * ady < 0;
        const onFood = (s.food || []).some(([fx, fy]) => Math.abs(fx - hx) + Math.abs(fy - hy) <= 1);
        if ((behind && cpuDist <= 8) || (onFood && cpuDist >= 10 && cpuDist <= 28)) fire = true;
      }
      if (fire) { g.fire(); lastFireT = now; }
    }
  };
});

// ---- match loop: two first-to-3 matches ----
const results = [];
let matchesDone = 0, roundNo = 0;
const sessionT0 = Date.now();
while (results.length < ROUNDS && Date.now() - sessionT0 < 35 * 60 * 1000) {
  await p.evaluate(() => {
    if (!window.game || !window.game.is_over()) return;
    const champ = document.getElementById('champion-overlay');
    if (champ && !champ.classList.contains('hidden')) {
      document.getElementById('new-match-btn').click();
    } else {
      document.getElementById('next-round-btn').click();
    }
  });
  await p.evaluate((r) => {
    window.__vidRound = r;
    // ADR-028: count coil closures via the wire receipt so the roll
    // gate can demand a wrap on camera.
    if (!window.__coilWatch) {
      window.__coilWatch = true;
      const prev = window.__onFrame;
      window.__onFrame = (s) => {
        if (prev) prev(s);
        const ex = s && s.brain && s.brain.learnedExploit;
        if (ex && /ring closed/.test(ex.counter) && ex.frame !== window.__coilLastFrame) {
          window.__coilLastFrame = ex.frame;
          window.__coilSeen = (window.__coilSeen || 0) + 1;
        }
      };
    }
  }, roundNo + 1);
  const started = await p.waitForFunction(
    () => window.game && !JSON.parse(window.game.state_json()).over,
    { timeout: 10000 }
  ).catch(() => null);
  if (!started) { console.log('TRANSITION-STUCK'); continue; }
  roundNo++;

  const t0 = Date.now();
  let st = null, done = false;
  while (Date.now() - t0 < 300000) {
    await p.waitForTimeout(400);
    st = await p.evaluate(() => {
      const s = JSON.parse(window.game.state_json());
      const champ = document.getElementById('champion-overlay');
      return { over: s.over, winner: s.winner, wins: s.wins, frames: s.frame, cause: s.cause,
               champion: champ && !champ.classList.contains('hidden') };
    });
    if (st.over) { done = true; break; }
  }
  const err = await p.evaluate(() => window.__brainErr || null);
  if (err) console.log('BRAIN-ERR', err);
  if (!done) { console.log('round', roundNo, 'STILL-RUNNING at frames', st && st.frames); continue; }
  if (st.champion) matchesDone++;
  st.coils = await p.evaluate(() => window.__coilSeen || 0);
  results.push({ round: roundNo, ...st });
  // Cumulative session score for the banner (per-match wins reset).
  const totals = results.reduce(
    (a, r) => { if (r.winner === 0) a[0]++; else if (r.winner === 1) a[1]++; return a; },
    [0, 0]
  );
  await p.evaluate((t) => { window.__vidScore = t; }, totals);
  console.log('round', roundNo, JSON.stringify(results[results.length - 1]));
}

await p.evaluate(() => { window.__onFrame = null; });
const sfx = await p.evaluate(() => window.__sfx || []);
fs.writeFileSync(`${OUT}/sfx-log.json`, JSON.stringify({ videoT0, sfx }, null, 0));
fs.writeFileSync(`${OUT}/results.json`, JSON.stringify(results, null, 1));
const video = p.video();
await ctx.close();
const path = await video.path();
fs.writeFileSync(`${OUT}/video-path.txt`, path);
await b.close();
console.log('DONE video at', path, 'rounds:', results.length, 'matches:', matchesDone);
