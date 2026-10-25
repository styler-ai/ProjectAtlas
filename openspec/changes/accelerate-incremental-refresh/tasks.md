# Tasks

## 1. Publish the plan

- [x] 1.1 Finalize the complete 0.5.1 indexing/refresh proposal, design, requirements, architecture and exact #708 task mirror; synchronize milestone, sole parent and blocker graphs with #715 and #493, then pass independent specification/visual review, OpenSpec, Mermaid and IssueOps before implementation.

## 2. Implement and prove the complete indexing improvements

- [ ] 2.1 Settle the shared indexing architecture and work-count guarantees from current code. Use a small optimized 0.5.0 reference baseline to set practical speed/resource targets for the required paths, and resolve cold-CLI history/fallback before production changes; defer the broad platform matrix to release acceptance.
- [ ] 2.2 Implement lightweight ordered source comparison and one affected-work plan in the existing shared owners, reusing valid source evidence and avoiding full-node decoding or repeated discovery. Verify the slice with causal work-count and real SQLite read/write/rollback tests, bounded workers/memory and documented wider invalidation.
- [ ] 2.3 Optimize initial indexing, full scan and explicit symbol/graph rebuild in their shared owners. Meet their measured speed/throughput/memory targets and verify complete selection, parser/document/projection results, existing rebuild semantics, publication races and preserved authored state.
- [ ] 2.4 Reuse change tracking and incremental updates safely in CLI, MCP and continuous watch. Meet cold-CLI and warm no-op targets; test arbitrary mixed changes, missed/offline/same-metadata edits, policy changes, concurrent writes, cancellation, retry and exact fallback.
- [ ] 2.5 Select work-based defaults at the shared indexing boundary while retaining the 1,800-second ceiling, larger-request clamp and unknown-work fallback. Test estimates, explicit-limit precedence, zero/unrepresentable refusal and cancellation; update affected help, schemas and guidance together without treating longer timeouts as a speedup.

## 3. Accept the complete 0.5.1 behavior

- [ ] 3.1 Run the preregistered optimized installed matrix on Windows, Linux and both Mac architectures with normal telemetry. Meet each initial/full/rebuild/refresh latency, throughput, work-count and resource target; prove complete results, preserved state and atomic failure recovery, with no material regression in unaffected navigation or supported upgrades.
- [ ] 3.2 Resolve independent review and automated feedback, pass the required Rust/OpenSpec/IssueOps/documentation/installed-platform gates, and reconcile the final implementation with the architecture and user-facing guidance before closing #708.

## Required verification routes

Architecture decisions and work guarantees come first. Use the smallest causal tests and focused before/after measurements for each compiling slice; keep the broad installed/platform matrix for integrated release acceptance. The repository's affected-check planner and normal push hook remain authoritative. Rust changes retain applicable locked workspace formatting, compilation, Clippy, unit/integration/E2E tests, doctests and warning-clean rustdoc. Every implementation task owns its behavior tests and documentation; do not repeat unchanged passing proof without a failure, uncovered behavior, changed input or mandatory gate.
