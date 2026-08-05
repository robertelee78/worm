#!/usr/bin/env bash
# Real-Chrome proof for no-scroll first-load stage focus, responsive layout,
# round-boundary logical resizing, and IndexedDB reload. Requires vibium.
set -euo pipefail

repo_root=$(cd "$(dirname "$0")/../.." && pwd)
test_port=${WORM_BROWSER_TEST_PORT:-18765}
test_tmp=$(mktemp -d)
server_pid=""
started_daemon=0
started_browser=0

cleanup() {
  if [[ -n "$server_pid" ]] && kill -0 "$server_pid" 2>/dev/null; then
    kill "$server_pid"
    wait "$server_pid" 2>/dev/null || true
  fi
  if [[ "$started_browser" -eq 1 ]]; then
    vibium stop >/dev/null 2>&1 || true
  fi
  if [[ "$started_daemon" -eq 1 ]]; then
    vibium daemon stop >/dev/null 2>&1 || true
  fi
  rm -rf "$test_tmp"
}
trap cleanup EXIT

command -v vibium >/dev/null
command -v python3 >/dev/null
command -v curl >/dev/null
vibium is-installed >/dev/null

python3 -m http.server "$test_port" --directory "$repo_root/web" \
  >"$test_tmp/server.log" 2>&1 &
server_pid=$!
for _ in {1..50}; do
  if curl --fail --silent "http://127.0.0.1:$test_port/" >/dev/null; then
    break
  fi
  if ! kill -0 "$server_pid" 2>/dev/null; then
    sed -n '1,120p' "$test_tmp/server.log"
    exit 1
  fi
  sleep 0.1
done
curl --fail --silent "http://127.0.0.1:$test_port/" >/dev/null

if vibium daemon status >/dev/null 2>&1; then
  if ! vibium start >/dev/null 2>&1; then
    # A daemon can outlive its ChromeDriver socket while status still reports
    # healthy. Recover locally so the release gate is not order-dependent.
    vibium daemon stop >/dev/null 2>&1 || true
    vibium daemon start --headless >/dev/null
    started_daemon=1
    vibium start >/dev/null
  fi
else
  vibium daemon start --headless >/dev/null
  started_daemon=1
  vibium start >/dev/null
fi
started_browser=1
vibium viewport 1440 900 >/dev/null
vibium go "http://127.0.0.1:$test_port" >/dev/null
vibium wait "#game-canvas" --state visible --timeout 30000 >/dev/null
vibium wait fn \
  "document.querySelectorAll('.bp-model').length === 7 && document.getElementById('history-summary').textContent.length > 0" \
  >/dev/null

# Start a fresh page lifecycle at scrollY=0. There is deliberately no test
# scroll after reload: the application must focus the complete playable stage.
vibium eval "window.scrollTo(0, 0)" >/dev/null
vibium reload >/dev/null
vibium wait fn \
  "document.getElementById('play-column').dataset.autoFocused === 'true'" \
  >/dev/null
vibium eval --stdin >/dev/null <<'JS'
(() => {
  const viewport = window.visualViewport || { height: innerHeight, offsetTop: 0 };
  const stage = document.getElementById('play-column').getBoundingClientRect();
  const screen = document.getElementById('screen').getBoundingClientRect();
  const top = viewport.offsetTop || 0, bottom = top + viewport.height;
  if (stage.top < top - 1 || stage.bottom > bottom + 1) {
    throw new Error(`1440x900 stage is not fully visible: ${stage.top}..${stage.bottom} vs ${top}..${bottom}`);
  }
  if (screen.top < top - 1 || screen.bottom > bottom + 1) throw new Error('1440x900 screen is clipped');
  if (Math.abs((stage.top + stage.bottom) / 2 - (top + bottom) / 2) > 3) {
    throw new Error('1440x900 stage is not vertically centered');
  }
  if (scrollY <= 0) throw new Error('application did not move the below-fold stage into view');
  return true;
})()
JS

# Reproduce the narrow effective content viewport from the Safari acceptance
# capture. Again, reset before reload and issue no test scroll afterward.
vibium viewport 768 818 >/dev/null
vibium eval "window.scrollTo(0, 0)" >/dev/null
vibium reload >/dev/null
vibium wait fn \
  "document.getElementById('play-column').dataset.autoFocused === 'true'" \
  >/dev/null
