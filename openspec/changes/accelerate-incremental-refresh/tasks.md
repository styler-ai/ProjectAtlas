# Tasks

## 1. Publish the plan

- [x] 1.1 Finalize the proposal, design, requirements, architecture, exact #708 task mirror and v0.6.0 release declaration; verify OpenSpec, Mermaid and IssueOps before publishing the plan.

## 2. Implement and prove the shared refresh improvements

- [ ] 2.1 Measure optimized CLI/MCP on large projects across supported platforms, record timings and work counts, and set speed/resource/budget targets; before production changes, prove a feasible cold-CLI target or publish a reviewed design for reliable history across invocations.
- [ ] 2.2 Implement the measured filesystem/source-comparison and affected-data foundations in their existing owners; prove mixed add/edit/delete/rename sets and unchanged-source work counts with causal tests and real SQLite read, write and rollback checks, and document any justified wider invalidation.
- [ ] 2.3 Reuse existing change tracking and incremental updates safely in CLI/MCP; meet the selected cold-CLI target and warm MCP no-op bounds, test missed/offline/same-metadata edits, concurrent changes, cancellation and retry, and document required fallback.
- [ ] 2.4 Add the bounded work-based default budget at the existing work-control boundary; verify estimator bounds, explicit caller-limit precedence, zero/unrepresentable refusal and cancellation through unit and CLI/MCP tests, and update the affected help, schemas and guidance together.

## 3. Accept the complete v0.6.0 behavior

- [ ] 3.1 Run the preregistered installed performance and correctness matrix on Windows, Linux and both macOS architectures; meet the latency/resource and affected-work targets, preserve authored state and atomic recovery, and show no material regression in full indexing, unaffected navigation or supported upgrades.
- [ ] 3.2 Resolve independent review and automated feedback, pass the required Rust/OpenSpec/IssueOps/documentation/installed-platform gates, and reconcile the final implementation with the architecture and user-facing guidance before closing #708.

## Required verification routes

The repository's affected-check planner and normal push hook remain authoritative. Rust changes require the applicable locked workspace checks, including `cargo fmt --check`, `cargo check --workspace --all-targets --all-features --locked`, `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`, `cargo test --workspace --all-features --locked`, `cargo test --doc --workspace --all-features --locked`, and warning-clean rustdoc. Run the smallest causal tests first; every implementation task owns its behavior tests and documentation.
