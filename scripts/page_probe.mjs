import { chromium } from 'playwright';
const b = await chromium.launch({ args: ['--host-resolver-rules=MAP worm.robertgpt.ai 127.0.0.1'] });
const ctx = await b.newContext({
  viewport: { width: 390, height: 844 },
  hasTouch: true, isMobile: true,
  userAgent: 'Mozilla/5.0 (iPhone; CPU iPhone OS 18_7 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.5.2 Mobile/15E148 Safari/604.1',
});
const p = await ctx.newPage();
const logs = [];
p.on('console', m => logs.push(`[console.${m.type()}] ${m.text()}`));
p.on('pageerror', e => logs.push(`[pageerror] ${e.message}`));
p.on('requestfailed', r => logs.push(`[reqfail] ${r.url()} ${r.failure()?.errorText}`));
await p.goto('https://worm.robertgpt.ai/', { waitUntil: 'load', timeout: 30000 });
await p.waitForTimeout(6000);
const state = await p.evaluate(() => ({
  midStatus: document.getElementById('mid-status')?.textContent,
  brainStatus: document.getElementById('bp-status')?.textContent ?? document.querySelector('.bp-warm')?.textContent,
  canvasW: document.getElementById('game-canvas')?.width,
  canvasH: document.getElementById('game-canvas')?.height,
  touchControlsVisible: getComputedStyle(document.querySelector('.touch-controls') || document.body).display,
}));
logs.push('STATE ' + JSON.stringify(state));
await p.screenshot({ path: 'chromium_iphone.png', fullPage: false });
console.log(logs.join('\n'));
await b.close();
