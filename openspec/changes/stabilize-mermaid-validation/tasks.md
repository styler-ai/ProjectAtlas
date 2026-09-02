## 1. Contract and design

- [x] 1.1 Specify the bounded four-outcome Mermaid validation contract, exact timeout-only retry, aggregate validation-run deadline, fail-closed compatibility boundary, release-graph ownership, and existing architecture-view applicability.

## 2. Deterministic validation

- [x] 2.1 Give the locked Node validator distinct stable results for accepted syntax, rejected syntax, and dependency/bootstrap/initialization failure; map them into one closed valid, invalid, timed-out, or unavailable IssueOps outcome without diagnostic-text heuristics; run each attempt uncached under the fixed per-attempt ceiling and shared validation-run deadline, retry exactly once only after an admitted timeout, launch no parser after aggregate budget exhaustion, and cache only the final bounded result.
- [x] 2.2 Apply the outcome at the existing architecture-link boundary so a recovered valid diagram passes, terminal timeout and unavailable execution fail with their real class and target, and malformed, empty, missing, unsafe, or wrong-repository diagrams remain rejected without a new process framework or weakened gate.

## 3. Proof and reconciliation

- [x] 3.1 Cover first-timeout recovery with two actual uncached runner calls, terminal timeout, aggregate validation-run budget exhaustion without another process launch, invalid syntax and unavailable/broken execution with one call and no retry, bounded attempt count, target-specific diagnostics, unchanged planned-issue and milestone validation, and one real locked-validator process check in each explicit IssueOps gate that distinguishes syntax rejection from a controlled dependency/bootstrap failure while the parallel Rust workflow-contract test verifies that wiring without launching a duplicate parser self-test.
- [x] 3.2 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.
