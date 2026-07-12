## Why

ProjectAtlas currently runs Rust tests and doctests in CI but does not enforce independently reported nextest, source-coverage, or source-mutation quality gates. A pinned local all-feature observation is 24,130 of 27,495 lines (87.76%), 34,041 of 40,094 regions (84.90%), and 2,045 of 2,369 functions (86.32%), so calling the suite "100% tested" would be false; the repository needs blocking honest floors plus a monotonic path toward near-complete meaningful coverage and mutation strength. This local observation is not a hosted platform floor or release result.

## What Changes

- Add separate blocking CI jobs for `cargo nextest`, stable `cargo test --doc`, `cargo llvm-cov`, and `cargo mutants` instead of treating one test command as evidence for every quality dimension.
- Check in pinned tool versions, explicit command/job timeouts, deterministic nextest policy, machine-readable coverage floors/targets, and machine-readable mutation exclusions with required rationale and expiry. The v0.4 hard targets are 98% raw/100% adjusted lines, 95% raw/98% adjusted regions, 98% raw/100% adjusted functions, and 90% raw/95% adjusted viable mutation kills.
- Establish the measured baseline as an initial no-regression ratchet, reject threshold decreases or silent scope/exclusion growth, and raise floors only from retained passing evidence.
- Run bounded changed-source mutation testing for pull requests and scheduled/manual full-workspace mutation shards, retaining reports and failing on missed or timed-out viable mutants according to the declared policy.
- Keep doctests on stable `cargo test --doc`; do not rely on unstable llvm-cov doctest instrumentation or claim source mutation duplicates repository mutation/fault fixtures.
- Add self-tests for the gate policy, threshold comparison, exclusion validation, empty-test/mutant detection, shard coverage, stale evidence, and failure propagation.
- Configure GitHub IssueOps and local/CI/PR/release validation so every OpenSpec checkbox declares a stable task-specific unit-test identifier, links a successful commit-bound run before completion, stays synchronized with its authoritative GitHub checkbox, and uses deterministic disjoint phase ledgers when GitHub's body limit prevents one issue from carrying the complete checklist and evidence.
- Execute this repository-wide quality program only after the repository-intelligence architecture, refactoring, migrations, public contracts, and feature behavior stabilize. New stable behavior still receives focused unit, integration, CLI/MCP E2E, and affected-platform smoke tests when implemented; this sequencing defers only legacy/refactored saturation, final adjusted-coverage closure, the complete mutation campaign, and final evidence.
- Link this change and its GitHub issue as a final v0.4 release prerequisite for repository-intelligence delivery, not as a prerequisite to implement repository-intelligence functionality.
- **Non-goals:** no fabricated 100% claim, no lowering existing Rust/Clippy/rustdoc gates, no blanket source exclusions, no retry-to-green policy, no requirement that every generated or unreachable defensive branch be covered without reviewed evidence, and no repository-intelligence graph implementation in this change.
- This change is ready for implementation after strict OpenSpec validation, issue/checklist mapping, and stabilization of the repository-intelligence feature architecture it will measure.

## Capabilities

### New Capabilities
- `rust-test-quality-gates`: Independent nextest, doctest, LLVM coverage, and source-mutation policies with blocking CI, retained evidence, validated exceptions, and monotonic ratchets.

### Modified Capabilities

None.

## Impact

- **CI:** `.github/workflows/ci.yml` gains bounded independent quality jobs; the release path consumes their successful evidence rather than rerunning an ambiguous aggregate test command.
- **Configuration:** repository-owned nextest, coverage/ratchet, and cargo-mutants policies become the single machine-readable source for local and hosted gates, with 90-day artifact retention covering the 30-day release-decision window.
- **Automation:** a small validator checks schema, measured results, exclusions, shard completeness, monotonic thresholds, task-specific unit-test/run evidence, authoritative issue ownership, and local/GitHub checklist synchronization before check-in, PR review, status/closure, release, or CI acceptance.
- **Developer workflow:** local commands mirror CI exactly and report which quality dimension failed.
- **Dependencies:** CI installs pinned `cargo-nextest`, `cargo-llvm-cov`, `llvm-tools-preview`, and `cargo-mutants`; no runtime crate dependency or product API changes.
