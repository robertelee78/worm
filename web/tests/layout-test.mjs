// Pure responsive-layout contract. Run with: node web/tests/layout-test.mjs
import assert from 'node:assert/strict';
import { computeBoardLayout, SIDE_BY_SIDE_MIN, VIEWPORT_BLOCK_GUTTER } from '../layout.js';

const desktopCases = [
  [3840, 2160],
  [2560, 1440],
  [1920, 1080],
  [1440, 900],
  [1366, 768],
];

for (const [width, height] of desktopCases) {
  const layout = computeBoardLayout(width, height);
  assert.equal(layout.sideBySide, true, `${width}x${height} keeps the brain beside the arena`);
  assert.ok(layout.naturalWidth <= layout.availableWidth, `${width}x${height} logical arena fits its column`);
  assert.ok(layout.cols >= 30 && layout.rows >= 24, `${width}x${height} retains playable logical bounds`);
  assert.ok(layout.physicalCell >= 16, `${width}x${height} keeps cycles readable`);
  assert.equal(layout.displayScale, 1, `${width}x${height} needs no emergency CSS shrink`);
}

for (const [width, height] of [[1024, 768], [768, 1024], [390, 844]]) {
  const layout = computeBoardLayout(width, height);
  assert.equal(layout.sideBySide, false, `${width}x${height} stacks the brain below the arena`);
  assert.ok(layout.availableWidth <= width, `${width}x${height} arena budget stays within viewport`);
  assert.ok(layout.physicalCell >= 10, `${width}x${height} keeps cycles readable`);
  assert.ok(layout.displayScale > 0 && layout.displayScale <= 1, `${width}x${height} has a valid visual scale`);
}

const ultraWide = computeBoardLayout(3840, 2160);
assert.ok(ultraWide.availableWidth > 3000, 'large screens are not trapped in the old 1900px page cap');

assert.equal(computeBoardLayout(SIDE_BY_SIDE_MIN - 1, 800).sideBySide, false, 'breakpoint - 1 stacks');
assert.equal(computeBoardLayout(SIDE_BY_SIDE_MIN, 800).sideBySide, true, 'breakpoint uses side-by-side grid');

const before = computeBoardLayout(1440, 900);
const after = computeBoardLayout(1024, 768);
assert.notDeepEqual(before, after, 'a new page load or round boundary adapts logical dimensions to its viewport');

const measuredTall = computeBoardLayout(768, 818, {
  availableWidth: 712,
  availableHeight: 750,
});
assert.equal(measuredTall.availableWidth, 712, 'the resolved play-column width overrides outer-window estimates');
assert.equal(measuredTall.availableHeight, 750, 'the live visual-height budget is used without a fixed header subtraction');
const measuredShort = computeBoardLayout(768, 818, {
  availableWidth: 712,
  availableHeight: 420,
});
assert.ok(measuredShort.rows < measuredTall.rows, 'a short live viewport produces a shorter next-round board');
assert.equal(
  computeBoardLayout(768, 818).availableHeight,
  818 - VIEWPORT_BLOCK_GUTTER,
  'fallback height reserves only stage breathing room, not decorative page chrome',
);

console.log('LAYOUT PASS — measured stage, viewport, phone, and breakpoint contracts held');
