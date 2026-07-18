## 0. Execution Order

Work in dependency order. Land significant compiling behavior slices into `dev` after focused behavior checks and ordinary locked Rust/workspace gates pass. A single coherent test may cover several related tasks. Do not add per-task tests, receipts, SHA ledgers, rendered evidence, issue sealing, or repository-wide mutation/coverage machinery.

## 1. Lean Planning And Source Recovery

- [x] 1.1 Replace the old evidence-driven #308 plan with lean proposal, design, capability specs, and behavior-slice tasks that preserve the accepted product scope.
- [x] 1.2 Map the change to issue #308, replace the stale GitHub status/checklist with this exact task list, and pass strict OpenSpec plus IssueOps validation.
- [ ] 1.3 Review every retained old #308 branch and dirty worktree, independently reapply useful product behavior on current `dev`, reject obsolete evidence/workflow machinery, and delete the old branches after recovery.
- [x] 1.4 Freeze representative 0.3.26 agent workflows and tasks for startup, purpose-led folder/file selection, inspect/summary, relations, and exact slice so navigation improvement can be evaluated without changing the normal funnel.

## 2. Typed Graph And Safe Storage

- [ ] 2.1 Add responsibility-owned typed graph identities, entities, logical relations, retained source occurrences, resolution, confidence, coverage, generation, reusable target selectors, and limits while preserving legacy relation compatibility and failing closed on stable-key collisions.
- [ ] 2.2 Add read-only database/root/schema preflight and append-only migration ownership that preserves project identity, purposes, review state, settings, and telemetry on supported upgrades and refuses incompatible state without mutation.
- [ ] 2.3 Implement validated full-scan staging and one-transaction generation publication that keeps the last valid generation queryable; add physical slotting or retained rollback generations only when a measured recovery/concurrency need justifies them.
- [ ] 2.4 Implement one-transaction incremental structural deltas, with affected-row invalidation, one generation advance, and complete rollback on late failure.
- [ ] 2.5 Persist and query typed graph, source-context, coverage, and lexical rows through indexed prepared operations that propagate corruption/iteration failures and avoid whole-graph scans or generic JSON hot paths.
- [ ] 2.6 Implement explicit verified root-move and copy/detach behavior so independent worktrees or clones cannot collapse project identity and snapshots cannot overwrite destination identity.

## 3. Freshness, Invalidation, And Bounded Work

- [ ] 3.1 Make every normal index-backed read, watch, and one-shot refresh detect relevant local edits, deletes, renames, ignore/configuration changes, and optional VCS-state changes after edits or restart; reconcile a safe bounded delta or return typed `refresh_required` instead of serving known-stale file, summary, symbol, relation, or search results in git or non-git source trees.
- [ ] 3.2 Preserve last-valid facts on transient path/root/permission uncertainty and re-resolve inbound dependents when exported identities or dependency keys change.
- [ ] 3.3 Prove create, modify, move, rename, delete, ignore/unignore, parser/configuration change, interruption, and unchanged-dirty sequences converge to clean-scan graph, coverage, and lexical results without repeated no-change publication.
- [ ] 3.4 Keep indexing cancellable and resource-bounded, preserve last-valid queries during failed work, and allow isolated projects to progress without one process-global lock; when isolated workers/packs exist, derive an effective resource envelope from configured safe limits plus supported host/container/job constraints.

## 4. Language Capability And Resolution

- [ ] 4.1 Generate deterministic detection, parser ownership, fixtures, tiers, settings, and documentation inputs from one versioned language capability registry and accepted capability-set manifest with honest detected/parsed/symbols/semantic/benchmarked states, derived counts, and no silent loss or weakening of accepted rows.
- [ ] 4.2 Preserve every current built-in language and parser behavior while adding deterministic exact-filename, compound-extension, extension, content/dialect, and explicit-override precedence.
- [ ] 4.3 Keep broad grammar/parser capabilities in an explicit supply-chain-verified optional pack with pinned provenance/digest/license/ABI, offline normal use, supervised out-of-process or capability-denied WASM/native containment, hard resource/cancellation limits, no repository execution, and no default-core download, compilation, linkage, or initialization; implement lifecycle only alongside a selected consuming runtime and preserve the MCP process plus active generation on pack failure.
- [ ] 4.4 Add normalized project-wide registries and independently structured semantic providers with resolved, ambiguous, unresolved, and external outcomes plus non-vacuous positive and negative fixtures for every advertised family; accept typed compiler metadata only as bounded non-executable, secret-redacted data where required.
- [ ] 4.5 Make existing settings report schema/migration compatibility, active generation, optional selected-slot detail, registry/provider/relation digests, actionable coverage, lexical/FTS/search state, and optional-pack lifecycle from validated state without secrets or new machine-local paths; derive capability matrices, provenance/license inventory, and public support claims from the same authority.

