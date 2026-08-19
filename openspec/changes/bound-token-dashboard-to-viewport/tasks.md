## 1. Contract And Architecture

- [x] 1.1 Map issue #462 to this change, synchronize its objective checklist, and keep RC2 scope limited to one captured viewport plus a bounded compact fallback.
- [x] 1.2 Update the token telemetry diagram and durable terminal guidance only where the viewport decision changes the flow; render every changed Mermaid block and inspect it visually and semantically.

## 2. Viewport And Compact Rendering

- [x] 2.1 Add one validated columns-and-rows viewport capture with live-terminal authority, deterministic non-TTY fallbacks, zero/invalid handling, and no duplicate measurement.
- [x] 2.2 Select full overview, full trend, compact overview, compact trend, and optional Atlas loading from that captured viewport while preserving supported full-layout behavior.
- [x] 2.3 Render compact overview and trend snapshots through the existing Ratatui buffer and ANSI serializer with priority-ordered signed facts, semantic styles, bounded truncation, and propagated render errors.

## 3. Mandatory Regression And Release Proof

- [x] 3.1 Add deterministic unit tests for viewport resolution, every width/height boundary, buffer bounds, signed negative values, themes, compact fact priority, full-layout compatibility, and Atlas suppression.
- [x] 3.2 Add real CLI subprocess tests for bounded ANSI rows/cells, invalid and zero dimensions, overview and trend boundaries, output failure, and wide-but-short graph suppression without implicit mutation.
- [ ] 3.3 Wire the named terminal-size CLI contract into required Linux, Windows, and macOS CI plus Linux and Windows installed-candidate release checks; perform Windows ConPTY and representative Linux/macOS visual review.
- [x] 3.4 Run focused tests, `cargo fmt --check`, `cargo check --workspace --all-targets --all-features`, warnings-denied workspace Clippy, full workspace/all-feature and doc tests, warnings-denied docs, strict OpenSpec, IssueOps checklist parity, ProjectAtlas lint, and bounded-output checks with explicit timeouts.
- [ ] 3.5 Update durable TUI, release, platform, accessibility, and failure guidance; resolve or disposition every live review and automated finding and verify the exact packaged RC2 behavior without displacing v0.4.4 Latest.
- [x] 3.6 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.
