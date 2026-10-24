## Context

See proposal.md and #697-#699 for the recorded RC3 reproductions. SQLite's built-in lower() and ASCII normalization cannot implement Unicode-insensitive matching. Temporary parser profiles retain cleanup ownership after an explicit failure. The indexing runtime already uses one cooperative budget with a 30-minute ceiling.

## Goals / Non-Goals

Restore truthful search, cleanup and timeout behavior without a schema migration, new runtime component, expanded resource budget or unrelated feature. Preserve exact source lines, deterministic pagination, typed refusal, last-valid publication, authored data, process containment and canonical project identity.

## Decisions

- Use locale-independent Unicode simple case folding, matching existing regex semantics without canonical normalization. Escape literal patterns for the existing RegexBuilder. Fuzzy search greedily compares source scalars with per-query folded character classes from the already locked regex-syntax public HIR API, preserving linear subsequence traversal and exact source lines. Keep the ranking helper's existing policy unchanged. The 64 KiB pattern ceiling bounds preparation; a focused maximum-size probe does not justify matcher caching.
- Reuse the existing bounded persisted-text fallback. Candidate filters must be conservative for Unicode; keep acceleration only where it remains a complete superset and cover ASCII queries against Kelvin-sign and long-s variants. Skip SQLite's ASCII-only substring prefilter for case-insensitive legacy candidate visits; preserve exact case-sensitive filtering, streaming and early stop. A service-only change is insufficient because storage callers also expose literal filtering. No schema/index change is needed.
- Reuse the existing connection-owned prepared-statement cache for repeated persisted-text hydration. A repeatable optimized-build CLI/MCP comparison justifies avoiding repeated SQL compilation; preserve parameter binding, typed row validation and fresh reads after replacement, rollback and deletion. No result cache, new connection, schema or resource setting is needed.
- Consume the temporary profile's one cleanup attempt before calling the fallible cleanup owner. An explicit error reaches its caller; Drop must not silently spend another full budget. A forgotten terminal action still has one bounded fallback. Reuse the existing operational cleanup error where appropriate; preserve archive/manifest validation errors and terminal process/pipe supervision. Reject a general cleanup framework and blanket timeout increases.
- Reconcile schema/help/docs with existing shared work control. Preserve the 30-minute default/maximum, clamping of representable larger budgets, fail-closed refusal for unrepresentable deadlines, covered traversal/hash/parser/publication checkpoints and separate startup boundaries. No new timer or configuration is justified.
- Keep separate issue/PR boundaries and one independent review per boundary. Existing release owner #492 is reopened for RC4 and implements no product fix. Extend its existing complete-v050-release-readiness task slice with RC4 acceptance while preserving all completed RC1-RC3 tasks and the existing native hierarchy. Stable promotion remains separate #602 work.

## Risks / Trade-offs

- Broader candidate admission can increase search work: retain existing file/byte/time/output bounds and incomplete-result reporting; test accelerated/fallback equivalence and early stop.
- Unicode normalization can alter offsets: return original source lines and positions, with case-sensitive negative controls.
- Disarming an explicit cleanup attempt could hide residue: return the cleanup error, preserve containment/reaping, prove deterministic failure and a later clean retry; never report unsafe cleanup as success.
- RC4 version changes trigger packaging: land them only after all three fixes are accepted; execute exact-package, supported-upgrade and four-platform proof before final completion.

## Migration Plan

No database migration or source rewrite. Land reviewed fixes, freeze accepted main, update canonical RC4 metadata/pins, execute package/upgrade acceptance, publish v0.5.0-rc4 as a non-draft prerelease with Latest excluded, then independently verify artifacts and installed identities. Preserve compatible rollback, existing authored state and v0.4.5 Latest. Keep #492 open until final readback succeeds; #602 retains stable promotion.

## Dependencies / Cross-Issue Impact

#697, #698 and #699 are independent direct children and blockers of #492. The primary implements them sequentially. No prerequisite dependency is invented between the fixes. Existing completed RC1-RC3 acceptance remains historical. npm and later agent-surface work remain outside this release. Additional non-blocking defects target v0.5.1.

## Open Questions

None.
