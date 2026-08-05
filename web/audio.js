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
    this.volume = 0.06;   // cabinet default — quiet enough to layer voices
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
        g.gain.value = 0.05; // subtle bed under the action
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

  _startProcedural() {
    if (this.playing || !this.sfx.ready) return;
    this._ensureChain();
    this.playing = true;
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
    if (s16 % 2 === 1) t += d * 0.08;              // subtle swing
    const at = t - this._ctx.currentTime, out = this.chain.filter;
    if (s16 % 2 === 0) {                            // bass: 8th-note pump
      const off = BASS[bar] + ((s16 / 2) % 2 ? 12 : 0);
      this.sfx._tone({ type: 'triangle', freq: note(-24 + off), at, dur: d * 1.8, vol: 0.5, out });
      this.sfx._noise({ at, dur: 0.03, vol: s16 % 4 === 2 ? 0.12 : 0.07, freq: 6500, type: 'highpass', out });
    }
    const semi = ARPS[bar][LEAD_PAT[s16]] + (s16 % 8 === 0 ? 12 : 0);
    this.sfx._tone({ freq: note(semi), at, dur: d * 0.92, vol: s16 % 4 === 0 ? 0.3 : 0.2, out });
  }
}
