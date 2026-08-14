## 1. Contract and dependency order

- [x] 1.1 Finalize the lean Memory Atlas proposal, design, capability specs, issue title/body, `checklist-v1` ownership mapping, synchronized OpenSpec/GitHub checklist, and shared behavior-focused test plan.
- [ ] 1.2 Before runtime implementation, confirm #308 has stabilized the schema epoch, authored/derived publication boundary, freshness preflight, coherent read snapshot, selected-root behavior, `atlas_session_brief` extension point, and streamlined MCP inventory consumed by this change.
- [ ] 1.3 Trace the existing core, database, service, CLI/MCP, settings, host-plugin, and migration callers; record the Rust pattern-fit decision and keep ownership within the existing seven crates unless a demonstrated durable boundary requires otherwise.

## 2. Typed Memory Atlas model and measured budgets

- [ ] 2.1 Add closed responsibility-named core types for Memory Atlas kinds, stable identities, attribution, reviewed/current lifecycle state, portable typed references, conditional revisions, pressure, warnings, recovery/update payloads, and project-goal/active-issue skill-route scope, requirement, read-order, rationale, and resolution fields.
- [ ] 2.2 Define validated project-relative folder/file/symbol/span, issue, OpenSpec, public-documentation, skill, and plugin selectors that reject copied bodies, machine-local paths, executable/install directives, invalid route scope/order metadata, and other cross-boundary content.
- [ ] 2.3 Measure representative goal, scope, architecture, pattern, decision, workflow, route, and checkpoint fixtures; derive compatible configurable defaults below compiled per-record, row, retained-byte, checkpoint, recovery-row, and recovery-output maxima.
- [ ] 2.4 Add shared core/config tests for positive and negative serialization, canonical stable keys and revision strings, Unicode byte accounting, lifecycle/expiry rules, scoped skill-route validation and ordering metadata, invalid selector/content rejection, exact limits, pressure thresholds, and backwards-compatible defaults.

## 3. SQLite authored-state lifecycle

- [ ] 3.1 Add the append-only SQLite migration, constrained tables/indexes, one context revision, retained-size accounting, and schema inventory for authored Memory Atlas rows without adding a database, crate, or runtime dependency.
- [ ] 3.2 Implement coherent root-bound read/list snapshots and one revision-conditional transaction for complete-batch validation, stable-key upsert, exact remove/supersession, deterministic volatile cleanup, budget reconciliation, and one revision advance.
- [ ] 3.3 Preserve exact no-op semantics and rollback guarantees: no revision/timestamp change for identical state, and no partial cleanup or mutation after stale revision, busy database, malformed input, revision exhaustion, or protected pressure.
- [ ] 3.4 Preserve Memory Atlas rows and independent revision across supported migrations, repair/rollback, backup/restore, full and incremental source publication, watcher refresh, and derived-state cleanup while older runtimes reject the newer schema.
- [ ] 3.5 Add shared database tests covering migration, repair/rollback, backup/restore, full and incremental publication, watcher refresh, derived-cleanup authored-state preservation, wrong-root and missing-index failure, concurrent stale writers, repeated-write steady state, exact budget boundaries, eligible-only cleanup, interrupted transactions, future-schema rejection, and no implicit scan/migration/write.

## 4. Reflection and recovery services

- [ ] 4.1 Implement service validation, task relevance ranking, one-instant lifecycle evaluation, pressure planning, bounded pagination, and deterministic bird's-eye recovery that preserves project-goal governing skill routes, ranks active-issue routes, deduplicates shared routes, and returns their required-read order over concrete types and existing service boundaries.
- [ ] 4.2 Implement atomic reflection batches that require the observed revision, replace current facts, remove or supersede obsolete identities, protect durable current context, and return content-free typed conflicts or pressure.
- [ ] 4.3 Implement quiet harness-owned maintenance semantics: bounded input only, exact no-op silence, stale background writers lose without overwriting/retrying, recovery never waits, and only actionable conflict/pressure/root/content failures surface.
- [ ] 4.4 Route recovery references back into the local-source sieve: folder purpose plus graph role, file purpose plus relevant connections, summary plus trust/coverage, then exact slice, with reusable selectors and accurate next-call hints even when structural references are stale.
- [ ] 4.5 Add shared service tests for replacement without growth, eligible-only reconciliation, conflicts/failure rollback, fixed-instant deterministic recovery, root isolation, positive reusable selectors and next-call funneling, stale structural selectors, project-goal/active-issue route preservation, deduplication, ordering and issue-transition retirement, quiet maintenance, offline behavior, and privacy sentinels.

## 5. CLI, MCP, settings, and compatibility

- [ ] 5.1 Add typed CLI Memory Atlas read/update plus deeper validate and explicit compact dry-run/apply administration through the shared services, with bounded JSON/TOON output and no generic admin multiplexer.
- [ ] 5.2 Add only `atlas_memory` and `atlas_memory_update` to MCP for bounded retrieval and atomic reflection; keep project goal updates in the same batch and reject any separate goal or jump tool unless measured workflows later justify it.
- [ ] 5.3 Extend `atlas_session_brief` with optional recovery and `atlas_settings` with content-free capability, budget, pressure, count, and revision state while preserving omitted-request behavior and existing CLI/MCP request/response defaults.
- [ ] 5.4 Add shared adapter/E2E tests for CLI/MCP parity, TOON/JSON equivalence, pagination/cursor invalidation, configured limits, normal session-brief compatibility, project-goal replacement, CLI validation and compact dry-run/apply, MCP inventory, wrong-root/missing-index behavior, and symbol-build/source-refresh independence.

## 6. Host integration and agent workflow

- [ ] 6.1 Encode and document verified capability-based startup, resume, clear, post-compaction, and supported subagent recovery for Codex, Claude Code, OpenCode, and generic MCP, with trusted-hook activation state and truthful manual fallbacks.
- [ ] 6.2 Update packaged ProjectAtlas skills, plugin guidance, `AGENTS.md` snippets, and user documentation so agents resolve and completely read current required project-goal skills followed by active-issue skills, checkpoint only meaningful bird's-eye changes, keep successful maintenance quiet, and never crawl or duplicate host-private memory/goals/tasks.
- [ ] 6.3 Add isolated host-contract tests for startup/resume/clear/compact/subagent paths, disabled/untrusted hooks, synthetic homes/configs, offline behavior, project-goal/active-issue route resolution and complete-read ordering, stale/unavailable routes, privacy sentinels, and no host-global mutation or capability invention.

## 7. Issue-level verification and completion

- [ ] 7.1 Run the ordinary locked workspace gates on the complete implementation: formatting, all-target/all-feature check, Clippy with warnings denied, workspace and doc tests, and rustdoc warnings denied; keep ProjectAtlas scan/purpose/lint maintenance local rather than adding hosted self-scans.
- [ ] 7.2 After the complete #314 surface is implemented, run line coverage and mutation once, close real gaps or document justified exclusions, and do not create per-task coverage/mutation campaigns.
- [ ] 7.3 Run bounded final reviews for architecture/crate ownership, Rust skill and pattern fit, OpenSpec task truth, shared-test adequacy, privacy/root/migration safety, streamlined MCP usefulness, and whether the result materially improves agent recovery without an eighth crate.
- [ ] 7.4 Synchronize completed OpenSpec/GitHub task states, verify the issue checklist gate and packaged documentation, and close #314 only after the implementation is merged and the complete issue checks pass.
- [ ] 7.5 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.