vibium eval --stdin >/dev/null <<'JS'
(() => {
  const viewport = window.visualViewport || { height: innerHeight, offsetTop: 0 };
  const stage = document.getElementById('play-column').getBoundingClientRect();
  const screen = document.getElementById('screen').getBoundingClientRect();
  const top = viewport.offsetTop || 0, bottom = top + viewport.height;
  if (stage.top < top - 1 || stage.bottom > bottom + 1) {
    throw new Error(`768x818 stage is not fully visible: ${stage.top}..${stage.bottom} vs ${top}..${bottom}`);
  }
  if (screen.left < -1 || screen.right > document.documentElement.clientWidth + 1) {
    throw new Error('768x818 screen exceeds the effective content viewport');
  }
  if (Math.abs((stage.top + stage.bottom) / 2 - (top + bottom) / 2) > 3) {
    throw new Error('768x818 stage is not vertically centered');
  }
  if (scrollY <= 0) throw new Error('application did not auto-focus the compact stage');
  return true;
})()
JS

vibium viewport 2560 1440 >/dev/null
vibium reload >/dev/null
vibium wait fn \
  "document.getElementById('play-column').dataset.autoFocused === 'true'" \
  >/dev/null

vibium eval --stdin >/dev/null <<'JS'
(() => {
  const canvas = document.getElementById('game-canvas');
  window.__wormGateCanvas = [canvas.width, canvas.height];
  if (document.documentElement.scrollWidth > innerWidth) throw new Error('desktop horizontal overflow');
  if (document.querySelectorAll('.bp-model').length !== 7) throw new Error('missing model rows');
  if (document.querySelector('.game-row').getBoundingClientRect().width < document.documentElement.clientWidth - 24) {
    throw new Error('arena row is still capped on a large display');
  }
  const physicalCell = document.getElementById('screen').getBoundingClientRect().width / Number(canvas.dataset.cols);
  if (physicalCell < 30) throw new Error(`large-screen cells too small: ${physicalCell}px`);
  return true;
})()
JS

# A short Safari-like visual viewport refits the current presentation without
# rebuilding the logical board and keeps an already-visible stage in view.
vibium viewport 1440 520 >/dev/null
vibium wait fn \
  "document.getElementById('play-column').getBoundingClientRect().bottom <= (window.visualViewport?.height || innerHeight) + (window.visualViewport?.offsetTop || 0) + 1" \
  >/dev/null
vibium eval --stdin >/dev/null <<'JS'
(() => {
  const canvas = document.getElementById('game-canvas');
  const stage = document.getElementById('play-column').getBoundingClientRect();
  const viewport = window.visualViewport || { height: innerHeight, offsetTop: 0 };
  if (canvas.width !== window.__wormGateCanvas[0] || canvas.height !== window.__wormGateCanvas[1]) {
    throw new Error('short-height refit changed the active logical board');
  }
  if (stage.top < (viewport.offsetTop || 0) - 1 ||
      stage.bottom > (viewport.offsetTop || 0) + viewport.height + 1) {
    throw new Error('short-height refit left the playable stage clipped');
  }
  return true;
})()
JS

# CSS responds immediately, but the running engine retains its logical board.
vibium viewport 390 844 >/dev/null
vibium eval --stdin >/dev/null <<'JS'
(() => {
  const canvas = document.getElementById('game-canvas');
  if (canvas.width !== window.__wormGateCanvas[0] || canvas.height !== window.__wormGateCanvas[1]) {
    throw new Error('active viewport resize changed logical board');
  }
  if (document.documentElement.scrollWidth > innerWidth) throw new Error('phone horizontal overflow');
  if (document.getElementById('brain-panel').getBoundingClientRect().width > innerWidth) {
    throw new Error('brain panel exceeds phone viewport');
  }
  if (document.querySelector('.bp-mname').getBoundingClientRect().width < 100) {
    throw new Error('model names collapsed');
  }
  return true;
})()
JS

# Force the coarse-pointer controls visible and prove they are part of the
# measured stage budget rather than appearing below the viewport.
vibium eval --stdin >/dev/null <<'JS'
(() => {
  document.getElementById('touch-controls').style.display = 'block';
  dispatchEvent(new Event('resize'));
  return true;
})()
JS
vibium wait fn \
  "document.getElementById('play-column').getBoundingClientRect().bottom <= (window.visualViewport?.height || innerHeight) + (window.visualViewport?.offsetTop || 0) + 1" \
  >/dev/null
