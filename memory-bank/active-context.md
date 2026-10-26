# Active context

## Objective

Deliver the scheduled 0.5.1 work and publish `v0.5.1-rc1` for testing. Preserve the preceding stable release as GitHub Latest. Stable promotion is not authorized by this objective.

The user explicitly renewed the instruction to work until RC1 is released. Keep the goal active through implementation, review, required local/hosted checks, publication, and independent readback; do not stop at a partial slice.

## Latest user direction

- Index only the required or invalidated scope. Reuse verified source bytes, text, parser facts and graph state. Avoid repeated whole-tree discovery/hashing and unnecessary full builds; do not weaken source/writer race safety to achieve this.
- Do not get stuck cycling between tool calls. Use bounded calls, follow ranked evidence, avoid repeating passing checks without cause, and communicate useful progress.
- Sleep prevention is enabled in global Codex `config.toml` as `features.prevent_idle_sleep = true`. Native 0.164.0 Linux support and an actual temporary systemd idle inhibitor were verified. Existing app processes may need restart to read the new setting.

## Baseline and history

- The integration worktree starts at `7cd5a79` on `release/0.5.1`, from accepted `main`.
- Stable 0.5.0 shipped PHP navigation, bounded PDF/DOCX extraction, the short `atlas` CLI, bounded analysis views, worktree routing, Unicode search, coordinated plugin/runtime installation, and preserved-state atomic publication.
- Follow-up planning moved complete indexing performance from 0.6.0 into 0.5.1. Language navigation, Memory Atlas product implementation, and npm distribution remain 0.6.0 work.
- The stable 0.5.0 native Linux archive was verified against published SHA256SUMS and used to initialize each worktree's private index. The official Codex marketplace plugin, runtime, generated configs, and MCP registry were verified at 0.5.0. A bounded stdio handshake reported 43 tools and the correct server version. The active host has not exposed the new MCP tools yet; the user explicitly prefers the installed plugin's CLI workflow.

## Release scope

- #708: indexing, full scan, symbol/graph rebuild, one-shot and continuous refresh performance, trustworthy no-op and exact fallback, adaptive cooperative budgets. OpenSpec: `accelerate-incremental-refresh`.
- #721: bounded useful relation navigation, compatibility behavior, row/output/work budgets and continuation. Currently milestone backlog; its implementation plan and graph mapping must be completed before implementation.
- #718: protected-branch check guidance; already present on the baseline. Verify live closure before release.
- #715: release acceptance and publication, closes last after stable verification. OpenSpec: `complete-v051-release-acceptance`.
- The local release graph currently omits #721 despite its live milestone membership. Reconcile it as part of planning.

## Working rules

The primary agent implements and fixes. Independent reviews use GPT-6 Luna at max effort and explicitly read the custom Rust skill, applicable Rust subskills, database/SQLite skills when relevant, and accepted OpenSpec. Reviewers do not edit. Use `docs/workflow.md` for the native IssueOps, quality, PR, and release contracts.

## Current implementation

`runtime.rs` owns shared scan/rebuild/refresh orchestration. One-shot refresh compares a complete scan against fully decoded stored nodes, then stages a full publication for changes. Continuous watch already has an incremental path. `runtime/source_observation.rs` owns process-local verified-read and mutation evidence. Initial/full staging independently scans, stages text, parses symbols, builds graph projections, and revalidates source.

Issue worktree: `.worktrees/issue-708`, branch `perf/indexing-708`. Its base includes memory-bank commit `f39cd61`. Preregistered reference/targets are in `docs/benchmarks/v0.5.1-indexing.md`, initially committed as `4d4572a` before production edits. Stable optimized 0.5.0 median seconds: initial 5.257, full 6.928, explicit rebuild 6.589, cold no-op 0.0469, sparse edit 6.606. Cold exact discovery is viable, so no retained-history daemon is proposed. #708 architecture task 2.1 and source-comparison/evidence task 2.2 are checked locally and in the live issue after proportional proof and independent Luna Max review. Later implementation/acceptance tasks remain unchecked.

