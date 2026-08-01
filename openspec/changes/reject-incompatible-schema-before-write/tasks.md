## 1. Change Control and Failing Regression

- [x] 1.1 Map `reject-incompatible-schema-before-write` to GitHub issue #410, mirror this checklist in the issue, and keep every implementation task unchecked until its complete behavior and owning tests pass.
- [x] 1.2 Add one real newer-schema fixture with representative project identity, authored purposes, health resolutions, telemetry, derived rows, and an active uncheckpointed WAL; capture main/WAL bytes, sidecar inventory, schema objects, metadata, and durable rows so the normal writable open reproduces any mutation before typed refusal.

## 2. Database-Owned Compatibility Boundary

- [x] 2.1 Trace every normal writable store caller through `schema::preflight` and `AtlasStore::open_with_binding_requirement`; retain the single read-only gate before writable open, WAL policy, write pragmas, DDL, repair, index creation, and migration, changing only that shared owner if the failing regression exposes a gap.
- [x] 2.2 Prove newer-schema refusal returns the exact typed database version error while the live WAL owner remains usable and all captured database/WAL bytes, schema objects, metadata, authored state, telemetry, and derived rows remain unchanged without checkpointing.
- [x] 2.3 Retain positive current-schema and admitted-predecessor behavior and cover malformed/missing version metadata, incompatible schema shape, wrong-root ownership, and genuinely missing-database classification without implicit creation, rebind, repair, or migration.

## 3. Shared CLI and MCP Contract

- [x] 3.1 Add the shared `schema_version_mismatch` agent error kind and one typed payload/extractor for found schema, supported schema, and runtime versions; reuse it from CLI JSON/TOON and MCP encoding without paths, roots, SQL, metadata values, or authored content.
- [x] 3.2 Cover exact CLI JSON/TOON mismatch fields plus human diagnostics, privacy-negative assertions, an actionable supported-predecessor migration handoff that preserves explicit CLI and MCP database selection, and unchanged behavior for current, missing-index, wrong-root, malformed, and non-schema failures.
- [x] 3.3 Exercise a real stdio MCP initialize/tool exchange with explicit `project_path` against the newer active-WAL database, a missing index, and a wrong-root index; assert typed mismatch/init/project errors, protocol-correct failure, no implicit mutation, and subsequent MCP session behavior.

## 4. Windows Locked-Mirror Recovery

- [ ] 4.1 Extend the existing installer partial-convergence output using its bounded probes and captured readiness state to report the exact stale bare-command path/observed version, verified absolute runtime/target version, absolute verification/use command, restart applicability, and unlock/rerun/bare-command gate without broadening #411 handoff authority.
- [ ] 4.2 On Windows, cover obsolete locked mirror, unavailable stale-version probe, already-current command, fresh-PATH shadowing, non-retireable lock-owner survival, config-only and runtime drift, verified versioned-runtime/generated-config use while locked, unlock plus installer rerun, target bare-command version, and bare token TUI success against the unchanged compatible database; do not claim `handoff-obsolete-mcp-runtime` task 4.1.

## 5. Packaged, Performance, and Release Verification

- [x] 5.1 Construct and install the official packaged runtime in an isolated destination, verify its exact version and absolute path, and prove both packaged CLI and real stdio MCP return the shared typed refusal with the complete active-WAL snapshot unchanged.
- [x] 5.2 Profile newer-schema refusal on small and representative large row populations; confirm CPU, memory, SQLite reads, and elapsed work remain bounded by schema metadata with no table-sized scan, database/WAL write, checkpoint, or persistent growth.
- [x] 5.3 With explicit 20-minute process timeouts, run `cargo fmt --check`, `cargo check --workspace --all-targets --all-features`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace --all-features`, `cargo test --doc --all-features`, and `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`, plus focused database, CLI/MCP, packaged-runtime, and Windows installer tests.
- [ ] 5.4 Run strict OpenSpec validation, `.github/scripts/issue-checklists.py`, applicable release/package gates, and authenticated live review-thread/Codex/Dependabot feedback reconciliation before task, issue, PR, or release transition.
