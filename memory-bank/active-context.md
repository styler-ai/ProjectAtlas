# Active context

## Objective

Deliver the scheduled 0.5.1 work and publish `v0.5.1-rc1` for testing. Preserve the preceding stable release as GitHub Latest. Stable promotion is not authorized by this objective.

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

Issue worktree: `.worktrees/issue-708`, branch `perf/indexing-708`. Its base includes memory-bank commit `1fe2b86`. Preregistered reference/targets are in `docs/benchmarks/v0.5.1-indexing.md`, initially committed as `d998199` before production edits. Stable optimized 0.5.0 median seconds: initial 5.257, full 6.928, explicit rebuild 6.589, cold no-op 0.0469, sparse edit 6.606. Cold exact discovery is viable, so no retained-history daemon is proposed. #708 task 2.1 is checked locally and in the live issue after Luna's independent architecture review; implementation tasks remain unchecked.

First slice: a cancellation-aware source-only ordered SQLite cursor (joining purposes only for row existence), linear delta comparison with bounded retained paths and explicit completeness, source-only node loads for rebuild/incremental staging, cold explicit refresh routed to the existing incremental owner, typed full fallback for explicit/continuous refresh limits, and reuse of the existing bulk read-index rebuild policy for full graph writes. Deleted folders explicitly select full discovery to avoid a recursive fallback. Preserve exact publication validation until an equivalent writer/source witness is proven. MCP observed no-op integration and adaptive budgets remain pending.

The diagnostic stable scan of the ~747 KB source fixture performed 1.6 million pwrite64 and 617k pread64 syscalls; most writes target a SQLite temporary journal and WAL. Per-file symbol batching showed only ~1.7% difference and was removed. Kernel perf sampling was unavailable; no system permissions were changed. The measured bulk-index approach retains cascade lookup indexes during deletion, then drops/rebuilds only known derived read indexes for insertion, inside the same rollback boundary.

First repeated optimized results (Linux reference only): initial median 3.770 s (-28.3%), full 5.373 s (-22.4%), explicit rebuild 5.039 s (-23.5%), sparse 0.144 s (-97.8%, ~46x), cold no-op ~43.8 ms. Sparse maximum RSS 24,208 KiB versus 174,424 KiB baseline. All full paths still parse all candidates. Full supported-platform/large-fixture acceptance remains unproven.

## Verification and access

- Rust 1.98.0 with Cargo, Clippy, and rustfmt is installed from the repository toolchain pin.
- Baseline locked all-target/all-feature workspace check passed. Source-cursor, mixed-change/same-size/same-mtime and no-op generation/cleanup tests passed; all 59 runtime-module tests passed. Full database suite passed before the final bulk-index edit; its updated atomic reader/rollback test passed, including exact index-definition restoration. Full workspace gates and independent diff review are next; inspect current logs rather than assuming completion.
- Use `TMPDIR=/home/utopien/.cache/projectatlas-test-tmp` for filesystem-backed tests: `/tmp` is unsupported tmpfs for production WAL admission; a TMPDIR inside the repository inherits Git routing/ignores and corrupts non-Git fixture assumptions. Keep tests outside Git on ext4. Shared build cache: `.worktrees/v0.5.1/target` through `CARGO_TARGET_DIR`.
- Local diagnostics/results live in `/tmp/projectatlas-issue708-*.log` and `/tmp/projectatlas-v050-index-reference.json`; reference fixtures live in the exact `.cache/projectatlas-benchmarks` subdirectories. Do not index their parent/home.
- GitHub CLI authentication now succeeds as styler-ai, stored in the OS keyring with repo/workflow scopes. Connected GitHub reads/writes also work. No RC is published yet.
