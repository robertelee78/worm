// Responsive arena sizing. Logical cells are chosen once when WasmGame is
// constructed; CSS scales that stable board continuously as its grid column
// changes, so browser resizes never reset an active round.

export const BRAIN_PANEL_WIDTH = 310;
export const LAYOUT_GAP = 18;
export const SIDE_BY_SIDE_MIN = 980;

const PAGE_INLINE_GUTTER = 20;
const BEZEL_INLINE_PADDING = 36;
const MAX_ARENA_WIDTH = 1800;
const MIN_CELL = 8;
const MAX_CELL = 16;
const MIN_COLS = 70;
const MAX_COLS = 170;
const MIN_ROWS = 28;
const MAX_ROWS = 58;

function clamp(value, min, max) {
  return Math.max(min, Math.min(max, value));
}

/**
 * Choose the logical board for a newly-created game.
 *
 * The returned `availableWidth` is the physical content width left for the
 * arena after page gutters, bezel padding, and (on wide screens) the CPU Brain
 * column. `displayScale` documents when CSS must shrink the minimum logical
 * board on a narrow phone; it never changes game state.
 */
export function computeBoardLayout(viewportWidth, viewportHeight) {
  const width = Math.max(320, Number(viewportWidth) || 0);
  const height = Math.max(480, Number(viewportHeight) || 0);
  const sideBySide = width >= SIDE_BY_SIDE_MIN;
  const panelReserve = sideBySide ? BRAIN_PANEL_WIDTH + LAYOUT_GAP : 0;
  const availableWidth = Math.max(
    280,
    Math.min(width - PAGE_INLINE_GUTTER - BEZEL_INLINE_PADDING - panelReserve, MAX_ARENA_WIDTH),
  );
  const availableHeight = Math.max(height - 240, 320);
  const cell = clamp(
    Math.floor(Math.min(availableWidth / 110, availableHeight / 44)),
    MIN_CELL,
    MAX_CELL,
  );
  const cols = clamp(Math.floor(availableWidth / cell), MIN_COLS, MAX_COLS);
  const rows = clamp(Math.floor(availableHeight / cell), MIN_ROWS, MAX_ROWS);
  const naturalWidth = cols * cell;
  const naturalHeight = rows * cell;

  return {
    cell,
    cols,
    rows,
    sideBySide,
    availableWidth,
    availableHeight,
    naturalWidth,
    naturalHeight,
    displayScale: Math.min(1, availableWidth / naturalWidth),
  };
}
