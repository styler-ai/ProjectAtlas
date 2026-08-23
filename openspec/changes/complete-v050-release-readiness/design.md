## Context

v0.4.5 is the stable baseline. v0.5.0 combines confirmed correctness repairs, bounded content/analysis capabilities, cross-platform distribution, structural maintenance, evaluation, and a real installed release proof. These issues share contracts but are not one serial implementation: the release graph records genuine prerequisites, while #492 is the native parent and direct final blocker for every other accepted issue.

The accepted Rust shape remains seven crates with concrete modules, typed values/enums, bounded iterators/frontiers, and caller-owned cancellation/transactions. SQLite remains the project-local storage and publication authority. No task may invent an initiative-named crate, generic framework, second database, or stringly action surface.

## Goals / Non-Goals

**Goals:**

- Make every issue packet implementation-decidable with one owner, exact dependencies, positive/negative/failure/compatibility/platform proof, and one truthful architecture view.
- Require exact published-default-branch OpenSpec and architecture evidence before an issue becomes ready, enters a release, or reaches implementation.
- Preserve strict identity, freshness, typed errors, exact source evidence, bounded output, one-generation publication, authored state, and platform security.
- Keep independent graph lanes parallel while requiring accepted predecessors on `main` before dependent implementation/merge.
- Reduce pull-request work to the smallest contract-complete affected proof while retaining full proof at every shared or release boundary, and reconcile all Dependabot updates through one release-owned campaign.
- Finish with complete installed CLI/MCP/host/public-surface execution, a hard-gated in-place v0.4.5 database update, holistic packaged E2E, RC remediation, stable readback, and #492 closes-last truth.

**Non-goals:**

- Visual clients, Memory Atlas, v0.6 route rationalization, generic extensibility frameworks, speculative persistence, or product fixes in release acceptance.
- Weakening trust boundaries, replacing real behavior with help/schema/unit proof, or carrying partial work through disposition.

## Decisions

### Release hierarchy communicates scope; blockers control order

#492 is the sole native parent of all other `v0.5.0-00` issues and has no parent. Every non-release issue appears once as its direct sub-issue. `release_graphs.v0.5.0-00.issues[*].blocked_by` is the execution authority; no separate children array duplicates hierarchy. #492 is directly blocked by every child, implements no feature/bug, and closes last.

### Candidate specification proof does not authorize readiness

A candidate checkout is the authority for proposed OpenSpec structure, issue mirrors, Markdown headings, locked-parser Mermaid syntax, rendered communication, and semantic review. It is not publication proof for a durable `/blob/main/` issue link. Sol first lands those artifacts through a planning PR without a native closing issue, then reads them from an exact clean checkout of the live default-branch revision. The snapshot admits only the addressed Git top-level root with a well-formed local HEAD equal to the well-formed live default-branch ref SHA and no tracked changes; ignored untracked notes remain outside tracked publication identity. Root mismatch, malformed local or remote identity, Git timeout/process/OS failure, or default-branch movement before final merge authorization fails closed. Only that stable published snapshot may authorize `status:ready`, a release milestone, native parent/blocker relationships, or Luna implementation handoff. Planned-issue, implementation-reference, merge-authorization, milestone, and release paths fail closed on candidate-only, stale, dirty, missing, or malformed evidence. This preserves the documentation bootstrap without allowing a candidate worktree to impersonate published state.

### IssueOps derives transitions before mutation and repairs reverse drift

The relationship mutation route first reconciles the selected declaration with live native state and derives the exact bounded transition set. A request is admitted only when its relation kind, orientation, issue, related issue, and add/remove operation equal one missing-or-extra declared transition. This gate also precedes an idempotent zero-mutation result: the source of `blocked_by` belongs to exactly one release graph, while the source of `sub_issue` is exactly one graph's declared release issue. Unknown, unowned, or multiply owned sources fail even when the requested native relation already exists or is already absent. Post-mutation readback and complete graph reconciliation remain mandatory. Generic rollback is deliberately not used as compensation for avoidable prevalidation failure.

