// Pure responsive-layout contract. Run with: node web/tests/layout-test.mjs
import assert from 'node:assert/strict';
import { computeBoardLayout, SIDE_BY_SIDE_MIN } from '../layout.js';

const desktopCases = [
  [1920, 1080],
  [1440, 900],
  [1366, 768],
  [1024, 768],
];

for (const [width, height] of desktopCases) {
  const layout = computeBoardLayout(width, height);
  assert.equal(layout.sideBySide, true, `${width}x${height} keeps the brain beside the arena`);
  assert.ok(layout.naturalWidth <= layout.availableWidth, `${width}x${height} logical arena fits its column`);
  assert.ok(layout.cols >= 70 && layout.rows >= 28, `${width}x${height} retains playable logical bounds`);
  assert.equal(layout.displayScale, 1, `${width}x${height} needs no emergency CSS shrink`);
}

for (const [width, height] of [[768, 1024], [390, 844]]) {
  const layout = computeBoardLayout(width, height);
  assert.equal(layout.sideBySide, false, `${width}x${height} stacks the brain below the arena`);
  assert.ok(layout.availableWidth <= width, `${width}x${height} arena budget stays within viewport`);
  assert.ok(layout.displayScale > 0 && layout.displayScale <= 1, `${width}x${height} has a valid visual scale`);
}

assert.equal(computeBoardLayout(SIDE_BY_SIDE_MIN - 1, 800).sideBySide, false, 'breakpoint - 1 stacks');
assert.equal(computeBoardLayout(SIDE_BY_SIDE_MIN, 800).sideBySide, true, 'breakpoint uses side-by-side grid');

const before = computeBoardLayout(1440, 900);
const after = computeBoardLayout(1024, 768);
assert.notDeepEqual(before, after, 'a new page load adapts logical dimensions to its viewport');

console.log('LAYOUT PASS — desktop, tablet, phone, and breakpoint contracts held');
