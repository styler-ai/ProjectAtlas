## 1. Reproduction and Ownership

- [x] 1.1 Map `bound-impact-analysis-deadlines` to GitHub issue #380 in `openspec/issue-map.json` and keep the issue checklist synchronized with this file.
- [x] 1.2 Add deterministic leaf and larger-entrypoint regressions that expire during dead-code discovery, SQLite work, traversal, hydration, and output, plus a successful non-expired control.

## 2. Complete Aggregate Control

- [x] 2.1 Thread the existing captured request control through every impact/dead-code candidate, traversal, hydration, composition, and rendering phase using one absolute deadline and cancellation source.
- [x] 2.2 Make every owning SQLite read bounded and interruptible at its batch/iteration boundary, and prove statements, snapshots, task records, and intermediate collections are released on terminal control.
- [x] 2.3 Preserve typed deadline, cancellation, truncation, coverage, confidence, total-state, and safe-partial semantics through the shared service result and equivalent CLI/MCP adapters.

## 3. Adapter and Release Verification

- [x] 3.1 Add real MCP and CLI elapsed-tolerance tests, immediate follow-up responsiveness, no partial authoritative mutation, and declared row/node/edge/visited/intermediate/output budget coverage.
- [ ] 3.2 Run focused service/database/CLI/MCP tests, `cargo fmt --check`, workspace check/clippy/test/doc gates, OpenSpec validation, IssueOps, and live review-feedback reconciliation.