Issue-event repair derives reverse direct-blocker adjacency solely from the declared release graphs. A closed issue with an open direct blocker is reopened. When a blocker reopens, a graph-bounded queue visits its closed reverse dependents, reopens or fails each dependent whose blocker remains open, and continues through newly reopened dependents so downstream closed state cannot remain falsely accepted. Affected pull requests are selected only through repository-qualified closing references; before every status or auto-merge mutation, IssueOps rereads the pull request's current exact head and complete closing references. Foreign same-number references fail, while a raced head or reference set is skipped for bounded retry and fails closed. Invalidation first revokes the protected `issueops-merge-authorized` context and uses the existing auto-merge disable safety where applicable, then attempts `issueops-implementation`; all exact targets and both contexts are attempted with aggregated failures instead of short-circuiting after one failed call. Total GitHub API failure is reported fail-closed and does not falsely guarantee remote revocation. Issue-map ownership selects the target graph even after a `demilestoned` event removes the live milestone, so milestone drift remains observable and fails.

### Native identity is one typed value and codec

#481 owns a concrete native project-root identity and lossless/versioned SQLite encoding. CLI, MCP, configuration, watcher, worktree, telemetry, graph, and persistence consume it; UTF-8 display is terminal and never feeds comparison. Legacy metadata repairs only after native equivalence proof in one transaction. #484 reuses the same type/codec for worktree root, Git common directory, and Git administrative directory; public UTF-8 adapters return stable alias plus typed display-unavailable state rather than replacement text.

### Graph admission stays strict and publication stays atomic

#476 preserves `GraphIdentityText` validation. One shared source-graph admission result classifies every parser-derived package/symbol/parent/relation/resolution-key input. Valid rows and bounded typed rejection provenance publish under the same generation; rejected text never becomes an identity. #480 validates whole-publication duplicate/target invariants, then processes prepared chunks no larger than `GraphLimits::MAX_ROWS` inside one existing publication transaction/generation. Fault/cancellation exposes no partial current state.

### Rust and parser capability have single authorities

`rust-toolchain.toml` becomes the sole numeric declaration for exact Rust 1.98.0. Rust 1.93.1 remains historical reproduction evidence only; every local/CI/parser-pack/package/release entry point preflights expected versus actual rustc/cargo/clippy/rustfmt before expensive or mutating work. Floating stable and duplicate literals are forbidden.

`PackPlatform` or one closed equivalent enum owns optional-parser containment, tuple selection, installer, lifecycle, supervisor, runtime/MCP capability, fallback, feature gates, and tests. macOS arm64 optional parsing is unavailable in v0.5; install/update/verify/select/start fail before mutation and built-in parsing remains truthful. #486 consumes this authority and changes cfg ownership only for diagnostics reproduced on Rust 1.98.0; a clean matrix closes with no-change evidence.

### PHP and document intelligence are pinned and conservative

#477 pins `tree-sitter-php` 0.24.2 against Tree-sitter 0.26.9 and owns PHP 8 grammar registration plus conservative exact namespace/import/include/call relations. Dynamic PHP and mixed HTML gaps remain typed partial coverage. #339 follows #477 and publishes exactly one evidence-derived PHP guidance profile from the registry, generated support, fixtures, representative repositories, and installed skill.

#465 supports only PDF and DOCX. `pdf-extract` 0.12.0 and `docx-lite` 0.2.0 are pinned and their locked transitive trees are audited before adoption. DOCX admits only `word/document.xml`; PDF runs no script/external reference. Evidence locators are page/text span or part/paragraph/run/text span. Input, expansion, entries, recursion, time, memory, output, cancellation, coverage, and sparse-link policy are explicit; any necessary SQLite delta lands before adapters.

### Analysis is typed, bounded, and measurement-led

#342 measures the existing reverse-caller query/service path and changes it only for a material winner with exact alias/fairness/plan compatibility. #358 evaluates existing worker pools under one process budget and accepts only an exact-graph resource win. #456 accepts a released-main baseline only for measured net benefit, exact revision/digest/schema identity, private writable copy, and safe full-init fallback.

