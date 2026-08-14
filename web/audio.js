// audio.js — chiptune sfx engine for worm (CRT arcade cabinet sound).
//
// WebAudio, no deps. Voices: square + triangle oscillators and one reused
// white-noise buffer for percussive/explosion events, all under a master
// gain. Everything is a safe no-op until unlock() runs from a user gesture
// (autoplay policy) and the AudioContext is running.
//
// API (for app.js — integrator contract, do not rename):
//   Backward compat — new Sfx() / unlock() / play(freq, durMs, delayMs)
//     (current app.js wasm-event path; square note with a decay envelope).
//   Named jingles — food(v), powerup(), laser(), trishot(), bombPlant(),
//     detonate(), wallPunch(), deathRiff(), roundStart(), champion(playerWon),
//     insertCoin(), engineHum(on, speedPct).
//   BGM — sfx.bgm.start(speedPct) / setSpeed(speedPct) / stop() /
//     toggleMute() → muted boolean. speedPct is 0–100. Procedural 8-bit loop
//     (A minor, 4 bars @ ~126 BPM, bass+arp+hats) that nudges tempo and
//     filter brightness ±15% with game speed. FILE SLOT: on first start() it
//     HEAD-fetches 'music.mp3' — drop any royalty-free track at web/music.mp3
//     and it wins (looped via MediaElementSource); no file → procedural.
//     start() pre-unlock is remembered and kicks in on the next unlock().
//
// Nothing leaks: every one-shot node is stop()ed with an onended disconnect;
// the noise buffer is allocated once and looped; engineHum reuses a single
// persistent oscillator. Imports cleanly in node (no top-level WebAudio).

// note(n) → Hz, n semitones from A4 (0 → 440, 12 → 880, -12 → 220).
export const note = (n) => 440 * Math.pow(2, n / 12);

export class Sfx {
  constructor() {
    this.ctx = null;      // AudioContext, created lazily in unlock()
    this.master = null;   // master GainNode → destination
    this.noiseBuf = null; // shared 1s white-noise buffer, created once
    this.hum = null;      // { osc, gain } persistent engine hum
    this.volume = 0.3;    // cabinet default (was 0.06: measured -29 dB
                          // peak in a real browser — 'no music, no
                          // sounds?' — owner, 2026-08-11)
    this.bgm = new Bgm(this); // background music engine (see header)
  }

  // Call from a user gesture (keydown / pointerdown). Creates the context on
  // first use; resumes it whenever the browser auto-suspended it.
  unlock() {
    if (!this.ctx) {
      const AC = globalThis.AudioContext || globalThis.webkitAudioContext;
      if (!AC) return; // headless / ancient browser: stay silent, never throw
      this.ctx = new AC();
      this.master = this.ctx.createGain();
      this.master.gain.value = this.volume;
      this.master.connect(this.ctx.destination);
      const len = this.ctx.sampleRate;
      this.noiseBuf = this.ctx.createBuffer(1, len, this.ctx.sampleRate);
      const d = this.noiseBuf.getChannelData(0);
      for (let i = 0; i < len; i++) d[i] = Math.random() * 2 - 1;
    }
    const kick = () => this.bgm._kick();
    if (this.ctx.state === 'suspended') this.ctx.resume().then(kick, kick);
    else kick();
  }

  get ready() {
    return !!this.ctx && this.ctx.state === 'running';
  }

  // ── voices ─────────────────────────────────────────────────────────────

  // One enveloped oscillator note. `slide` exponentially bends pitch to a
  // target Hz over the note (zaps, sweeps, thump decay). `out` overrides the
  // routing target (BGM voices go through the BGM chain, not master).
  _tone({ type = 'square', freq, at = 0, dur = 0.08, vol = 0.5, slide = null, attack = 0.004, out = this.master }) {
    const t = this.ctx.currentTime + at;
    const osc = this.ctx.createOscillator();
    const g = this.ctx.createGain();
    osc.type = type;
    osc.frequency.setValueAtTime(freq, t);
    if (slide) osc.frequency.exponentialRampToValueAtTime(Math.max(20, slide), t + dur);
    g.gain.setValueAtTime(0, t);
    g.gain.linearRampToValueAtTime(vol, t + attack);
    g.gain.exponentialRampToValueAtTime(0.001, t + dur);
    osc.connect(g); g.connect(out);
    osc.start(t);
    osc.stop(t + dur + 0.02);
    osc.onended = () => { osc.disconnect(); g.disconnect(); };
  }

  // Enveloped filtered noise hit. `slide` sweeps the filter cutoff down over
  // the hit (explosion decaying to rumble). `out` as in _tone.
  _noise({ at = 0, dur = 0.3, vol = 0.6, freq = 2000, slide = null, type = 'lowpass', q = 0.8, out = this.master }) {
    const t = this.ctx.currentTime + at;
    const src = this.ctx.createBufferSource();
    src.buffer = this.noiseBuf;
    src.loop = true;
    const f = this.ctx.createBiquadFilter();
    f.type = type;
    f.frequency.setValueAtTime(freq, t);
    f.Q.value = q;
    if (slide) f.frequency.exponentialRampToValueAtTime(Math.max(30, slide), t + dur);
    const g = this.ctx.createGain();
    g.gain.setValueAtTime(0, t);
    g.gain.linearRampToValueAtTime(vol, t + 0.005);
    g.gain.exponentialRampToValueAtTime(0.001, t + dur);
    src.connect(f); f.connect(g); g.connect(out);
    src.start(t);
    src.stop(t + dur + 0.02);
    src.onended = () => { src.disconnect(); f.disconnect(); g.disconnect(); };
  }

  // ── backward-compat raw voice (wasm [freq, durMs, delayMs] events) ─────
  play(freq, durMs = 80, delayMs = 0) {
    if (!this.ready) return;
    this._tone({ freq, at: delayMs / 1000, dur: durMs / 1000, vol: 0.5 });
  }

  // ── named jingles ──────────────────────────────────────────────────────

  // Food pickup: two-note ding, up a fifth; base pitch climbs one semitone
  // per food value point (1-9) so bigger prizes literally ring higher.
  food(value = 1) {
    if (!this.ready) return;
    const v = Math.min(9, Math.max(1, value | 0));
    const base = note(7 + v); // ~F5 … ~E6
    this._tone({ freq: base, dur: 0.07, vol: 0.5 });
    this._tone({ freq: base * 1.5, at: 0.07, dur: 0.12, vol: 0.5 });
  }