First slice: a cancellation-aware source-only ordered SQLite cursor (joining purposes only for row existence), linear delta comparison with bounded retained paths and explicit completeness, source-only node loads for rebuild/incremental staging, cold explicit refresh routed to the existing incremental owner, typed full fallback for explicit/continuous refresh limits, and reuse of the existing bulk read-index rebuild policy for full graph writes. Deleted folders explicitly select full discovery to avoid a recursive fallback. Preserve exact publication validation until an equivalent writer/source witness is proven. MCP observed no-op integration and adaptive budgets remain pending.

The diagnostic stable scan of the ~747 KB source fixture performed 1.6 million pwrite64 and 617k pread64 syscalls; most writes target a SQLite temporary journal and WAL. Per-file symbol batching showed only ~1.7% difference and was removed. Kernel perf sampling was unavailable; no system permissions were changed. The measured bulk-index approach retains cascade lookup indexes during deletion, then drops/rebuilds only known derived read indexes for insertion, inside the same rollback boundary.

First repeated optimized results (Linux reference only): initial median 3.770 s (-28.3%), full 5.373 s (-22.4%), explicit rebuild 5.039 s (-23.5%), sparse 0.144 s (-97.8%, ~46x), cold no-op ~43.8 ms. Sparse maximum RSS 24,208 KiB versus 174,424 KiB baseline. All full paths still parse all candidates. Full supported-platform/large-fixture acceptance remains unproven.

Stable first-slice commit: `bd08c6b`. Independent Luna Max review found no correctness/integrity/compatibility blocker. Its required oversized-batch full-fallback regression was added in `a6d54c9` and passed: all actual files, including a late file outside the event set, publish in one generation while identity/purpose survive.

The current uncommitted slice retains exact discovered nodes with database-instance, root, contract and generation fences, consumes them in incremental staging, and queries downstream stored state only for affected paths. Built-in parsing borrows hash-verified raw UTF-8 staged text; documents, optional workers and incompatible/missing text retain bounded independent reads. Final exact validation remains. Luna accepted the design and caught a publication race: generation now comes from the same publication record whose state/contract was validated. The deterministic competing-policy regression passes.

MCP synchronous/background one-shot refresh now reuses verified observation for no-op admission without repair. `SourceReadAdmission` transfers a boxed opaque exact assessment to the same mutable refresh owner on a delta, avoiding a second detection scan. Reuse rechecks the durable database instance as well as root/contract/generation; final source validation remains. Tests cover zero warm discovery/bytes/node decoding, external publication invalidation, no hidden repair, real changed-work reports, snapshot-to-writer handoff, and a competing publication before handoff consumption. Missing DB fallback uses the existing initialized-store guard. CLI one-shot/continuous watch also refuses absent DB state; its E2E check passed without creating any project state.

Latest complete checks before the storage guard: all 351 CLI main tests, workspace Clippy, `cli-core` binary feature check, strict strings, doctests, warning-clean rustdoc, strict OpenSpec and scoped live IssueOps passed. Luna Max accepted the source handoff and found one guard edge case: zero-byte DB files were implicitly initialized. This is now fixed by storage-owned `open_existing_for_project` / `ExistingSchema` policy, with read-only Fresh refusal before writable setup and a migration transaction recheck. CLI/MCP use the strict opener; Fresh read-only state maps to typed initialization guidance. Supported schema20 migration remains available through MCP's narrowly gated migration fallback. Focused DB/current/predecessor, empty-file CLI once/continuous, and MCP empty-file/migration tests pass. Luna Max reviewed the fix and found no blocker. Fresh replacement during a supported migration can still acquire WAL mode before its transaction refuses schema creation; API docs promise preflight refusal, not stronger external file-replacement fencing.

