// Render the session soundtrack: replay the logged sfx events through
// the REAL audio.js voices into an OfflineAudioContext, export WAV.
import { chromium } from 'playwright';
import fs from 'fs';
const OUT = process.cwd();
const log = JSON.parse(fs.readFileSync(`${OUT}/sfx-log.json`, 'utf8'));
const durMs = Math.max(...log.sfx.map(([t]) => t), 1000) + 3000;

const b = await chromium.launch();
const p = await b.newPage();
await p.goto('http://localhost:8082/', { waitUntil: 'domcontentloaded' });
const dl = p.waitForEvent('download', { timeout: 600000 });
const wav = await p.evaluate(async ({ events, durMs }) => {
  const rate = 44100;
  const off = new OfflineAudioContext(2, Math.ceil(durMs / 1000 * rate), rate);
  // Shim: audio.js unlock() creates `new AudioContext()` — hand it ours.
  globalThis.AudioContext = function () { return off; };
  const mod = await import('./audio.js');
  const Sfx = mod.Sfx || mod.default;
  const sfx = new Sfx();
  sfx.unlock();
  // OfflineAudioContext sits 'suspended' until startRendering — the
  // ready-gate would silently no-op every voice. Shadow it.
  Object.defineProperty(sfx, 'ready', { value: true });
  sfx.volume = 0.5;
  if (sfx.master) sfx.master.gain.value = 0.5;
  // Re-anchor every voice at its logged timestamp: all voices funnel
  // through _tone/_noise — offset their `at` by the current event time.
  let OFF = 0;
  const tone = sfx._tone.bind(sfx);
  sfx._tone = (o) => tone({ ...o, at: (o.at || 0) + OFF });
  if (sfx._noise) {
    const noise = sfx._noise.bind(sfx);
    sfx._noise = (o) => noise({ ...o, at: (o.at || 0) + OFF });
  }
  let foodSeen = 0;
  for (const [t, ev] of events) {
    OFF = t / 1000;
    if (!Array.isArray(ev) || ev.length < 3) continue;
    if (ev.length < 4) { sfx.play(ev[0], ev[1], ev[2]); continue; }
    const [kind, freq, dur, delay] = ev;
    switch (kind) {
      case 0: if (foodSeen % 2 === 0) sfx.food(Math.round((freq - 880) / 40)); foodSeen++; break;
      case 1: sfx.powerup(); break;
      case 2: sfx.laser(); break;
      case 3: sfx.trishot(); break;
      case 4: sfx.bombPlant(); break;
      case 5: sfx.detonate(); break;
      case 6: sfx.wallPunch(); break;
      case 7: sfx.deathRiff(); break;
      default: sfx.play(freq, dur, delay);
    }
  }
  const buf = await off.startRendering();
  // Float32 stereo -> 16-bit PCM WAV.
  const n = buf.length, ch = 2;
  const data = new DataView(new ArrayBuffer(44 + n * ch * 2));
  const w = (o, s) => { for (let i = 0; i < s.length; i++) data.setUint8(o + i, s.charCodeAt(i)); };
  w(0, 'RIFF'); data.setUint32(4, 36 + n * ch * 2, true); w(8, 'WAVEfmt ');
  data.setUint32(16, 16, true); data.setUint16(20, 1, true); data.setUint16(22, ch, true);
  data.setUint32(24, rate, true); data.setUint32(28, rate * ch * 2, true);
  data.setUint16(32, ch * 2, true); data.setUint16(34, 16, true);
  w(36, 'data'); data.setUint32(40, n * ch * 2, true);
  const L = buf.getChannelData(0), R = buf.getChannelData(1);
  for (let i = 0; i < n; i++) {
    data.setInt16(44 + i * 4, Math.max(-1, Math.min(1, L[i])) * 32767, true);
    data.setInt16(46 + i * 4, Math.max(-1, Math.min(1, R[i])) * 32767, true);
  }
  // Native blob download — never haul hundreds of MB through evaluate.
  const blob = new Blob([data.buffer], { type: 'audio/wav' });
  const a = document.createElement('a');
  a.href = URL.createObjectURL(blob);
  a.download = 'soundtrack.wav';
  document.body.appendChild(a);
  a.click();
  return data.byteLength;
}, { events: log.sfx, durMs });
console.log('rendered bytes:', wav, 'duration ms:', durMs);
const download = await dl;
await download.saveAs(`${OUT}/soundtrack.wav`);
await b.close();
console.log('saved soundtrack.wav');