  // Powerup: the 4-note major arp (C5 E5 G5 C6), brightened — each note gets
  // a quiet octave-up double and a short high "shing" of noise on top.
  powerup() {
    if (!this.ready) return;
    [12, 16, 19, 24].forEach((n, i) => {
      this._tone({ freq: note(n), at: i * 0.05, dur: 0.09, vol: 0.45 });
      this._tone({ freq: note(n + 12), at: i * 0.05, dur: 0.07, vol: 0.13 });
    });
    this._noise({ dur: 0.06, vol: 0.12, freq: 7000, type: 'highpass' });
  }

  // Laser: layered death beam (~300ms). Square pair 2400→120 Hz (the second
  // a hair sharp for width), a 3.2 kHz noise sizzle, and a sine sub thump
  // landing at the tail.
  laser() {
    if (!this.ready) return;
    this._tone({ freq: 2400, slide: 120, dur: 0.28, vol: 0.5, attack: 0.002 });
    this._tone({ freq: 2450, slide: 130, dur: 0.24, vol: 0.22, attack: 0.002 });
    this._noise({ dur: 0.15, vol: 0.3, freq: 3200, type: 'highpass' });
    this._tone({ type: 'sine', freq: 110, slide: 42, at: 0.09, dur: 0.22, vol: 0.6, attack: 0.003 });
  }

  // Trishot: three staggered mini-lasers 50ms apart, detuned unison/+1/-1
  // semitone so the volley reads as three distinct bolts.
  trishot() {
    if (!this.ready) return;
    const detune = [1, 1.059, 0.943];
    for (let i = 0; i < 3; i++) {
      const at = i * 0.05;
      this._tone({ freq: 1900 * detune[i], slide: 210, at, dur: 0.11, vol: 0.4, attack: 0.002 });
      this._noise({ at, dur: 0.05, vol: 0.16, freq: 3600, type: 'highpass' });
    }
  }

  // Bomb plant: the low two-beat thud (G2, E2), then three quiet rising
  // "armed" ticks while the fuse cooks.
  bombPlant() {
    if (!this.ready) return;
    this._tone({ type: 'triangle', freq: note(-26), dur: 0.18, vol: 0.7 });
    this._tone({ type: 'triangle', freq: note(-29), at: 0.2, dur: 0.26, vol: 0.7 });
    [7, 12, 16].forEach((n, i) =>
      this._tone({ freq: note(n), at: 0.34 + i * 0.09, dur: 0.03, vol: 0.16 }));
  }

  // Detonate: proper explosion (~800ms). White-noise blast with the lowpass
  // collapsing 4 kHz→80 Hz, an A1→35 Hz sine sub drop, an initial bandpass
  // crack, then three tiny debris ticks trickling out at 90/140/200ms.
  detonate() {
    if (!this.ready) return;
    this._noise({ dur: 0.06, vol: 0.5, freq: 2500, type: 'bandpass', q: 0.7 });
    this._noise({ dur: 0.7, vol: 0.85, freq: 4000, slide: 80 });
    this._tone({ type: 'sine', freq: note(-36), slide: 35, dur: 0.75, vol: 0.9, attack: 0.003 });
    [0.09, 0.14, 0.2].forEach((at, i) =>
      this._noise({ at, dur: 0.03, vol: 0.22 - i * 0.05, freq: 1800 + i * 700, type: 'bandpass', q: 1.4 }));
  }

  // Wall punch: bandpass crunch + square slam falling out of E4 with a sine
  // body under it, then a quieter echo repeat of the whole hit at +120ms.
  wallPunch() {
    if (!this.ready) return;
    this._noise({ dur: 0.08, vol: 0.6, freq: 750, type: 'bandpass', q: 1.3 });
    this._tone({ freq: note(-5), slide: 55, dur: 0.16, vol: 0.6, attack: 0.002 });
    this._tone({ type: 'sine', freq: 95, slide: 40, dur: 0.18, vol: 0.5 });
    this._noise({ at: 0.12, dur: 0.07, vol: 0.28, freq: 700, type: 'bandpass', q: 1.3 });
    this._tone({ freq: note(-7), slide: 50, at: 0.12, dur: 0.13, vol: 0.28, attack: 0.002 });
  }

  // Death riff: 4-note descending lose jingle, A4 F4 D4 A3, last note held.
  deathRiff() {
    if (!this.ready) return;
    [0, -4, -7, -12].forEach((n, i) =>
      this._tone({ freq: note(n), at: i * 0.15, dur: i === 3 ? 0.45 : 0.18, vol: 0.4 }));
  }

  // Round start: "ready go" — C5 C5 G5.
  roundStart() {
    if (!this.ready) return;
    [12, 12, 19].forEach((n, i) =>
      this._tone({ freq: note(n), at: i * 0.11, dur: i === 2 ? 0.22 : 0.09, vol: 0.45 }));
  }

  // Champion fanfare. Win: 7-note ascending A-major arp (square, triumphant).
  // Loss: 6-note descending minor figure (triangle, sadder timbre).
  champion(playerWon = true) {
    if (!this.ready) return;
    if (playerWon) {
      [0, 4, 7, 12, 16, 19, 24].forEach((n, i) =>
        this._tone({ freq: note(n), at: i * 0.09, dur: i === 6 ? 0.35 : 0.11, vol: 0.45 }));
    } else {
      [12, 8, 7, 3, 0, -4].forEach((n, i) =>
        this._tone({ type: 'triangle', freq: note(n), at: i * 0.14, dur: i === 5 ? 0.4 : 0.16, vol: 0.45 }));
    }
  }

  // Insert coin: classic 2-note ding, B5 → E6 (up a fourth), bright + short.
  insertCoin() {
    if (!this.ready) return;
    this._tone({ freq: note(14), dur: 0.07, vol: 0.5 });
    this._tone({ freq: note(19), at: 0.075, dur: 0.18, vol: 0.5 });
  }