A clean-PATH locked workspace verification completed across the full run and affected recovery: CLI library/main/worker/release unit targets passed; delivery had 49 passing cases plus three inventory cases rerun successfully after registering the new lifecycle test and current digests; lifecycle, maintenance and navigation passed; worktrees had 16 passing cases plus the notify case rerun successfully after explicit fixture initialization before adding source. All remaining native integration and other workspace targets passed, including all 238 DB tests. Doctests, warning-clean rustdoc, strict strings and final workspace Clippy passed. Conditional artifact/platform tests remain explicitly ignored pending release-owned installed proof. Logs are `/tmp/projectatlas-issue708-*.log`. Luna Max reviewed both final fixture/inventory changes and found no blocker. The verified significant slice is committed as `1ae6626`; adaptive-budget implementation is next. Own unpublished history now has public author identity and consistent #708 ownership, with the same source trees and preregistration order.

Sibling #721 now has a draft `bound-relation-navigation` OpenSpec packet and local graph/owner mapping in `.worktrees/issue-721`; strict OpenSpec passes. Independent Luna Max specification review requested explicit legacy omitted rows (50) and unchanged explicit analysis defaults (50 rows/256 KiB), now added to proposal/spec/design and draft issue body. Native #715 children/blockers still contain only #708/#718; #721 live body/status/parent/blocker synchronization remains pending. No #721 product source changed.

Adaptive budgets, refreshed broad performance/platform acceptance, #721 and release work remain incomplete. Implementation tasks 2.3 onward remain unchecked until their complete proof/review is accepted.

## Verification and access

- Rust 1.98.0 with Cargo, Clippy, and rustfmt is installed from the repository toolchain pin.
- Baseline locked all-target/all-feature workspace check passed. Source-cursor, mixed-change/same-size/same-mtime and no-op generation/cleanup tests passed; all 59 runtime-module tests passed. Full database suite passed before the final bulk-index edit; its updated atomic reader/rollback test passed, including exact index-definition restoration. Full workspace gates and independent diff review are next; inspect current logs rather than assuming completion.
- Workspace fmt/check/Clippy passed at the first checkpoint. The clean-PATH full workspace rerun passed through all product targets and reached the final lints target, where a machine-specific path in this memory file failed the private-path check. The path is corrected; this gate also inspects committed HEAD, so commit the correction before retrying. Doctests and warning-clean rustdoc remain pending. The installed Atlas/Python fixture collisions were resolved through the clean system PATH plus existing Python venv, without changing the user's installation.
- A bare no-default-features all-target check is not the CLI feature matrix: its integration tests require CLI dependencies. Use the actual `cli-core` binary feature combination for the non-optional-parser branch. That branch and current Clippy/docs still need verified results.
- Stable 0.5.0 installer repair completed successfully for this exact issue checkout, preserved its existing database, and verified native runtime, plugin, generated host files and Codex MCP registry.
- Use `TMPDIR=$HOME/.cache/projectatlas-test-tmp` for filesystem-backed tests: `/tmp` is unsupported tmpfs for production WAL admission; a TMPDIR inside the repository inherits Git routing/ignores and corrupts non-Git fixture assumptions. Keep tests outside Git on ext4. Shared build cache: `.worktrees/v0.5.1/target` through `CARGO_TARGET_DIR`.
- Local diagnostics/results live in `/tmp/projectatlas-issue708-*.log` and `/tmp/projectatlas-v050-index-reference.json`; reference fixtures live in the exact `.cache/projectatlas-benchmarks` subdirectories. Do not index their parent/home.
- GitHub CLI authentication now succeeds as styler-ai, stored in the OS keyring with repo/workflow scopes. Connected GitHub reads/writes also work. No RC is published yet.
- Sibling `.worktrees/issue-721` / `perf/relations-721` is initialized and indexed with stable 0.5.0, but its OpenSpec plan and source implementation are still pending. #718 is live-verified closed. #721 remains backlog and is missing from the local release graph; planning must reconcile native/local membership before implementation.
