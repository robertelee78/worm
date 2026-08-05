# ADR-002: Responsive Browser Arena

## Status
Implemented

## Date
2026-08-04

## Updated
2026-08-05

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

The first implementation removed overflow but failed its player acceptance
test: it targeted roughly 110 columns, producing 9px cells at 1440x900 and
effectively 4.8px cells on a 390px phone. A 1900px row cap also left most of an
ultrawide display unused. Fitting the board was not enough; the cycles and items
had to remain readable while the arena consumed the space actually available.

A second acceptance test exposed a different failure in Safari: the sizing
formula subtracted a fixed 240px from viewport height, even though the title,
hero, and scoreboard consumed 532px before the cabinet at 1440x900. With the
page at its restored top position, the screen occupied y=564..1219 and required
manual scrolling. A captured browser session also exposed an effective 768px
content viewport inside a much wider browser capture. Outer-window estimates
therefore cannot establish the space the stage actually owns.

## Decision

1. Select the logical board once at game construction from the play column's
   measured content width and the live visual viewport's measured height.
2. Lay out the arena and brain with a two-column CSS grid on wide screens and a
   one-column stack on narrow screens.
3. Derive the screen's CSS `aspect-ratio` from the logical board and make the
   canvas and CRT overlays fill that screen.
4. Let CSS continuously scale the screen with its grid column as the browser is
   resized. Resizing changes presentation only; it never restarts `WasmGame` or
   mutates the active board.
5. Derive logical dimensions from a readable physical-cell target (10-32px),
   reducing the number of cells on constrained screens before shrinking game
   objects. Do not cap arena width on large screens.
6. Recompute logical dimensions when a new round or new match begins. Apply the
   new fixed dimensions through the existing `WormGame` instance so the brain
   and match scoreboard survive; never migrate or reset an active round.
7. Stack the brain below the arena below 1240px so the side card cannot make the
   playfield and its contents tiny on ordinary laptop widths.
8. Fit the active screen from the actual play-column width, computed bezel
   chrome, visible touch controls, and `visualViewport` height. Window and
   `visualViewport` resize events change presentation only.
9. Keep touch controls in the play column, then center that complete playable
   stage on boot, next round, and new match. Decorative content remains
   available above it, but the player never has to scroll manually to begin.
10. Reserve hero geometry in HTML so late image decoding cannot move the stage
    after it has been centered.

## Spike/Test Contract

- 3840x2160, 2560x1440, 1920x1080, 1440x900, and 1366x768 keep the brain beside
  the arena; 1024x768, 768x1024, and 390x844 stack it below.
- The rendered arena never exceeds its layout column.
- Effective cells remain at least 16px in the tested desktop layouts and 10px
  on constrained layouts; large displays are not trapped in a fixed-width row.
- The arena retains its logical aspect ratio at every tested size.
- A resize does not construct, restart, or reset the game.
- A browser resize takes new logical dimensions on the next round boundary,
  preserving the learned brain and, for a normal restart, the match score.
- A real browser test verifies bounding boxes and horizontal overflow at the
  representative viewport sizes instead of relying only on formula tests.
- From an explicit `scrollY=0`, a fresh 1440x900 and 768x818 load automatically
  places the complete playable stage inside the visual viewport without a test
  scroll command.
- A short-height live resize refits the CSS presentation, keeps the stage
  visible, and leaves the active canvas's logical dimensions unchanged.

## Consequences

- Positive: the complete arena remains visible on desktop, tablet, phone, and
  browser resize without losing match state.
- Positive: the CPU Brain remains a true side panel when space permits instead
  of being wrapped below an oversized canvas.
- Positive: gameplay dimensions remain stable for the duration of a round.
- Tradeoff: resizing during a live round scales existing cells rather than
  adding or removing logical cells. New logical dimensions take effect at the
  next round boundary, avoiding destructive mid-round migration.

## Proof (2026-08-05)

- Formula tests cover desktop, tablet, phone, ultrawide, and the exact
  1239/1240px layout
  breakpoint.
- A committed real-Chrome/Vibium gate proves no horizontal overflow, full model
  names on a 390x844 phone, a stable logical board during active resizes, a
  full-width 2560px arena with 30px+ cells, and a one-time roughly 34-column
  logical resize with 10px+ rendered cells on the following phone-sized round.
- The gate now starts fresh 1440x900 and 768x818 page lifecycles at `scrollY=0`
  and issues no test-side scroll after reload. In both cases the app centers the
  complete play stage inside `visualViewport`; a 1440x520 live-height spike
  refits the stage while retaining the exact active canvas dimensions.
- Visual captures confirm a 1041x829 screen plus brain panel at 1440x900 and a
  centered, nearly full-viewport arena at the screenshot-like 768x818 size.
- A Rust integration test proves `restart_with_size` preserves the learned
  brain, banks the current winner once, and resets only round-owned state.
