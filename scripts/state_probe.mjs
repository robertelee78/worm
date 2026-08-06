import { chromium } from 'playwright';
const b = await chromium.launch({ args: ['--host-resolver-rules=MAP worm.robertgpt.ai 127.0.0.1'] });
const ctx = await b.newContext({ viewport: { width: 1200, height: 900 } });
const p = await ctx.newPage();
const logs = [];
p.on('console', m => logs.push(`[console.${m.type()}] ${m.text().slice(0,200)}`));
p.on('pageerror', e => logs.push(`[pageerror] ${e.message}`));

// Visit once so the DB exists, then poison it with early-visitor state:
// pre-ghost round records (7-model era, no replay field) and a garbage brain.
await p.goto('https://worm.robertgpt.ai/', { waitUntil: 'load' });
await p.waitForTimeout(1500);
await p.evaluate(async () => {
  const db = await new Promise((res, rej) => {
    const rq = indexedDB.open('worm_brain_db', 3);
    rq.onsuccess = () => res(rq.result); rq.onerror = () => rej(rq.error);
  });
  const dev = await new Promise((res) => {
    const rq = db.transaction('meta','readonly').objectStore('meta').get('profileId');
    rq.onsuccess = () => res(rq.result); rq.onerror = () => res(null);
  });
  const tx = db.transaction(['rounds','brains'], 'readwrite');
  const rounds = tx.objectStore('rounds');
  const names = ['rep','pat','frq','due','wlR','wlL','knn'];
  for (let i = 0; i < 12; i++) {
    rounds.put({
      schemaVersion: 1, id: `${dev}:legacy:${i}`, deviceId: dev,
      endedAt: Date.now() - 86400000 + i, winner: i % 2, cause: 'hit the wall',
      frames: 300 + i, foodEaten: [3, 2],
      accuracy: { rate: 0.5, samples: 40, hits: 20 },
      decisionSourceKey: 'knn', decisionSourceName: 'Deep memory',
      decisionReason: 'following the survival floor', decisionHeading: 2,
      memoryDelta: 5,
      models: names.map(k => ({ key: k, name: k, rawScore: 0.1, effectiveScore: 0.1, hits: 3, samples: 9 })),
      // no replay field — pre-v9 records had none
    });
  }
  // A brain an old build wrote: WRM2 magic followed by truncated garbage.
  const bytes = new Uint8Array(64); bytes.set([0x57,0x52,0x4d,0x32, 2,0, 9,0]);
  tx.objectStore('brains').put(bytes, dev);
  await new Promise((res) => { tx.oncomplete = res; });
  return dev;
});
// Reload — this is the early visitor coming back today.
await p.reload({ waitUntil: 'load' });
await p.waitForTimeout(5000);
const state = await p.evaluate(() => ({
  mid: document.getElementById('mid-status')?.textContent,
  brain: document.getElementById('brain-status')?.textContent,
  frameAdvancing: (() => { const t = document.getElementById('mid-status')?.textContent; return t; })(),
  historyRows: document.querySelectorAll('#history-body tr').length,
}));
await p.waitForTimeout(1500);
const state2 = await p.evaluate(() => document.getElementById('mid-status')?.textContent);
logs.push('STATE ' + JSON.stringify(state));
logs.push('STATE2 ' + JSON.stringify(state2));
console.log(logs.join('\n'));
await b.close();