  // Engine hum: persistent quiet triangle whose pitch tracks game speed
  // (36–120 Hz). Created once, frequency glides via setTargetAtTime.
  // engineHum(false) releases it — call on game over.
  engineHum(on, speedPct = 0) {
    if (!this.ctx) return;
    if (on) {
      if (!this.hum && this.ready) {
        const osc = this.ctx.createOscillator();
        const g = this.ctx.createGain();
        osc.type = 'triangle';
        osc.frequency.value = 36;
        g.gain.value = 0.14; // bed under the action (0.05 under the old
                             // quiet master measured ~-50 dB: inaudible)
        osc.connect(g); g.connect(this.master);
        osc.onended = () => { osc.disconnect(); g.disconnect(); };
        osc.start();
        this.hum = { osc, g };
      }
      if (this.hum) {
        const pct = Math.max(0, Math.min(1, speedPct));
        this.hum.osc.frequency.setTargetAtTime(36 + pct * 84, this.ctx.currentTime, 0.08);
      }
    } else if (this.hum) {
      const { osc, g } = this.hum;
      this.hum = null;
      g.gain.setTargetAtTime(0, this.ctx.currentTime, 0.03);
      try { osc.stop(this.ctx.currentTime + 0.12); } catch { /* already stopped */ }
    }
  }
}

// ── BGM: procedural 8-bit background track ───────────────────────────────
// A minor, 4 bars of 16th-note steps (64-step loop) at ~126 BPM base.
// Voices route sfx._tone/_noise → lowpass filter → bgm gain → master, so
// mute/stop and speed-brightness act on the whole music bed at once.
// Bass: triangle 8th-note root pump with octave hops. Lead: square 16th
// up-down arp through chord tones, octave accent on bar beats. Hats: tiny
// highpassed noise ticks on 8ths, offbeats slightly louder. Off-16ths are
// delayed 8% of a step for a subtle swing. Zero licensing risk: original
// two-cell pattern, no sampled or transcribed material.

const BASS = [0, -4, -2, 0];        // bar roots: Am F G Am (offsets from A)
const ARPS = [                       // per-bar lead chord tones (from A4)
  [0, 3, 7, 12],   // A C E A
  [-4, 0, 3, 8],   // F A C F
  [-2, 2, 5, 10],  // G B D G
  [0, 3, 7, 12],   // A C E A
];
const LEAD_PAT = [0, 1, 2, 3, 2, 1, 2, 3, 0, 1, 2, 3, 3, 2, 1, 0]; // up-down
// Layered build (owner: 'pretty good but bare .. add chords after a
// while, and maybe a counter melody'): the bed grows as a session runs.
// Bars 0-7: the bare mix. Bars 8+: sustained triad pads (the same
// harmony the arps outline, an octave down). Bars 16+: a counter
// melody answering on beats 2 and 4 — thirds and fifths walking
// against the lead, an octave below, sine so it sits behind.
// Bars 24+ (owner: 'one more layer — make it even more interesting'):
// the drums arrive — kick thump on the downbeats, snare on 2 and 4,
// and a three-hit rising fill closing every other 4-bar loop.
// Bars 32+ (owner: 'rising line', 'more interesting', then 'more
// creative too'): five contours rotate per loop — straight ascent,
// zig-zag, wide leaps into a DROP-AND-VAULT run (falls, then leaps
// to the peak), a chromatic snake, and a minor-6 lift. Phrasing:
// dotted pushes through bars 1-3 (sliding in from a tone below),
// double-time run in bar 4. Every other loop the pushes DISPLACE by
// an 8th (pull instead of push) and the line doubles in parallel
// fifths; every push trails a quiet octave echo three 16ths later.
const RISE_PATS = [
  [0, 2, 3, 5, 7, 8,    10, 12, 14, 15],
  [0, 3, 2, 7, 5, 10,   8, 12, 15, 19],
  [0, 7, 3, 10, 7, 14,  19, 15, 12, 24],
  [0, 3, 5, 7, 8, 10,   12, 15, 17, 19],
  [0, 3, 7, 8, 12, 15,  12, 17, 19, 24],
];
// Bars 40+: the full kit — 16th hats with accents, open hats on the
// off-beats, a ghost snare, and descending tom fills every 4th bar.
// Bars 48+: horn hits — detuned sawtooth stacks stabbing the bar's
// triad on syncopated accents (the and-of-2, plus a bar-4 double).
// Bars 56+: THE SOLO — a doubled, bent sawtooth guitar line over the
// whole groove; 8 bars on, 8 bars off, so it phrases like a player
// and not a siren. Entries: [s16, semi, durSteps, bendFromSemi?].
const SOLO = [
  [[0, 12, 2], [2, 15, 2], [4, 17, 2], [6, 19, 6, 17], [12, 15, 2], [14, 12, 2]],
  [[0, 17, 4], [4, 15, 2], [6, 12, 2], [8, 17, 8, 15]],
  [[0, 14, 2], [2, 17, 2], [4, 19, 2], [6, 22, 4], [10, 19, 2], [12, 17, 2], [14, 14, 2]],
  [[0, 12, 10, 10], [12, 19, 2], [14, 24, 2]],
  [[0, 12, 1], [1, 15, 1], [2, 17, 1], [3, 19, 1], [4, 22, 1], [5, 24, 1], [6, 27, 6, 24], [14, 22, 2]],
  [[0, 24, 4], [4, 22, 2], [6, 19, 2], [8, 17, 4, 19], [12, 15, 2]],
  [[0, 14, 2], [2, 14, 1], [3, 14, 1], [4, 17, 2], [6, 19, 2], [8, 22, 2], [10, 24, 2], [12, 26, 4, 24]],
  [[0, 24, 6, 26], [8, 19, 2], [10, 15, 2], [12, 12, 8]],
];
const COUNTER = [
  [7, 3],    // over Am: E then C
  [3, 0],    // over F:  C then A
  [5, 2],    // over G:  D then B
  [3, 7],    // over Am: C then E (lift into the loop restart)
];

export class Bgm {
  constructor(sfx) {
    this.sfx = sfx;
    this.level = 0.32;        // bed sits under the one-shot sfx
    this.muted = false;
    this.playing = false;
    this.speedPct = 50;       // 0–100; 50 = nominal 126 BPM / neutral filter
    this.step = 0;
    this.nextT = 0;
    this.timer = null;
    this.chain = null;        // { filter, gain } lazily built on first start
    this.file = null;         // { el, src } when music.mp3 took over
    this._fileTried = false;  // HEAD-probe music.mp3 once per instance
    this._wantStart = null;   // start() pre-unlock → remembered, kicked later
  }

  get _ctx() { return this.sfx.ctx; }
  _speedMul() { return 0.85 + Math.min(100, Math.max(0, this.speedPct)) / 100 * 0.3; }
  _brightHz() { return 1200 + Math.min(100, Math.max(0, this.speedPct)) / 100 * 2400; }