## 5. Agent-First Navigation And Analysis

- [ ] 5.1 Extend the purpose queue with coalesced task/generation/path-scoped rows and update packaged host-neutral guidance/agent profiles to run isolated low-reasoning `low`-scope curation beside the main task at startup/relevant transitions when supported; keep successful maintenance out of normal responses, never start `medium`/`strict` implicitly, and never write SQLite directly.
- [ ] 5.2 Enrich existing folder and file rows before summary with reviewed one-line purposes plus crisp bounded package/import/call/reference/test/route/config connections, while preserving exact path/name and strong purpose priority with deterministic compact reason codes.
- [ ] 5.3 Add deeper bounded relationship/coverage digests, opt-in project-wide coverage discovery, and typed next-call hints to selected-file summaries/health surfaces so agents gain useful context without default edge dumps or extra mandatory calls.
- [ ] 5.4 Preserve deterministic lexical search semantics, use FTS/BM25 only as exact-verified candidate acceleration, and add explicit optional `semantic`/`hybrid` modes with typed unavailable-state errors, lexical-complete hybrid ranking, and differential fallback-equivalence coverage.
- [ ] 5.5 Extend existing relation queries additively with typed direction, extended family, depth, confidence, resolution, retained source occurrences, reusable exact target selectors/next calls, pagination, and hard limits while old requests retain legacy rows and ordering.
- [ ] 5.6 Add bounded architecture, language-valid complexity/bottleneck candidate, VCS-aware impact/dead-code candidate, and node-simple trace views through existing summary/health/relation services; add at most one optional closed analysis tool only if real agent tasks prove the relation request becomes less clear, and do not add a generic query language or fourth complexity tool.
- [ ] 5.7 Bind cursors to project/root, active generation, capability, query, filters, ordering, and result-defining budgets; keep task-appropriate TOON, typed graph/path, verbatim slice, and supported JSON output valid, deterministic, UTF-8 safe, and uniformly bounded under serial or parallel execution.

## 6. Enrichments, Explicit Federation, And Optional Capabilities

- [ ] 6.1 Add one versioned accepted relation-family inventory and independently gated typed structural/type, package/manifest, test, route/protocol, configuration/environment, deployment/infrastructure, bounded static read/write, and optional inferred similarity/co-change relationships with source context, ambiguity, secret exclusion, invalidation, derived settings/query/docs/fixture coverage, and adversarial negative fixtures; include bounded caller-argument/request-field to handler-parameter/request-field context only where statically supported, and reject silent loss or weakening of accepted rows.
- [ ] 6.2 Add explicit ordered-root, call-only, read-only federation for approved relation/analysis calls with project-qualified identities, all-roots fail-closed validation, bounded in-memory resolution, and no persisted roots or cross-project writes.
- [ ] 6.3 Add an optional semantic retrieval lifecycle whose install, enable, build, ready, stale, update, rollback, disable, failure, and removal states cannot affect structural/lexical publication or default-core operation; select ANN/model composition only after labeled retrieval quality, determinism, update-cost, memory, package, and platform checks.
- [ ] 6.4 Add safe derived-graph snapshot export/import only through consistent SQLite backup, bounded archive validation, root/schema/digest checks, authored-data preservation, and normal atomic generation publication.

## 7. Integrated Behavior And Issue Completion

- [ ] 7.1 Add focused owning unit/integration tests plus risk-required real CLI/MCP, concurrency, corruption, Unicode, cancellation, and affected-platform checks for the coherent behavior slices above; reuse one test across related tasks when it proves the behavior.
- [ ] 7.2 Add a closed `agent | full` MCP surface: installer-generated configs advertise the compact documented agent inventory within its discovery-byte budget, bare/manual full mode preserves every old name/schema/default, redundant aliases share services, and representative purpose-led local-source workflows prove no mandatory call growth or semantic drift.
- [ ] 7.3 Use the candidate as the main agent's first navigation tool on representative dirty-worktree, clean, and non-git tasks; require correct fresh source selection through purpose-plus-crisp-connections before summary, then trust and exact slice, with no regression in calls, reads, backtracking, emitted bytes, or total context against 0.3.26 and the stronger predefined comparison targets.
- [ ] 7.4 Pass focused checks, ordinary locked Rust/workspace gates, source/dependency policy, strict OpenSpec, and IssueOps synchronization on the combined #308 surface; resolve bounded Rust/storage/performance/security/platform/KISS/agent-workflow reviews.
- [ ] 7.5 Reconcile `docs/agent-navigation.md` plus generated capability and user/agent documentation against validated behavior, add exact-commit OpenSpec links to issue #308, delete the fully recovered old #308 branches/worktrees, synchronize the completed checklist, and close the issue before #314 and #311 proceed.
