## 1. Specification and Architecture

- [x] 1.1 Finalize and map the sanitized Windows fixture-readiness contract, accepted complexity, v0.5 release relationships, non-goals, failure modes, and exact implementation/test ownership before implementation.
- [x] 1.2 Add a focused Codex MCP owner fixture readiness/cleanup architecture view to the existing v0.5 release architecture document; render it with the locked Mermaid toolchain and inspect visual communication plus semantic truth.

## 2. Bounded Fixture Readiness

- [x] 2.1 Replace the hard-coded five-second child-PID publication assumption at the smallest owning Windows delivery-test helper with one named bounded readiness contract; preserve 25 ms polling or an equally bounded existing mechanism, parent early-exit detection, atomic identity publication, exact PID/start-time/executable-path validation, actionable timeout diagnostics, and complete owned-process cleanup.
- [x] 2.2 Add deterministic regression proof whose valid publication exceeds the former five-second ceiling plus negative coverage for true timeout, early parent exit, malformed or mismatched identity, and cleanup; do not weaken or duplicate the existing production-installer behavior assertions.

## 3. Integration and Acceptance

- [x] 3.1 Run the focused fixture and installer E2E, repeated required parallel workspace gate, strict Rust/workspace/OpenSpec/IssueOps checks, and exact-head hosted Windows proof; refresh dependent #487 only after the accepted fix is on `main`, and confirm it retains one final helper owner.
- [x] 3.2 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.
