## Context

The overview currently serializes a Ratatui `TestBackend` buffer with a width clamped to 80 through 200 and a fixed height of 50. The trend view uses 80 through 140 columns and 30 rows. Terminal height is not read. The overview also reads terminal width once before optional graph loading and again during rendering.

This makes the output larger than narrow or short viewports and allows the graph-load decision to diverge from the rendered layout. Ratatui can collapse over-constrained areas, so simply shrinking the existing full layout would hide panels without defining which facts survive.

## Goals And Non-Goals

Goals:

- never serialize more cells or logical rows than the selected viewport;
- preserve the accepted full dashboard at supported dimensions;
- retain the most important signed token facts in compact mode;
- make graph loading and rendering one viewport decision;
- prove source and installed-binary behavior on supported platforms.

Non-goals:

- an interactive resize loop or persistent terminal session;
- a new renderer abstraction or dependency;
- arbitrary compression of every full-size panel;
- token calculation, telemetry, or MCP schema changes.

## Decisions

### Capture one concrete viewport

The CLI/TUI boundary captures `(columns, rows)` once. A live terminal size is authoritative. Valid non-zero `COLUMNS` and `LINES` values remain deterministic non-TTY and test fallbacks; the established default is used only when neither live nor valid fallback dimensions exist. Zero and invalid values never reach `TestBackend` construction.

This removes the resize race and gives the graph preview, layout mode, buffer dimensions, and serializer one fact.

### Preserve the full layout and add one compact layout

The full overview remains available at least at 80 by 50; the full trend remains available at least at 80 by 30. Their existing maximum widths and visual contracts stay unchanged. Below either threshold, an existing Ratatui buffer renders a borderless compact `Paragraph` from typed overview or trend data. No parallel terminal-rendering system is introduced.

Compact overview facts are ordered by value:

1. product/title identity;
2. signed average or net saving;
3. without, with, and avoided arithmetic;
4. observed, modeled, and total file reads;
5. measured and modeled token composition;
6. lookup count, estimate/confidence, and version.

Compact trend mode follows the same principle for the recent value and time window. Rows that cannot fit are omitted from the bottom. Width truncation uses Ratatui cell-aware rendering. Extremely small non-zero viewports may show only a clipped title or primary value, but remain bounded and non-panicking.

The existing ANSI serializer emits each rendered grapheme once and advances by Ratatui's `CellWidth`, skipping continuation cells occupied by wide symbols. This keeps CJK, combining, emoji, and ASCII output within the same display-cell bound without introducing a second width library or renderer.

### Skip the Atlas preview outside the fitting full layout

The optional graph preview is loaded only when the captured viewport selects the full overview and meets its wide threshold. Compact mode never loads or displays the graph preview. This prevents false empty-graph messages and avoids bounded but useless SQLite/layout work in a short terminal.

### Keep errors at the CLI boundary

Ratatui backend construction and draw failures propagate through the existing CLI error boundary instead of panicking. No general renderer trait or new error framework is warranted.

## Rust Pattern Fit

A small concrete viewport value and an exhaustive full-or-compact decision are owned by the existing CLI adapter. This uses current typed reports, Ratatui, serializer, and CLI error path. No new crate, trait, generic renderer, async task, worker, shared lock, or dependency is justified.

## Database Implications

None. The compact path consumes the same in-memory token reports. When it cannot show the Atlas preview it performs less SQLite work and creates no new transaction, query, migration, or persisted state.

## Performance And Bounds

Rendering remains `O(columns * rows)` and output is strictly bounded by the selected viewport. The full overview remains capped at 10,000 cells and the full trend at 4,200 cells. Compact buffers are smaller. Capturing the viewport once eliminates a second terminal query and suppresses the bounded graph query and layout when height cannot display them. No allocation optimization beyond those bounds is necessary.

## Failure And Compatibility

- Existing full-size content, colors, arithmetic, maximum widths, and Atlas threshold remain compatible.
- Invalid or zero environment dimensions fall through to a valid live or default viewport.
- A terminal measurement failure produces deterministic bounded non-TTY output rather than a zero-sized backend.
- Negative savings remain signed and never acquire a positive marker in compact mode.
- Output errors remain terminal errors; no partial second render is attempted.

## Verification

Focused unit tests inject viewports without shared environment mutation and assert buffer dimensions, mode selection, signed facts, every theme, wide grapheme serialization, and Atlas suppression. CLI tests exercise `COLUMNS` and `LINES`, CJK display width, rejected stdout writes, and logical row/display-cell bounds. Required platform smoke covers Linux, Windows, and macOS. Release verification repeats the boundary contract against installed candidates. Human terminal review covers Windows ConPTY and representative Linux/macOS terminals at the documented full/compact boundaries.