  async start(speedPct = this.speedPct) {
    this.speedPct = speedPct;
    this._wantStart = speedPct;
    if (!this.sfx.ready) return;              // pre-unlock: kicked by unlock()
    if (this.playing) { this.setSpeed(speedPct); return; }
    if (!this._fileTried) {                   // file slot: music.mp3 wins
      this._fileTried = true;
      try {
        const r = await fetch('music.mp3', { method: 'HEAD' });
        if (r && r.ok && typeof Audio !== 'undefined') { this._startFile(); return; }
      } catch { /* no file server / no file → procedural */ }
      if (this.playing) return;               // a parallel start() beat us
    }
    this._startProcedural();
  }

  setSpeed(speedPct) {
    this.speedPct = Math.min(100, Math.max(0, +speedPct || 0));
    if (!this._ctx) return;
    if (this.chain)
      this.chain.filter.frequency.setTargetAtTime(this._brightHz(), this._ctx.currentTime, 0.1);
    if (this.file) this.file.el.playbackRate = this._speedMul();
  }

  stop() {
    this._wantStart = null;
    if (!this.playing) return;
    this.playing = false;
    if (this.timer) { clearTimeout(this.timer); this.timer = null; }
    if (this.file) {
      try { this.file.el.pause(); } catch { /* not playing */ }
      try { this.file.src.disconnect(); } catch { /* already gone */ }
      this.file = null;
    }
    this._applyGain();
  }

  toggleMute() {
    this.muted = !this.muted;
    this._applyGain();
    return this.muted;
  }

  // Called from unlock() once the context is running; honors a start() that
  // arrived before the first user gesture.
  _kick() {
    if (this._wantStart != null && !this.playing && this.sfx.ready) {
      const p = this._wantStart;
      this._wantStart = null;
      this.start(p);
    }
  }

  _ensureChain() {
    if (this.chain || !this._ctx) return;
    const filter = this._ctx.createBiquadFilter();
    filter.type = 'lowpass';
    filter.frequency.value = this._brightHz();
    const gain = this._ctx.createGain();
    gain.gain.value = 0; // opened by _applyGain()
    filter.connect(gain); gain.connect(this.sfx.master);
    this.chain = { filter, gain };
    // FX buses (the fullness pass): a slap delay for snare/horns, a
    // longer ping delay for the solo, and a tanh waveshaper that turns
    // the solo's sawtooth into an actual screaming guitar. Sends run
    // dry+wet in parallel into the main filter, so the speed-coupled
    // brightness still governs everything.
    const ctx = this._ctx;
    const mkDelay = (time, fb, wet) => {
      const inp = ctx.createGain();
      const del = ctx.createDelay(1.0);
      del.delayTime.value = time;
      const fbg = ctx.createGain(); fbg.gain.value = fb;
      const wg = ctx.createGain(); wg.gain.value = wet;
      inp.connect(filter);                 // dry
      inp.connect(del); del.connect(fbg); fbg.connect(del);
      del.connect(wg); wg.connect(filter); // wet
      return inp;
    };
    const snareIn = mkDelay(0.09, 0.22, 0.3);
    const soloPing = mkDelay(0.28, 0.35, 0.33);
    const shaper = ctx.createWaveShaper();
    const curve = new Float32Array(1024);
    for (let i = 0; i < 1024; i++) {
      const x = (i / 511.5) - 1;
      curve[i] = Math.tanh(3.2 * x);
    }
    shaper.curve = curve;
    const soloIn = ctx.createGain(); soloIn.gain.value = 0.55;
    shaper.connect(soloPing);
    soloIn.connect(shaper);
    this.fx = { snareIn, soloIn };
  }

  _applyGain() {
    if (!this.chain || !this._ctx) return;
    const v = this.muted || !this.playing ? 0 : this.level;
    this.chain.gain.gain.setTargetAtTime(v, this._ctx.currentTime, 0.03);
  }

  _startFile() {
    if (this.playing) return;
    this._ensureChain();
    const el = new Audio('music.mp3');
    el.loop = true;
    el.playbackRate = this._speedMul();
    const src = this._ctx.createMediaElementSource(el);
    src.connect(this.chain.filter);
    this.file = { el, src };
    this.playing = true;
    this._applyGain();
    const p = el.play();
    if (p) p.catch(() => { // gesture raced us — drop to the procedural loop
      try { src.disconnect(); } catch { /* ignore */ }
      this.file = null;
      this.playing = false;
      this._startProcedural();
    });
  }

  // THE ARRANGER (owner: 'random mix and match — each feature should
  // be mixable with other features'): every 8 bars a SCENE is rolled —
  // either a special section (the pocket, new jack swing, the quiet
  // build into the tom drop, or a breakdown into the solo) or a groove
  // scene with every layer and bass personality toggled independently.
  // Sparse early, denser as the session matures.
  _rollScene(i) {
    const r = Math.random;
    if (i >= 5 && r() < 0.3) {
      const mode = ['pocket', 'njs', 'build', 'solo'][(r() * 4) | 0];
      return { mode, chords: mode !== 'pocket', counter: false, drums: true,
               kit: true, horns: false, strings: r() < 0.5, rise: false, bass: 0 };
    }
    return {
      mode: 'groove',
      chords: i >= 1 && r() < 0.8,
      counter: i >= 2 && r() < 0.6,
      drums: i >= 2 || r() < 0.4,
      kit: i >= 3 && r() < 0.65,
      horns: i >= 4 && r() < 0.4,
      strings: i >= 3 && r() < 0.5,
      rise: i >= 2 && r() < 0.5,
      bass: i < 1 ? 0 : (r() * 4) | 0,
    };
  }

  // Audition hook: pin a scene by name until the next boundary.
  forceScene(mode) {
    const full = { mode: 'groove', chords: true, counter: true, drums: true,
                   kit: true, horns: true, strings: true, rise: true, bass: 1 };
    this.scene = mode === 'full' ? full
      : { ...full, mode, counter: false, horns: false, rise: false };
    this.holdScene = true;   // the mixer's scene sticks until released
  }

  // Hand control back to the roller (the 'auto' mode).
  releaseScene() {
    this.holdScene = false;
  }

