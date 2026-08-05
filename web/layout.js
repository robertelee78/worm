// Responsive arena sizing. DOM-measured presentation scales the stable active
// board continuously; this logical calculation runs only at boot or a
// round/new-match boundary, so a live resize never destroys an active round.

export const BRAIN_PANEL_WIDTH = 310;
export const LAYOUT_GAP = 18;
export const SIDE_BY_SIDE_MIN = 1240;
export const VIEWPORT_BLOCK_GUTTER = 32;

const PAGE_INLINE_GUTTER = 20;
const WIDE_BEZEL_INLINE_PADDING = 36;
const MIN_CELL = 10;
const MAX_CELL = 32;
const TARGET_COLS = 54;
const TARGET_ROWS = 34;
const MIN_COLS = 30;
const MAX_COLS = 180;
const MIN_ROWS = 24;
const MAX_ROWS = 90;

function clamp(value, min, max) {
  return Math.max(min, Math.min(max, value));
}

/**
 * Choose the logical board for a newly-created game or round boundary.
 *
 * The returned dimensions fill the physical arena budget while keeping a cell
 * near a readable size. Wide displays gain board area instead of stopping at a
 * fixed page cap; narrow displays reduce the logical grid before shrinking the
 * cycles into near-invisible pixels.
 */
export function computeBoardLayout(viewportWidth, viewportHeight, measured = {}) {
  const width = Math.max(320, Number(viewportWidth) || 0);
  const height = Math.max(240, Number(viewportHeight) || 0);
  const sideBySide = width >= SIDE_BY_SIDE_MIN;
  const panelReserve = sideBySide ? BRAIN_PANEL_WIDTH + LAYOUT_GAP : 0;
  const bezelInlinePadding = sideBySide
    ? WIDE_BEZEL_INLINE_PADDING
    : clamp(width * 0.05, 16, WIDE_BEZEL_INLINE_PADDING);
  const fallbackWidth = Math.max(
    270,
    width - PAGE_INLINE_GUTTER - bezelInlinePadding - panelReserve,
  );
  const fallbackHeight = Math.max(height - VIEWPORT_BLOCK_GUTTER, 240);
  const availableWidth = Math.max(270, Number(measured.availableWidth) || fallbackWidth);
  const availableHeight = Math.max(240, Number(measured.availableHeight) || fallbackHeight);
  const cell = clamp(
    Math.round(Math.min(availableWidth / TARGET_COLS, availableHeight / TARGET_ROWS)),
    MIN_CELL,
    MAX_CELL,
  );
  const cols = clamp(Math.floor(availableWidth / cell), MIN_COLS, MAX_COLS);
  const physicalCell = availableWidth / cols;
  const rows = clamp(Math.floor(availableHeight / physicalCell), MIN_ROWS, MAX_ROWS);
  const naturalWidth = cols * cell;
  const naturalHeight = rows * cell;

  return {
    cell,
    cols,
    rows,
    sideBySide,
    availableWidth,
    availableHeight,
    physicalCell,
    naturalWidth,
    naturalHeight,
    displayScale: Math.min(1, availableWidth / naturalWidth, availableHeight / naturalHeight),
  };
}
