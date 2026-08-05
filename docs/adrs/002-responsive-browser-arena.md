# ADR-002: Responsive Browser Arena

## Status
Accepted

## Date
2026-08-04

## Context
The browser frontend chose a logical board from `window.innerWidth` and
`window.innerHeight`, then rendered the canvas at a fixed CSS size. That sizing
ignored the 310px CPU Brain panel and relied on a 70-column, 10px-cell minimum.

A viewport spike showed that the arena, bezel, gap, and brain panel exceeded the
available width by 256-316px at representative desktop sizes (1920x1080 through
768x1024). On a 390px-wide phone, the minimum arena was about 736px wide. Flex
wrapping moved the brain below the arena on desktop, while the fixed-size canvas
could overflow narrow screens.

An active game cannot be reconstructed on every browser resize: doing so would
discard the current round or require migrating live board state. Logical board
size and displayed canvas size therefore have different lifecycles.

## Decision

1. Select the logical board once at game construction, reserving the CPU Brain
   panel width when the viewport can support a side-by-side layout.
2. Lay out the arena and brain with a two-column CSS grid on wide screens and a
   one-column stack on narrow screens.
3. Derive the screen's CSS `aspect-ratio` from the logical board and make the
   canvas and CRT overlays fill that screen.
4. Let CSS continuously scale the screen with its grid column as the browser is
   resized. Resizing changes presentation only; it never restarts `WasmGame` or
   mutates the active board.
5. Keep explicit minimum logical dimensions for playability, but decouple those
   cells from fixed CSS pixels so phones can display the complete arena without
   horizontal overflow.

## Spike/Test Contract

- 1920x1080, 1440x900, 1366x768, and 1024x768 keep the brain beside the arena
  whenever the minimum useful arena width is available.
- 768x1024 and 390x844 stack the brain below a full-width arena.
- The rendered arena never exceeds its layout column.
- The arena retains its logical aspect ratio at every tested size.
- A resize does not construct, restart, or reset the game.

## Consequences

- Positive: the complete arena remains visible on desktop, tablet, phone, and
  browser resize without losing match state.
- Positive: the CPU Brain remains a true side panel when space permits instead
  of being wrapped below an oversized canvas.
- Positive: gameplay dimensions remain stable for the duration of a round.
- Tradeoff: resizing after boot scales existing cells rather than adding or
  removing logical cells. New logical dimensions take effect on the next page
  load, avoiding destructive mid-round migration.
