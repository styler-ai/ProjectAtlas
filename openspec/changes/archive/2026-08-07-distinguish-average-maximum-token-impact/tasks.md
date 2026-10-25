## 1. Contract and Release Scope

- [x] 1.1 Map `distinguish-average-maximum-token-impact` to GitHub issue #439 in `openspec/issue-map.json`, mirror this checklist exactly into the issue, and keep v0.4.4 limited to this issue.
- [x] 1.2 Preserve the accepted 50% directory-walk formula, unchanged Atlas payload charge, explicit average/maximum names, existing-field compatibility, fixed non-goals, and no-schema-migration decision across the spec and implementation.

## 2. Shared Accounting

- [x] 2.1 Extend the existing core telemetry overview and raw/bucket aggregation paths with signed, saturating average and maximum values while applying 50% only to the deduped aggregate modeled `directory_walk` baseline.
- [x] 2.2 Derive the same values from existing bounded SQLite aggregate dimensions and counters without schema, query, transaction, WAL, pruning, or recovery changes; preserve the folder discriminator in one reserved overflow row, label predecessor unclassified overflow, and add real SQLite parity round trips.
- [x] 2.3 Preserve measured compression, non-folder modeled categories, search policy, buckets, trends, read-avoidance counts, and legacy decoding; verify JSON/TOON plus CLI/MCP report compatibility with positive, negative, repeated-baseline, odd-baseline, and older-field cases.

## 3. Minimal TUI and Documentation

- [x] 3.1 Keep the current Ratatui dashboard composition, make average tokens avoided the primary hero, and stack complete average and maximum without-minus-with equations using only the concise `Average avoided` and `Maximum avoided` result labels.
- [x] 3.2 Add deterministic `TestBackend` text, ordering, signed-value, narrow-width, and semantic-style coverage for dark, light, and terminal themes without weakening existing dashboard tests.
- [x] 3.3 Update version-matched user, plugin, and release documentation only where required to define the two formulas and v0.4.4 compatibility boundary.

## 4. Verification and Release

- [x] 4.1 Run focused tests, `cargo fmt --check`, workspace check/Clippy/tests/doc-tests/rustdoc, ProjectAtlas lint, OpenSpec strict validation, IssueOps checklist validation, diff checks, and affected packaging/compatibility/release gates with explicit timeouts.
- [x] 4.2 Render and visually inspect the real token TUI at desktop, common, and narrow terminal widths in dark, light, and terminal themes; fix overlap, truncation, hierarchy, contrast, or misleading formula presentation.
- [x] 4.3 Reconcile every live review thread and automated finding against the behavior-relevant source, configuration, dependency, artifact, and workflow inputs; merge only after hosted gates and packaged prepublication proof succeed, then synchronize the completed checklist and close issue #439 before dispatching the release workflow with `prepublish_only: false`.

Archive condition: Keep this OpenSpec change unarchived until the release workflow has published v0.4.4 and a separate post-publication verification confirms the GitHub release, platform artifacts and checksums, installer behavior, runtime version, plugin version, and MCP identity.

Archive disposition: The automatic release run was created 95 seconds before issue #439 closed, so task 4.3's literal pre-dispatch order was not met. The mandatory release checklist gate completed after closure, and all packaging and publication followed that gate. The enforced safety boundary therefore held; another dispatch would only republish the already-verified release inputs and is intentionally not performed.
