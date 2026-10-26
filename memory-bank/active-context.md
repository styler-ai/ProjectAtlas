# Active context

## Objective

Deliver the scheduled 0.5.1 work and publish `v0.5.1-rc1` for testing. Preserve the preceding stable release as GitHub Latest. Stable promotion is not authorized by this objective.

## Baseline and history

- The integration worktree starts at `7cd5a79` on `release/0.5.1`, from accepted `main`.
- Stable 0.5.0 shipped PHP navigation, bounded PDF/DOCX extraction, the short `atlas` CLI, bounded analysis views, worktree routing, Unicode search, coordinated plugin/runtime installation, and preserved-state atomic publication.
- Follow-up planning moved complete indexing performance from 0.6.0 into 0.5.1. Language navigation, Memory Atlas product implementation, and npm distribution remain 0.6.0 work.
- The stable 0.5.0 native Linux archive was verified against published SHA256SUMS and used to initialize this worktree's private index. The official Codex marketplace plugin was installed from `v0.5.0`; runtime/registry convergence is being verified.

## Release scope

- #708: indexing, full scan, symbol/graph rebuild, one-shot and continuous refresh performance, trustworthy no-op and exact fallback, adaptive cooperative budgets. OpenSpec: `accelerate-incremental-refresh`.
- #721: bounded useful relation navigation, compatibility behavior, row/output/work budgets and continuation. Currently milestone backlog; its implementation plan and graph mapping must be completed before implementation.
- #718: protected-branch check guidance; already present on the baseline. Verify live closure before release.
- #715: release acceptance and publication, closes last after stable verification. OpenSpec: `complete-v051-release-acceptance`.
- The local release graph currently omits #721 despite its live milestone membership. Reconcile it as part of planning.

## Working rules

The primary agent implements and fixes. Independent reviews use GPT-6 Luna at max effort and explicitly read the custom Rust skill, applicable Rust subskills, database/SQLite skills when relevant, and accepted OpenSpec. Reviewers do not edit. Use `docs/workflow.md` for the native IssueOps, quality, PR, and release contracts.

## Current implementation checkpoint

This is `.worktrees/issue-721`, branch `perf/relations-721`, with uncommitted
implementation and the accepted `bound-relation-navigation` plan. Luna Max accepted
the specification and independently reviewed the storage/adapter implementation.
Local/live #721 tasks 1.1, 2.1, 2.2 and 2.3 are checked; task 3.1 and all
acceptance rows remain pending hosted/final proof. The task mirror is being verified.

The storage-owned compatibility reader admits at most 200 rows, 100,000 SQLite VM
instructions, 1,000 ms and 1 MiB intermediate state. ID-only candidate selection
refuses its overflow sentinel before hydration; complete small results retain their
ordering. RAII restores limits/progress hooks and respects caller transactions.
No schema change; existing rusqlite limits feature is enabled. Full DB suite:
237 tests plus doctests passed, with focused query-plan/limits/cleanup tests.

CLI and MCP default to existing detailed ten-row/32-KiB pages. Explicit legacy
retains complete small arrays, omitted rows 50, hard rows/output bounds and typed
read/output refusals. Shared service policy owns defaults; adapters enforce final
encoded UTF-8 bytes (including MCP routing wrapper). Real JSON/TOON/MCP test covers
exact N/N-1 output limits, invalid controls, typed work, continuation and isolation.
Workspace Clippy and warning-clean docs passed; new real-adapter E2E passed.

The first full workspace run exposed two obsolete implicit-legacy unit probes;
both were corrected and passed affected reruns. First delivery integration run
passed 49 cases and exposed three intentional guidance/default compatibility
expectations. Narrow normalization, exact advertised-schema digest and explicit
legacy array probes now pass all three targeted reruns. Frozen historical fixtures
remain unchanged. E2E inventory positive and two negative contracts pass. The remaining
platform/library/workspace/doctest checks passed (artifact-only ignored contracts
remain release-owned). The offline/worktree legacy probes were corrected and
passed their affected reruns. Logs `/tmp/projectatlas-issue721-*`.
Reuse unchanged passing proof; normal push hook remains mandatory.

Rebased onto accepted main 9845364 (PR #723 merged after all four platforms and
required verify passed). The owned backup stash is retained. Coordinate
coordinate #708 shared runtime/MCP/guidance/E2E inventory changes. No #721 commit or PR yet. #708 PR #723 merged normally at 9845364 after
all four platforms, Rust, repository, PR-state and required verify passed; automated
review had no findings. Its restored adaptive-budget draft is in the
sibling issue-708 checkout on perf/indexing-budgets-708, under independent review.

## Access and release limits

GitHub CLI 2.103.2 is authenticated as styler-ai through the OS keyring. Use clean
test PATH (rustup proxies, task gh-only helper, existing Python venv), filesystem
TMPDIR `~/.cache/projectatlas-test-tmp`, and shared integration target. Never compile
siblings into that target during CLI E2E tests. Stable Atlas 0.5.0 remains installed;
this checkout received typed full-refresh guidance after the dependency closure
limit, and that required full refresh completed without resetting its database.
The following rebase/edits invalidate prior indexed facts; refresh only when needed.

RC1 is not released. Exact one-shot reconciliation fixes the native-notification
race; no warm zero-source-work claim is accepted. A user scope question is pending
before deferring that promised target. Continue all independent work toward RC1.

## Integrated task proof

On accepted main 9845364, locked all-target workspace check, Clippy, warning-clean
docs and strict string policy pass. The inventory now retains both issue slices
(207 test entries, frozen historical metadata unchanged). Real-adapter reruns pass;
MCP now consumes its own continuation to terminal and compares complete relation
coverage with CLI, rejects a changed root digest and cross-project replay with the
same anchor. Luna approved these added causal assertions. Final strict OpenSpec
and scoped IssueOps are running before the sole-owner commit/push. Normal native
pre-push remains mandatory. Do not report #721 accepted or close it before hosted
and final issue gates.

## Publication checkpoint

Local commit 8279cfa owns #721 implementation on accepted main 9845364; it is not
pushed yet. The first native push hook passed planner/IssueOps/string policy but
stopped at newly fetched RustSec advisories RUSTSEC-2026-0391/0398/0403 affecting
zstd0.14.0/zstd-safe8.0.0. The primary updated the exact workspace zstd pin to
0.14.3; the lock moves only zstd, zstd-safe8.2.0 and zstd-sys2.1.2. No ignore or
exception was added. Fresh dependency review/gate and normal complete push proof
must pass before publishing. Earlier Rust evidence predates this dependency fix.
Task2.2/2.3 mirror is confirmed: local5/remote5/checked4; strict OpenSpec passes.