#384 adds a non-persistent typed `EntrypointProfile` request with exact relation anchors/families and bounded node-simple traversal; outcomes are reachable, evidence-backed unreachable candidate, or inconclusive, never deletion authority. #464 replaces only the optional community projection with deterministic weighted label-propagation v1 over resolved local non-containment relations, stable ordering/tie-breaks/IDs, fixed weights and bounds, no persistence, and explicit convergence/coverage/truncation.

### Distribution reuses one verified runtime

#388 is a thin npm adapter with one package identity, exact tuple/asset/version/SHA-256 authority, explicit Node/npm floor, scripts-disabled route, proxy/offline semantics, process-safe cache locking, staged verification, and atomic activation. It never owns another runtime or database.

#390 verifies installer-generated Claude Code/OpenCode configuration through actual host readers in isolated homes/config roots, including MCP initialization and exact source-evidence readback. It never automates authentication or mutates unrelated global state.

#491 installs one collision-safe `atlas` forwarder to the same runtime while preserving `projectatlas`. `atlas health [report flags]` is the read-only report; `atlas health resolve ...` retains administration; `health-check` remains a compatibility alias.

### Maintenance follows proven responsibility

#372 adds only the existing GitHub Actions `timeout-minutes` field to the reported filtered custom-harness step and its narrow contract assertion. #487 uses the accepted five-domain E2E move map and extracts only multiply-owned support. #488 accepts or rejects cohesive moves from an explicit caller/state/data/SQL/transaction/cancellation/test map, preserving seven crates and permitting no-change. #489 removes only the named 42,809,126-byte raw trace from the current tree and extends the narrow existing policy. #490 gap-audits and reuses the existing equal-arm harness, retaining failures and publishing only bounded sanitized evidence.

### Lean CI and one dependency campaign preserve proof ownership

#497 owns one checked-in closed impact contract and one Python-standard-library planner reused by human and Dependabot pull requests. Rust reverse dependencies come from `cargo metadata`; only database/schema, CLI/MCP, Python, Node/Mermaid, documentation/OpenSpec/IssueOps, installer/package, fixture/generated-input, and platform edges are declared. A trusted pull-request plan runs the smallest complete subset of existing contracts. Unknown, shared, planner-owning, stale, malformed, rename/delete, or graph-failure input selects full proof; default-branch, scheduled, candidate, and release boundaries always run full proof. Stable protected contexts aggregate only affected success or exact plan-bound not-applicable evidence. #341's digest-addressed dependency reuse and #366's fail-closed input proof remain the foundations; #372 remains separate timeout work. Only superseded read-only pull-request CI may be canceled.

#498 owns intake, the exact source-aware Dependabot identity (`dependabot[bot]` event actor and `app/dependabot` authenticated PR-author readback), campaign relationship and IssueOps authorization contract, one bounded body-region reconciler, and one sequential default-branch `workflow_dispatch` audit driven solely by `.github/dependabot.yml`. #499 is the single v0.5.0 campaign issue declared by the existing release graph. Its inventory retains every currently open and release-window Dependabot PR through campaign close; `pending` and `provisional` block release until each record is finally `accepted`, `deferred`, `declined`, or `superseded`. Campaign automation reads #497 proof but never duplicates its planner. Dependabot PRs retain the same protected checks, review/thread readback, exact-head binding, and explicit Sol merge authorization; there is no per-PR issue, auto-merge, App/bot waiver, second graph/database/service, REST/UI hosted-update trigger, parallel audit fan-out, or second `workflow_run` framework.

The v0.5.0 release window starts at `2026-08-20T16:55:18Z`. #457/#468 begin retrospective provisional accepted. After #482 and #498 are accepted on `main`, #453, #454, and #455 refresh/rebase onto current `main` and deliver in that order. Their current stale #314 branch-global IssueOps failures are baseline drift, not dependency-regression evidence. #454 separately requires the complete Rust/database/SQLite implementation and review stack; campaign automation itself adds no Rust or storage boundary.

### Release acceptance composes rather than repairs

