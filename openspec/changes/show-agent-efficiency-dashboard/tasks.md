## 1. Typed Comparison Contract

- [x] 1.1 Add backward-compatible core comparison states, identities, baseline metrics, and capability-contribution rows to the existing token overview, with serialization and unavailable/default coverage and no SQLite schema or write-path change.
- [x] 1.2 Implement the bounded service-owned benchmark reader and typed schema, identity, schedule, run-retention, numeric, failure, and path-boundary validation, including malformed, oversized, stale, missing, escaping, and indirection cases.
- [x] 1.3 Derive matched frozen-v0.3.26 and plain-control distributions, unmatched failure counts, unavailable percentages/break-even, and reconciled v0.4 capability call/byte rows without provider-token causality.

## 2. Existing Adapter Integration

- [x] 2.1 Add one optional repository-relative benchmark path to the existing token overview request and wire identical typed results through CLI JSON/TOON and `atlas_token_report`, preserving project isolation and no-implicit-mutation behavior.
- [x] 2.2 Add representative real CLI and MCP smoke coverage for compatible, partial, unavailable, incompatible, malformed, wrong-root, and backward-compatible requests.

## 3. Ratatui Dashboard

- [x] 3.1 Render one bounded agent-efficiency comparison panel only when a caller explicitly supplies benchmark evidence, hide it from the normal no-artifact token overview, and preserve the accepted headline, file-read, composition, source, calibration, footer, palette, background, and separate trend dashboard.
- [x] 3.2 Cover the hidden no-artifact state plus explicitly requested compatible, partial, failed, and incompatible states, normal/compact labels, semantic styles, bounded values and reasons, failed trials, hidden-overflow prevention, and unchanged accounting equations in deterministic Ratatui buffer tests.

## 4. Documentation And Release Proof

- [x] 4.1 Update the owning telemetry architecture view and user documentation for read-only benchmark validation, typed report ownership, CLI/MCP/TUI flow, provenance, compatibility, limitations, and provider-counter separation; render every changed Mermaid block and inspect semantic and visual correctness.
- [x] 4.2 Run focused core/service/DB-read-only/CLI/MCP/TUI tests, `cargo fmt --check`, workspace check, Clippy, full tests, doc tests, rustdoc, dependency policy, strict OpenSpec validation, ProjectAtlas lint, and IssueOps synchronization; capture and visually inspect a real normal-width dashboard render.
- [ ] 4.3 Inspect and disposition all live PR review threads plus Codex and Dependabot feedback on the exact head, then synchronize the completed OpenSpec and GitHub issue checklists before merge and closure.