  _startProcedural() {
    if (this.playing || !this.sfx.ready) return;
    this._ensureChain();
    this.playing = true;
    this.totalSteps = 0;
    this.sceneIdx = -1;
    this.scene = null;
    this.step = 0;
    this.nextT = this._ctx.currentTime + 0.06;
    this._applyGain();
    this._tick();
  }

  // Lookahead scheduler: book steps ~120ms ahead, re-read tempo each step
  // so setSpeed() bends the groove within one 16th.
  _tick() {
    if (!this.playing) return;
    const horizon = this._ctx.currentTime + 0.12;
    while (this.nextT < horizon) {
      this._scheduleStep(this.step, this.nextT);
      this.nextT += 60 / (126 * this._speedMul()) / 4;
      this.step = (this.step + 1) % 64;
    }
    this.timer = setTimeout(() => this._tick(), 40);
  }

  _scheduleStep(step, t) {
    const bar = (step / 16) | 0, s16 = step % 16;
    const d = 60 / (126 * this._speedMul()) / 4;
    // The layered build: musical time decides what has joined —
    // computed FIRST (before swing: sections swing differently).
    // From bar 64 a 32-bar SUPER-CYCLE rotates the back half:
    // solo (8) -> drum & bass (8) -> new jack swing (8) -> the quiet
    // build into the gated-tom drop (8) -> solo …  Era sections are
    // STYLE homages with original lines: the njs shuffle/claps/
    // orchestra hits, and the gated-reverb tom cascade, are idioms —
    // no borrowed melodies.
    const totalBar = ((this.totalSteps || 0) / 16) | 0;
    this.totalSteps = (this.totalSteps || 0) + 1;
    const sceneIdx = (totalBar / 8) | 0;
    if (!this.scene || (!this.holdScene && sceneIdx !== this.sceneIdx)) {
      this.sceneIdx = sceneIdx;
      const next = (!this.holdScene && this.nextScene) || this._rollScene(sceneIdx);
      // NATURAL TRANSITIONS (owner): remember whether the show changed
      // so the entrance can be stamped with a crash.
      const key = JSON.stringify(next);
      this.sceneChanged = this.lastSceneKey != null && this.lastSceneKey !== key;
      this.lastSceneKey = key;
      this.scene = next;
      this.nextScene = null;
    } else {
      this.sceneIdx = sceneIdx;
    }
    const sc = this.scene;
    const sceneBar = totalBar % 8;
    // Look ahead one bar: the last bar of a scene knows what's coming
    // and telegraphs a change with a drum fill.
    if (!this.holdScene && sceneBar === 7 && s16 === 0 && !this.nextScene) {
      this.nextScene = this._rollScene(sceneIdx + 1);
    }
    const fillTime = sceneBar === 7 && this.nextScene
      && JSON.stringify(this.nextScene) !== JSON.stringify(sc)
      && this.nextScene.mode !== 'build';
    const dnb = sc.mode === 'pocket';
    const njs = sc.mode === 'njs';
    const collins = sc.mode === 'build';
    const sect = collins ? 24 + sceneBar : -1;  // cascade at 30/31
    const soloOn = sc.mode === 'solo' && sceneBar >= 1;
    const breakdown = sc.mode === 'solo' && sceneBar === 0;
    const special = dnb || njs || collins;
    if (s16 % 2 === 1) t += d * (dnb ? 0.02 : njs ? 0.3 : 0.08);
    const at = t - this._ctx.currentTime, out = this.chain.filter;
    // BASS (owner: 'more interesting periodically'): the personality
    // rotates per 4-bar loop once the intro is done — 0 the straight
    // pump, 1 funk (beat 3 dropped, octave pops on the and-of-2/4),
    // 2 the pump with a chord-walk bar (root-3rd-5th-7th quarters)
    // climbing into the loop restart, 3 a 16th gallop with slide-in
    // roots. Sub and hats keep their own grid throughout.
    const bassMode = sc.bass;
    if (dnb) {
      // THE GROOVE (owner: 'was thinking more michael jackson'): a
      // stripped drums-and-bass pocket, Billie-Jean-school — DRY
      // four-on-the-floor kick, tight backbeat, crisp 8th hats, and a
      // syncopated ostinato bassline (original line, scale-fit per
      // bar). Nothing else on top: the pocket IS the section.
      // THE KILLER DRUMS (owner: '"in the air tonight" type … epic'):
      // half-time weight under the driving bass. Giant GATED snare —
      // a staggered noise bloom chopped dead (the gate) over a deep
      // shell — huge kick, quiet 8th hats for motion, and gated tom
      // pickups answering the line every 4th bar.
      if (s16 === 0 || s16 === 8 || (bar % 2 === 1 && s16 === 10)) {
        this.sfx._tone({ type: 'sine', freq: note(-33), slide: 30, at, dur: 0.26, vol: 0.62, attack: 0.002, out });
        this.sfx._noise({ at, dur: 0.014, vol: 0.3, freq: 3000, type: 'highpass', out });
        this.sfx._noise({ at, dur: 0.09, vol: 0.14, freq: 300, type: 'bandpass', out });
      }
      if (s16 === 4 || s16 === 12) {
        const sOut = (this.fx && this.fx.snareIn) || out;
        // The gate: three blooms, each bigger band, chopped hard.
        this.sfx._noise({ at, dur: 0.16, vol: 0.38, freq: 1600, type: 'bandpass', out: sOut });
        this.sfx._noise({ at: at + 0.02, dur: 0.14, vol: 0.24, freq: 2400, type: 'bandpass', out: sOut });
        this.sfx._noise({ at: at + 0.04, dur: 0.12, vol: 0.14, freq: 3400, type: 'bandpass', out: sOut });
        this.sfx._tone({ type: 'triangle', freq: 185, at, dur: 0.09, vol: 0.2, attack: 0.001, out: sOut });
      }
      if (s16 % 2 === 0) {
        this.sfx._noise({ at, dur: 0.018, vol: s16 % 4 === 2 ? 0.08 : 0.05, freq: 7500, type: 'highpass', out });
      }
      if (bar % 4 === 3 && s16 >= 13) {
        const sOut = (this.fx && this.fx.snareIn) || out;
        this.sfx._tone({ type: 'triangle', freq: note([-12, -16, -19][s16 - 13]), slide: 25, at, dur: 0.13, vol: 0.5, attack: 0.002, out: sOut });
        this.sfx._noise({ at, dur: 0.09, vol: 0.22, freq: 850, type: 'bandpass', out: sOut });
      }
      if (s16 % 2 === 0) {
        // The ostinato: root, 5th, 7th-ish peak and back, per bar root.
        const LINE = [
          [0, 7, 10, 12, 10, 7, 0, 7],
          [-4, 3, 5, 8, 5, 3, -4, 3],
          [-2, 5, 8, 10, 8, 5, -2, 5],
          [0, 7, 10, 12, 10, 7, 0, 7],
        ];
        const semi = LINE[bar][s16 / 2];
        // DRIVING ELECTRIC BASS (owner): pick attack, staccato punch,
        // relentless 8ths. Saw carries the string bite (near-instant
        // attack, tight decay), square adds knuckle, sine holds the
        // octave-down chest. Downbeats and beat 3 accent harder.
        const acc = s16 % 8 === 0 ? 1.0 : 0.82;
        this.sfx._tone({
          type: 'sawtooth', freq: note(-24 + semi) * 1.015, slide: note(-24 + semi),
          at, dur: d * 1.15, vol: 0.34 * acc, attack: 0.0015, out,
        });
        this.sfx._tone({ type: 'square', freq: note(-24 + semi), at, dur: d * 0.9, vol: 0.1 * acc, attack: 0.0015, out });
        this.sfx._tone({ type: 'sine', freq: note(-36 + semi), at, dur: d * 1.5, vol: 0.36 * acc, attack: 0.003, out });
      }
      // Crash the section entrance.
      if (sceneBar === 0 && s16 === 0) {
        this.sfx._noise({ at, dur: 0.7, vol: 0.22, freq: 5200, type: 'highpass', out });
      }
    } else if (njs) {
      // NEW JACK SWING: hard-swung shuffle, kick 1 / and-of-2 / and-of-3,
      // clap-stacked backbeat, staccato funk bass, orchestra hits.
      if (s16 === 0 || s16 === 7 || s16 === 10) {
        this.sfx._tone({ type: 'sine', freq: note(-31), slide: 34, at, dur: 0.18, vol: 0.58, attack: 0.002, out });
        this.sfx._noise({ at, dur: 0.012, vol: 0.28, freq: 3500, type: 'highpass', out });
      }
      if (s16 === 4 || s16 === 12) {
        const sOut = (this.fx && this.fx.snareIn) || out;
        this.sfx._noise({ at, dur: 0.09, vol: 0.28, freq: 1800, type: 'bandpass', out: sOut });
        this.sfx._noise({ at: at + 0.014, dur: 0.06, vol: 0.16, freq: 2300, type: 'bandpass', out: sOut });
        this.sfx._tone({ type: 'triangle', freq: 195, at, dur: 0.06, vol: 0.14, attack: 0.001, out: sOut });
      }
      this.sfx._noise({
        at, dur: s16 === 14 ? 0.09 : 0.02,
        vol: [0.11, 0.05, 0.08, 0.06][s16 % 4], freq: 7000, type: 'highpass', out,
      });
      {
        const root = BASS[bar];
        const stab = { 0: root, 3: root, 6: root + 12, 10: root + 12, 13: root + 7 }[s16];
        if (stab != null) {
          this.sfx._tone({ type: 'triangle', freq: note(-24 + stab), at, dur: d * 0.7, vol: 0.46, out });
        }
        if (s16 === 0 || (bar % 2 === 1 && s16 === 6)) {
          // Orchestra hit: the era's exclamation mark — triad stacked
          // three octaves, saw+square, slapback, hard cutoff.
          const hOut = (this.fx && this.fx.snareIn) || out;
          for (const c of ARPS[bar].slice(0, 3)) {
            for (const oct of [-12, 0, 12]) {
              this.sfx._tone({ type: 'sawtooth', freq: note(c + oct), at, dur: 0.13, vol: 0.09, attack: 0.002, out: hOut });
              this.sfx._tone({ type: 'square', freq: note(c + oct), at, dur: 0.11, vol: 0.05, attack: 0.002, out: hOut });
            }
          }
        }
      }
    } else if (collins) {
      // THE QUIET BUILD: pads carry it; sub heartbeat; a high drone —
      // then the gated-reverb tom cascade breaks the sky open.
      if (sect < 30) {
        if (s16 === 0) {
          this.sfx._tone({ type: 'sine', freq: note(-36 + BASS[bar]), at, dur: d * 4, vol: 0.22, attack: 0.03, out });
          this.sfx._tone({ type: 'sine', freq: note(19), at, dur: d * 16, vol: 0.045, attack: d * 4, out });
        }
        if (s16 % 8 === 0) {
          this.sfx._noise({ at, dur: 0.02, vol: 0.04, freq: 7000, type: 'highpass', out });
        }
      } else {
        const sOut = (this.fx && this.fx.snareIn) || out;
        const hit = (semi, v) => {
          this.sfx._tone({ type: 'triangle', freq: note(semi), slide: 25, at, dur: 0.12, vol: v, attack: 0.002, out: sOut });
          this.sfx._noise({ at, dur: 0.08, vol: v * 0.5, freq: 900, type: 'bandpass', out: sOut });
        };
        if (sect === 30 && s16 >= 8 && s16 % 2 === 0) {
          hit([-9, -12, -14, -17][(s16 - 8) / 2], 0.5);
        }
        if (sect === 31 && s16 % 2 === 0) {
          hit([-9, -12, -14, -17, -19, -21, -24, -26][s16 / 2], 0.55);
        }
      }
    } else if (s16 % 2 === 0) {
      const root = BASS[bar];
      let semi = root + ((s16 / 2) % 2 ? 12 : 0);
      let slideFrom = null;
      if (bassMode === 1 && s16 === 8) semi = null;
      if (bassMode === 2 && bar === 3) {
        semi = s16 % 4 === 0 ? root + [0, 3, 7, 10][s16 / 4] : null;
      }
      if (bassMode === 3 && s16 === 0) slideFrom = root - 3;
      if (semi != null) {
        this.sfx._tone({
          type: 'triangle',
          freq: note(-24 + (slideFrom != null ? slideFrom : semi)),
          slide: slideFrom != null ? note(-24 + semi) : undefined,
          at, dur: d * 1.8, vol: 0.5, out,
        });
        this.sfx._tone({
          type: 'sawtooth', freq: note(-24 + semi), at,
          dur: d * 1.5, vol: 0.13, attack: 0.004, out,
        });
      }
      if (s16 % 4 === 0) {                          // sine sub: glue + weight
        this.sfx._tone({ type: 'sine', freq: note(-36 + root), at, dur: d * 3.6, vol: 0.27, attack: 0.01, out });
      }
      if (!breakdown) this.sfx._noise({ at, dur: 0.03, vol: s16 % 4 === 2 ? 0.12 : 0.07, freq: 6500, type: 'highpass', out });
    } else if (bassMode === 1 && (s16 === 7 || s16 === 15)) {
      // Funk pops: octave stabs pushing into the next downbeat.
      this.sfx._tone({ type: 'triangle', freq: note(-12 + BASS[bar]), at, dur: d * 0.8, vol: 0.36, out });
    } else if (bassMode === 3 && s16 % 4 === 1) {
      // Gallop: quiet low-root 16th right behind each 8th.
      this.sfx._tone({ type: 'triangle', freq: note(-24 + BASS[bar]), at, dur: d * 0.7, vol: 0.2, out });
    }
    // THE BREAKDOWN: the bar before each solo entry strips to bass +
    // kick, so the solo slams in over the crash (flags computed at the
    // top of the step).
    if (!breakdown && !njs && !collins && !dnb) {
      const semi = ARPS[bar][LEAD_PAT[s16]] + (s16 % 8 === 0 ? 12 : 0);
      this.sfx._tone({ freq: note(semi), at, dur: d * 0.92, vol: s16 % 4 === 0 ? 0.3 : 0.2, out });
    }
    if (sc.chords && s16 === 0 && !breakdown && !dnb) {
      // Chord pad: the bar's triad, octave down, whole-bar sustain with
      // a slow attack so it swells in under the arp.
      for (const c of ARPS[bar].slice(0, 3)) {
        this.sfx._tone({
          type: 'sawtooth', freq: note(c - 12), at, dur: d * 15.5,
          vol: 0.085, attack: d * 4, out,
        });
      }
    }
    if (sc.counter && (s16 === 4 || s16 === 12) && !breakdown && !special && !soloOn) {
      // Counter melody: answers on beats 2 and 4 — and steps back to
      // half volume while the soloist has the floor.
      const c = COUNTER[bar][s16 === 4 ? 0 : 1];
      this.sfx._tone({
        type: 'sine', freq: note(c - 12), at, dur: d * 3.4,
        vol: soloOn ? 0.12 : 0.24, attack: 0.02, out,
      });
    }
    if (sc.drums && !special) {
      // The kit: kick on the downbeats (sine thump sliding down),
      // snare on 2 and 4 (bandpassed noise burst).
      if (s16 === 0 || s16 === 8) {
        this.sfx._tone({
          type: 'sine', freq: note(-31), slide: 34, at, dur: 0.24,
          vol: 0.6, attack: 0.002, out,
        });
      }
      if ((s16 === 4 || s16 === 12) && !breakdown) {
        const sOut = (this.fx && this.fx.snareIn) || out;
        this.sfx._noise({ at, dur: 0.09, vol: 0.28, freq: 1800, type: 'bandpass', out: sOut });
        this.sfx._tone({ type: 'triangle', freq: 195, at, dur: 0.07, vol: 0.14, attack: 0.001, out: sOut });
      }
      // Rising three-hit fill closing every OTHER loop — pre-kit phase
      // only; the full kit replaces it with a real snare roll.
      if (totalBar % 8 === 7 && s16 >= 13 && !sc.kit) {
        this.sfx._noise({
          at, dur: 0.05, vol: 0.12 + (s16 - 13) * 0.08,
          freq: 2400 + (s16 - 13) * 900, type: 'bandpass', out,
        });
      }
    }
    if (sc.rise && !breakdown && !soloOn && !special) {
      const loopIdx = (totalBar / 4) | 0;
      const pat = RISE_PATS[loopIdx % RISE_PATS.length];
      const shifted = loopIdx % 2 === 1;           // displaced loop
      const [hitA, hitB] = shifted ? [2, 8] : [0, 6];
      let idx = -1, run = false;
      if (bar < 3 && (s16 === hitA || s16 === hitB)) {
        idx = bar * 2 + (s16 === hitB ? 1 : 0);
      }
      if (bar === 3 && s16 % 4 === 0) { idx = 6 + s16 / 4; run = true; }
      if (idx >= 0) {
        // On-pitch, clean attack (owner: 'less swoopy/out of tune' —
        // the old slide-in from a tone below WAS the swoop).
        const target = note(pat[idx]);
        const vol = 0.12 + idx * 0.016;
        this.sfx._tone({
          type: 'triangle', freq: target,
          at, dur: run ? d * 3.4 : d * 5.2,
          vol, attack: run ? 0.01 : 0.03, out,
        });
        if (shifted) {
          // Parallel fifths on displaced loops: thicker, hymnal.
          this.sfx._tone({
            type: 'triangle', freq: note(pat[idx] + 7),
            at, dur: run ? d * 3.4 : d * 5.2,
            vol: vol * 0.55, attack: run ? 0.01 : d * 1.2, out,
          });
        }
        if (!run) {
          // Octave echo three 16ths behind every push.
          this.sfx._tone({
            type: 'triangle', freq: note(pat[idx] + 12),
            at: at + d * 3, dur: d * 2.2, vol: vol * 0.38,
            attack: 0.01, out,
          });
        }
      }
    }
    if (sc.kit && !breakdown && !special) {
      // Full kit: quiet 16ths between the existing hats, open hats on
      // the off-beats, a ghost snare on the e-of-3, and a descending
      // tom fill closing every 4th bar.
      if (s16 % 2 === 1 && s16 !== 7 && s16 !== 15) {
        this.sfx._noise({ at, dur: 0.02, vol: 0.05, freq: 7000, type: 'highpass', out });
      }
      if (s16 === 6 || s16 === 14) {
        this.sfx._noise({ at, dur: 0.12, vol: 0.11, freq: 6000, type: 'highpass', out });
      }
      if (s16 === 11) {
        this.sfx._noise({ at, dur: 0.05, vol: 0.1, freq: 1800, type: 'bandpass', out });
      }
      if (totalBar % 8 === 3 && s16 >= 12) {
        const toms = [-14, -17, -21, -24];
        this.sfx._tone({
          type: 'triangle', freq: note(toms[s16 - 12]), slide: 30,
          at, dur: 0.11, vol: 0.4, attack: 0.002, out,
        });
      }
      // AWESOME PASS (owner): layered transients and groove.
      // Kick click for punch, on every kick — plus a funk push on the
      // and-of-3 every other bar.
      if (s16 === 0 || s16 === 8 || (s16 === 10 && totalBar % 2 === 1)) {
        this.sfx._noise({ at, dur: 0.012, vol: 0.3, freq: 3500, type: 'highpass', out });
        if (s16 === 10) {
          this.sfx._tone({
            type: 'sine', freq: note(-31), slide: 38, at, dur: 0.12,
            vol: 0.42, attack: 0.002, out,
          });
        }
      }
      // Snare crack: tonal body + click under the noise burst.
      if (s16 === 4 || s16 === 12) {
        this.sfx._tone({
          type: 'triangle', freq: note(-20), slide: 140, at, dur: 0.06,
          vol: 0.26, attack: 0.001, out,
        });
        this.sfx._noise({ at, dur: 0.015, vol: 0.2, freq: 4500, type: 'highpass', out });
      }
      // Clap doubling the 4-backbeat every other loop: two offset bursts.
      if (s16 === 12 && ((totalBar / 4) | 0) % 2 === 1) {
        this.sfx._noise({ at: at + 0.012, dur: 0.05, vol: 0.16, freq: 2200, type: 'bandpass', out });
        this.sfx._noise({ at: at + 0.026, dur: 0.07, vol: 0.12, freq: 2600, type: 'bandpass', out });
      }
      // Crash splash topping every 8-bar cycle.
      if (totalBar % 8 === 0 && s16 === 0) {
        this.sfx._noise({ at, dur: 0.7, vol: 0.2, freq: 5200, type: 'highpass', out });
      }
      // Accelerating snare roll into every cycle top.
      if (totalBar % 8 === 7 && s16 >= 8) {
        this.sfx._noise({
          at, dur: 0.035, vol: 0.05 + (s16 - 8) * 0.028,
          freq: 1800, type: 'bandpass', out,
        });
      }
    }
    // AIRY ORCHESTRAL STRINGS (owner): detuned saw ensembles, slow
    // attacks, whisper volume, high register — a two-voice shimmer
    // (high 5th + root two octaves up) over the groove from bar 48,
    // and the full four-voice section in the quiet build, where
    // strings live. Each voice is three saws a few cents apart: the
    // beating IS the air. Never in the pocket (drums and bass only).
    {
      const strings =
        (sc.strings && !breakdown && !dnb && !njs) || (collins && sect < 30);
      if (strings && s16 === 0 && totalBar % 2 === 0) {
        const semis = collins
          ? [ARPS[bar][0] + 12, ARPS[bar][1] + 12, ARPS[bar][2] + 12, ARPS[bar][0] + 24]
          : [ARPS[bar][2] + 12, ARPS[bar][0] + 24];
        for (const semi of semis) {
          for (const det of [0.9955, 1.0, 1.0055]) {
            this.sfx._tone({
              type: 'sawtooth', freq: note(semi) * det, at,
              dur: d * 31, vol: collins ? 0.038 : 0.027,
              attack: d * 8, out,
            });
          }
        }
      }
    }
    if (sc.horns && !breakdown && !special) {
      // Horn hits: the bar's triad as a tight detuned-saw stack, sharp
      // attack, short — the and-of-2 every bar, doubled in bar 4.
      const stab = s16 === 6 || (bar === 3 && s16 === 4);
      if (stab) {
        const hOut = (this.fx && this.fx.snareIn) || out;
        for (const c of ARPS[bar].slice(0, 3)) {
          for (const det of [1, 1.006]) {
            this.sfx._tone({
              type: 'sawtooth', freq: note(c) * det, at, dur: 0.16,
              vol: 0.11, attack: 0.004, out: hOut,
            });
          }
        }
      }
    }
    // Transition drama: the telegraph fill into a changing scene, and
    // the crash stamping a changed scene's first downbeat (kit scenes
    // already crash their cycle top; quiet build earns its silence).
    if (fillTime && s16 >= 10 && (sc.drums || sc.kit || dnb || njs)) {
      this.sfx._noise({
        at, dur: 0.04, vol: 0.06 + (s16 - 10) * 0.045,
        freq: 1800, type: 'bandpass', out,
      });
      if (s16 === 12 || s16 === 14) {
        this.sfx._tone({
          type: 'triangle', freq: note(s16 === 12 ? -14 : -19), slide: 25,
          at, dur: 0.1, vol: 0.34, attack: 0.002, out,
        });
      }
    }
    if (this.sceneChanged && sceneBar === 0 && s16 === 0
        && !collins && !breakdown && !sc.kit && (sc.drums || dnb || njs)) {
      this.sfx._noise({ at, dur: 0.6, vol: 0.18, freq: 5200, type: 'highpass', out });
    }
    if (soloOn) {
      // THE SOLO: its scene rolls it, the breakdown bar launches it.
      const soloBar = (totalBar - 1) % 8;
      const gOut = (this.fx && this.fx.soloIn) || out;
      for (const [hit, semi, durSteps, from] of SOLO[soloBar]) {
        if (hit === s16) {
          const target = note(semi);
          const start = from != null ? note(from) : target;
          if (durSteps >= 4) {
            // Held note: bend in, then VIBRATO — successive micro
            // slides above/below pitch for the wail.
            const seg = d * durSteps / 4;
            this.sfx._tone({ type: 'sawtooth', freq: start, slide: target, at, dur: seg * 1.05, vol: 0.3, attack: 0.008, out: gOut });
            for (let i = 1; i < 4; i++) {
              const wob = i % 2 ? target * 1.012 : target * 0.994;
              this.sfx._tone({ type: 'sawtooth', freq: i % 2 ? target : wob, slide: i % 2 ? wob : target, at: at + seg * i, dur: seg * 1.05, vol: 0.28, attack: 0.002, out: gOut });
            }
            // Octave-down double for power on the holds.
            this.sfx._tone({ type: 'sawtooth', freq: target / 2, at, dur: d * durSteps * 0.95, vol: 0.1, attack: 0.01, out: gOut });
          } else {
            for (const [det, v] of [[1, 0.3], [1.007, 0.12]]) {
              this.sfx._tone({
                type: 'sawtooth', freq: start * det,
                slide: from != null ? target * det : undefined,
                at, dur: d * durSteps * 0.95, vol: v, attack: 0.006, out: gOut,
              });
            }
          }
        }
      }
    }
  }
}