vibium eval --stdin >/dev/null <<'JS'
(() => {
  const stage = document.getElementById('play-column').getBoundingClientRect();
  const controls = document.getElementById('touch-controls').getBoundingClientRect();
  if (controls.height < 100) throw new Error('forced touch controls did not become visible');
  if (controls.top < stage.top || controls.bottom > stage.bottom + 1) {
    throw new Error('touch controls are not contained in the fitted play stage');
  }
  return true;
})()
JS

# The next round adopts the phone's available logical dimensions exactly once.
vibium keys ArrowUp >/dev/null
vibium wait "#over-overlay" --state visible --timeout 45000 >/dev/null
vibium keys Enter >/dev/null
vibium wait "#over-overlay" --state hidden --timeout 5000 >/dev/null
vibium eval --stdin >/dev/null <<'JS'
(() => {
  const canvas = document.getElementById('game-canvas');
  const cols = Number(canvas.dataset.cols), rows = Number(canvas.dataset.rows), cell = Number(canvas.dataset.cell);
  window.__wormPhoneCanvas = [canvas.width, canvas.height];
  if (cols < 30 || cols > 40) throw new Error(`phone logical width should preserve readable cells, got ${cols}`);
  if (canvas.width !== cols * cell || canvas.height !== rows * cell) {
    throw new Error('phone backing canvas does not match its logical grid');
  }
  if (document.getElementById('screen').style.aspectRatio !== `${cols} / ${rows}`) {
    throw new Error('screen aspect ratio did not follow logical board');
  }
  const screenWidth = document.getElementById('screen').getBoundingClientRect().width;
  if (screenWidth < document.documentElement.clientWidth - 42) throw new Error('phone arena is not full width');
  const physicalCell = screenWidth / cols;
  if (physicalCell < 10) throw new Error(`phone cells too small after round boundary: ${physicalCell}px`);
  return true;
})()
JS

# Prove the exact stack/side-by-side breakpoint without changing game state.
vibium viewport 1239 800 >/dev/null
vibium eval --stdin >/dev/null <<'JS'
(() => {
  const columns = getComputedStyle(document.querySelector('.game-row')).gridTemplateColumns.split(' ');
  if (columns.length !== 1) throw new Error('1239px should stack the brain panel');
  if (document.documentElement.scrollWidth > innerWidth) throw new Error('1239px horizontal overflow');
  return true;
})()
JS
vibium viewport 1240 800 >/dev/null
vibium eval --stdin >/dev/null <<'JS'
(() => {
  const columns = getComputedStyle(document.querySelector('.game-row')).gridTemplateColumns.split(' ');
  if (columns.length !== 2) throw new Error('1240px should use two columns');
  if (document.documentElement.scrollWidth > innerWidth) throw new Error('1240px horizontal overflow');
  const canvas = document.getElementById('game-canvas');
  if (canvas.width !== window.__wormPhoneCanvas[0] || canvas.height !== window.__wormPhoneCanvas[1]) {
    throw new Error('active breakpoint resize reset board');
  }
  return true;
})()
JS

# Finish another round, then prove its durable evidence survives page reload.
vibium keys ArrowUp >/dev/null
vibium wait "#over-overlay" --state visible --timeout 45000 >/dev/null
vibium eval --stdin >/dev/null <<'JS'
(() => {
  const rows = document.querySelectorAll('#history-body tr').length;
  if (rows < 1) throw new Error('no persisted history rows');
  localStorage.setItem('__worm_browser_gate_rows', String(rows));
  return true;
})()
JS
vibium reload >/dev/null
vibium wait "#game-canvas" --state visible --timeout 30000 >/dev/null
vibium wait fn \
  "document.getElementById('history-summary').textContent.includes('saved rounds')" \
  >/dev/null
vibium eval --stdin >/dev/null <<'JS'
(() => {
  const expected = Number(localStorage.getItem('__worm_browser_gate_rows'));
  const actual = document.querySelectorAll('#history-body tr').length;
  localStorage.removeItem('__worm_browser_gate_rows');
  if (actual !== expected) throw new Error(`history reload lost rows: ${expected} -> ${actual}`);
  if (!document.getElementById('brain-status').textContent.includes('restored')) {
    throw new Error('brain corpus was not restored after reload');
  }
  if (document.documentElement.scrollWidth > innerWidth) throw new Error('reload horizontal overflow');
  return true;
})()
JS

echo "BROWSER PASS — no-scroll stage focus, live refit, boundary resize, breakpoint, and durable evidence held"
