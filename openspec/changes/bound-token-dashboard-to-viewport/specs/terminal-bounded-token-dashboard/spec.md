## ADDED Requirements

### Requirement: Token dashboards are bounded by one captured viewport

The CLI SHALL select token dashboard loading, layout, buffer dimensions, and serialization from one validated terminal viewport captured for the invocation. The serialized snapshot SHALL NOT exceed the selected non-zero viewport in logical rows or display cells.

#### Scenario: Compact overview fits a narrow and short viewport

- **WHEN** the token overview is requested with fewer than 80 columns or fewer than 50 rows
- **THEN** ProjectAtlas SHALL render the compact overview into a buffer no larger than that viewport
- **AND** the ANSI snapshot SHALL NOT contain a logical row wider than the viewport or more logical rows than the viewport
- **AND** rendering SHALL NOT panic.

#### Scenario: Compact trend fits below its full boundary

- **WHEN** the token trend is requested with fewer than 80 columns or fewer than 30 rows
- **THEN** ProjectAtlas SHALL render the bounded compact trend rather than an over-constrained full trend.

#### Scenario: Full layouts remain compatible at their boundaries

- **WHEN** the captured viewport is at least 80 by 50 for overview or 80 by 30 for trend
- **THEN** ProjectAtlas SHALL retain the accepted full layout, semantic styles, signed arithmetic, and supported maximum width
- **AND** a sufficiently wide overview SHALL load and render the Atlas preview from that same captured viewport decision.

### Requirement: Compact output preserves facts by explicit priority

Compact token snapshots SHALL preserve the title and signed primary value first, followed as space permits by calculation, file-read composition, token-source composition, lookup metadata, estimate/confidence, and version. They SHALL use the existing typed token reports and semantic styles without changing token calculations.

#### Scenario: Negative savings remain truthful

- **WHEN** a compact overview or trend contains a negative saving
- **THEN** the rendered primary value SHALL retain its negative sign
- **AND** it SHALL NOT display a success marker that implies positive savings.

#### Scenario: Viewport is mathematically too small for every fact

- **WHEN** the selected non-zero viewport cannot contain every prioritized compact row
- **THEN** ProjectAtlas SHALL omit lower-priority rows from the bottom
- **AND** SHALL remain bounded and non-panicking rather than widening, scrolling earlier rows away, or inventing a second output format.

### Requirement: Viewport fallbacks are validated and deterministic

A live terminal measurement SHALL be authoritative when available. Valid non-zero environment dimensions MAY provide deterministic non-TTY and test fallback values. Invalid or zero dimensions SHALL NOT construct a zero-sized backend or force output beyond a live terminal.

#### Scenario: Stale environment dimensions exceed a live terminal

- **WHEN** environment dimensions are larger than a valid live terminal viewport
- **THEN** the live viewport SHALL determine the bounded output and graph-preview decision.

#### Scenario: No valid terminal or environment dimensions are available

- **WHEN** live measurement fails and environment dimensions are missing, invalid, or zero
- **THEN** ProjectAtlas SHALL use its documented valid fallback viewport
- **AND** render deterministically without panic.

### Requirement: Small-terminal behavior is mandatory release proof

The viewport boundary SHALL be covered by owning unit tests, real CLI subprocess tests, required cross-platform CI, and installed-candidate release verification. Human terminal review SHALL verify visual truth where cell assertions cannot prove emulator wrapping, scrolling, or prompt behavior.

#### Scenario: Required verification would skip terminal boundary tests

- **WHEN** a required CI or release-candidate path is configured
- **THEN** it SHALL run the named token terminal-size contract rather than relying only on full-size source snapshots.

#### Scenario: Real terminal acceptance

- **WHEN** RC2 is evaluated in Windows ConPTY and representative Linux and macOS terminals at the documented boundaries
- **THEN** the visible snapshot SHALL show no wrapped right edge, lost top content, false empty Atlas panel, panel collision, or prompt collision attributable to output exceeding the viewport.
