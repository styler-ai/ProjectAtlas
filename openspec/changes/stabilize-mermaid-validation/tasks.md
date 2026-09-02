## 1. Contract and design

- [x] 1.1 Specify the bounded four-outcome Mermaid validation contract, exact timeout-only retry, fail-closed compatibility boundary, release-graph ownership, and existing architecture-view applicability.

## 2. Deterministic validation

- [ ] 2.1 Define one closed Mermaid parser outcome with valid, invalid, timed-out, and unavailable execution states; run each locked-parser attempt uncached with one fixed timeout, allow exactly one retry only after a timeout, and cache only the final bounded validation result.
- [ ] 2.2 Apply the outcome at the existing architecture-link boundary so a recovered valid diagram passes, terminal timeout and unavailable execution fail with their real class and target, and malformed, empty, missing, unsafe, or wrong-repository diagrams remain rejected without a new process framework or weakened gate.

## 3. Proof and reconciliation

- [ ] 3.1 Cover first-timeout recovery with two actual uncached runner calls, terminal timeout, invalid syntax and unavailable/broken execution with one call and no retry, bounded attempt count, target-specific diagnostics, and unchanged planned-issue and milestone validation through focused IssueOps tests.
- [ ] 3.2 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.
