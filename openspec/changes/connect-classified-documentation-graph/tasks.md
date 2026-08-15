## 1. Contract And Architecture

- [x] 1.1 Map issue #440 to this OpenSpec change and define the release scope, non-goals, ownership boundaries, and exact-routing dependency on #430 plus release-policy dependency on #448.
- [x] 1.2 Specify the five closed content classifications, registry authority, omitted-selection compatibility, explicit selection values, and additive adapter contract.
- [x] 1.3 Specify bounded Markdown/MDX heading and explicit-link extraction, canonical `documents` storage, inbound `documented_by`, typed unresolved outcomes, privacy, and incremental invalidation edge cases.
- [x] 1.4 Specify schema-17 ownership, atomic publication, query/index/batching expectations, migration and recovery, intended-scale performance, and clean-build equivalence.
- [x] 1.5 Specify private per-worktree database refresh, exact `project_path` isolation, dirty-source authority, structural manager selection, ordinary-checkout behavior, and seamless v0.4.4 upgrade paths with #430.
- [x] 1.6 Add the focused classified-document/worktree architecture view, render it with Mermaid CLI, inspect it visually, and link its owning heading from issue #440.

## 2. Core And SQLite Foundation

- [x] 2.1 Add typed content classification and selection contracts to the core language registry, plus `Heading`, `Documents`, and the closed unresolved document-target reasons without scattering protocol strings.
- [x] 2.2 Add the append-only active-atlas schema 16-to-17 migration with one constrained classification table, file ownership, classification/path index, and no duplicate inverse document table.
- [x] 2.3 Land prepared and batched classification write/read APIs, endpoint projection, inbound `documents` lookup, generation cleanup, and query-plan assertions before dependent services.
- [x] 2.4 Prove schema-17 write/read/reopen, v0.4.4 migration, rollback on constraint/busy/interruption failures, corrupt/newer-schema refusal, and preservation of authored purposes in real SQLite tests.

## 3. Markdown Facts And Document Resolution

- [x] 3.1 Move reusable Markdown/MDX heading extraction into `projectatlas-symbols` with the existing `pulldown-cmark`, exact byte/line selectors, parser provenance, and explicit byte/count/evidence limits; make structural summaries reuse those facts.
- [x] 3.2 Extract only parser destinations and complete repository-path code spans, rejecting images, external/absolute/drive/UNC/dynamic/fragment-only/directory/prose candidates and unsupported RST/JSX/HTML structure without guessing.
- [x] 3.3 Resolve candidates relative to the owning document through exact indexed identities, case/collision and symlink/root checks, ignored/missing distinction, bounded repository-relative evidence, fragment/heading selectors, and stable deduplication.
- [x] 3.4 Publish headings, canonical document relations, completeness, and unresolved evidence in the same generation transaction as classifications and graph facts.
- [x] 3.5 Make add/change/delete/rename/ignore/case refresh recompute changed documents plus the affected inbound closure, and prove incremental/full-build equivalence and prior-generation retention on failure or cancellation.

## 4. Shared Service And Adapter Behavior

- [x] 4.1 Apply one service-owned explicit-selection predicate before ranking, pagination, anchor selection, and frontier expansion while preserving omitted-selection candidate/order/cursor compatibility and explicit cross-class document endpoints.
- [x] 4.2 Batch and expose classification on every file-bearing files, search, summary, purpose, symbol, relation, analysis, capability, and next-call result without letting purpose mutations override it.
- [x] 4.3 Add semantically equivalent CLI JSON/TOON and MCP schemas, validation, exact continuation fields, capability reporting, and side-effect-free errors for unsupported selection.
- [x] 4.4 Preserve legacy relation-family defaults so `documents` appears only when requested or classified traversal opts in, and keep existing source, configuration/data, other-text, counters, ranks, and pagination behavior intact.
- [x] 4.5 Update the version-matched shipped ProjectAtlas skill and user guidance for source/documentation/both selection, trust/completeness handling, `documents`/`documented_by`, exact source follow-through, and per-call worktree routing.

## 5. Exact Worktree Integration

- [x] 5.1 Keep schema-17 classifications, headings, canonical document relations, provenance, completeness, and unresolved evidence inside the selected exact-root atlas with no sibling or common-manager graph authority.
- [x] 5.2 Prove each linked worktree independently refreshes additions, changes, removals, rename/delete/case changes, and dirty saved bytes before exposing a complete classified generation.
- [x] 5.3 Expose bounded structural manager/worktree status without opening a sibling atlas or changing the current TUI.
- [x] 5.4 Prove simultaneous explicit `project_path` requests, captured request context, session-default changes, and ambiguous bare/common managers cannot mix classifications, relations, purposes, generations, or next calls between sibling worktrees.
- [x] 5.5 Prove ordinary and linked-worktree v0.4.4 databases migrate in place without losing purposes, missing databases build locally, and offline/no-Git fallback needs no manual database surgery.

## 6. Focused Verification

- [x] 6.1 Cover classification, selection, heading identity, parser limits, candidate admission, resolution, deduplication, cycles, privacy, and negative/unsupported paths with owning core and parser unit tests.
- [x] 6.2 Cover schema constraints, index/query plans, batching, atomic generation publication, rollback/recovery, WAL/concurrency, migration, and incremental closure with SQLite integration and fault tests.
- [x] 6.3 Cover legacy compatibility, pre-limit filtering, cross-class endpoints, pagination/cursor identity, completeness, cancellation, and exact next calls with service integration tests.
- [x] 6.4 Cover CLI/MCP parity, malformed selections, no-side-effect failures, bounded output, current-source follow-through, and Windows/Linux/macOS path, case, Unicode, and symlink behavior with real adapter and platform E2E.
- [x] 6.5 Run one joint #430/#440 holistic E2E from a v0.4.4 fixture through two isolated worktree builds/divergence, structural CLI/MCP status, and classified bidirectional traversal under one interleaved MCP process.
- [x] 6.6 Measure a representative full build and high-fan-out one-document refresh for CPU, wall time, allocations/RSS, SQLite statements/lock time, WAL/I/O, persistent bytes, affected rows, and bounded output; address regressions rather than inventing unmeasured claims.

## 7. Release Readiness

- [x] 7.1 Update durable architecture, database, graph, workflow, upgrade, privacy, and failure guidance so #430 and #440 describe one coherent exact-worktree lifecycle without duplicating authorities.
- [x] 7.2 Render and visually inspect every changed Mermaid block, checking semantic ownership, arrow direction, termination, readability, and agreement with the live implementation.
- [ ] 7.3 Run the affected focused tests, then `cargo fmt --check`, `cargo check --workspace --all-targets --all-features`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace --all-features`, `cargo test --doc --all-features`, and `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features` with explicit timeouts.
- [ ] 7.4 Run ProjectAtlas lint, strict OpenSpec validation, IssueOps parity/readiness, installer/plugin/MCP drift checks, packaged cross-platform E2E, and exact-head GitHub checks; resolve or disposition every live review and automated finding.
- [ ] 7.5 Synchronize issue #440's checklist and metadata only after all implementation, compatibility, verification, cross-issue, and release-blocker work is complete.
- [ ] 7.6 Confirm the v0.4.5-rc1 release notes, shipped skill, installers, holistic agent E2E, and prerelease/latest-state verification include the complete #430/#440 behavior.
- [ ] 7.7 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.