#492 freezes one exact revision only after every child and required review is complete and the exact live default branch resolves every accepted issue's mapped OpenSpec task source and architecture URL, heading, and Mermaid. It derives/reconciles the complete installed CLI command/nested-command and MCP tool inventory, then safely executes every route—including unchanged and administrative/mutating routes—against isolated fixtures. A holistic installed E2E spans binary/npm/plugin/host installation, database lifecycle, navigation, PHP/documents/analysis, worktrees/watchers, parser capability, update/repair/uninstall, concurrency/cancellation/failure/rollback. A separate publication hard gate starts from an exercised v0.4.5 installation and database, updates that same state to the exact candidate on every supported platform, preserves project identity, authored purposes, telemetry, worktree registrations, roots, generation, and source evidence, and proves injected-failure refusal, repair/retry, and compatible rollback without destructive reinitialization. Confirmed defects, including missing or stale published issue evidence, return to their specification or implementation owner and invalidate the candidate; #492 does not repair them. RC1 and stable are independently read back; v0.4.5 stays Latest until stable proof; #492 closes last.

## Risks / Trade-offs

- Native identity migration could bind an unrelated root. Mitigation: native equivalence, constraints, one transaction, concurrency/fault proof, and no implicit init.
- Parser/document inputs could exceed resources or invent evidence. Mitigation: single typed authorities, pinned/audited dependencies, pre-mutation refusal, exact locators, hard admission/output bounds, and typed incomplete state.
- Measurement issues could optimize fixtures instead of products. Mitigation: frozen representative shapes, plans/profiles, material thresholds, and valid no-change closure.
- Refactors could change contracts while moving code. Mitigation: accepted move maps, intermediate runnable owners, inventory comparison, and compatibility/fault/platform proof.
- Candidate-local specifications could be mistaken for durable issue evidence. Mitigation: publish through a non-closing planning PR, bind readiness and release gates to an exact clean live-default-branch snapshot, and read every mapped task source and architecture target back.
- Release proof could mutate ambient state, hide a route, or publish despite a broken v0.4.5 database update. Mitigation: isolated fixtures, independent inventories, behavior execution, an explicit pre-publication update gate with injected failure/retry/rollback, cleanup, and defect return.

## Migration Plan

Parallel delivery waves derived from the exact graph:

Before wave 1, the planning PR publishes the shared OpenSpec and architecture sources, the exact live default branch passes the published-readiness gate, and every accepted issue link/task mirror is read back. No Luna implementation worktree starts from candidate-only specification evidence.

1. Operational priority lands #497 first without inventing blocker edges for independent product issues.
2. After #497, #482 and #498 may proceed in parallel with independent #372 and #489; #498 is graph-blocked by #497 while #482 remains dependency-free.
3. After #482: #476, #480, #481, and #483 may proceed independently.
4. #484 follows #481; #486 follows #482/#483; #487 follows #481/#476/#480.
5. #488 follows #487.
6. #477 follows #476/#488; #342, #358, and #465 follow #476/#480/#488; #491 follows #487.
7. #339 follows #477; #384 follows #358/#477; #464 follows #358; #456 follows #358/#465/#477/#484; #485 follows #481/#482/#483/#484/#486; #388 and #390 follow #491.
8. #490 follows #339/#342/#358/#384/#464/#465/#485/#489/#491; #499 follows #482/#498 and delivers #453, #454, then #455 without serializing independent lanes.
9. #492 follows every child issue, runs full CI/campaign reconciliation, RC remediation, and stable proof, and closes last.

Each dependent worktree refreshes/rebases onto accepted predecessors on `main` and reruns affected proof. Independent, disjoint lanes remain parallel.

## Dependencies / Cross-Issue Impact

The authoritative direct blockers are exactly those in `openspec/issue-map.json.release_graphs.v0.5.0-00`. #492 is the hierarchy root and release-acceptance issue, not an implementation predecessor. #310/#314 remain v0.6 children of separate release owner #493.

Database-first transitions apply to #481, #484, #476 when a coverage gap is proven, #480, #465 when extraction state needs a delta, and #456 only if the baseline wins. Other issues preserve database/schema identity and prove continuity.

## Open Questions

None.
