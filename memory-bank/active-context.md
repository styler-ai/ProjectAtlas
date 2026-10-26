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

## Current investigation

`runtime.rs` owns shared scan/rebuild/refresh orchestration. One-shot refresh compares a complete scan against fully decoded stored nodes, then stages a full publication for changes. Continuous watch already has an incremental path. `runtime/source_observation.rs` owns process-local verified-read and mutation evidence. Initial/full staging independently scans, stages text, parses symbols, builds graph projections, and revalidates source.

Before production edits, settle the shared work plan and cold-CLI exact-discovery contract using optimized 0.5.0 reference measurements. Do not infer historical edits from a fresh empty event queue, or freshness from timestamps alone. Preserve race checks, authored purposes, identity, and atomic publication.

## Verification and access

- Rust 1.98.0 with Cargo, Clippy, and rustfmt is installed from the repository toolchain pin.
- No product implementation or completed performance claim yet.
- Connected GitHub reads work. The local `gh` command reports no authentication; hosted workflow operations need a supported authenticated route before publication. Continue local implementation and verification independently.
