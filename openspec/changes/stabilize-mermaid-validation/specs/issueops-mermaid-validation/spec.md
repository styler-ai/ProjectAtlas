## ADDED Requirements

### Requirement: IssueOps preserves bounded Mermaid parser failure classes
IssueOps SHALL validate architecture Mermaid blocks with the repository-locked parser and classify each parser attempt as valid, invalid, timed out, or unavailable. The locked Node validator SHALL use distinct stable process results for accepted syntax, rejected syntax, and dependency, bootstrap, or initialization failure; the IssueOps runner SHALL map those results without inspecting diagnostic prose. A first timeout SHALL receive exactly one retry of the same exact diagram under the existing fixed per-attempt ceiling. Invalid syntax and unavailable execution SHALL fail without retry. One fixed monotonic 120-second deadline SHALL bound all Mermaid parser attempts in an IssueOps validation run; no new diagram SHALL start unless the remaining run budget can admit both bounded attempts, and exhaustion SHALL fail with a timeout-specific target diagnostic. No timeout or execution failure SHALL be reported as invalid source syntax or treated as success.

#### Scenario: A transient parser timeout recovers
- **WHEN** the first locked-parser attempt times out and the single retry accepts the same exact diagram
- **THEN** the architecture target passes without weakening any link, heading, repository, or diagram requirement

#### Scenario: The parser times out twice
- **WHEN** both bounded attempts time out
- **THEN** validation fails with a timeout-specific diagnostic naming the architecture target

#### Scenario: The validation-wide parser budget is exhausted
- **WHEN** serial Mermaid blocks consume the fixed validation-run parser budget
- **THEN** IssueOps starts no additional parser process and fails with a timeout-specific diagnostic naming the current architecture target before the workflow timeout is exhausted

#### Scenario: Syntax is invalid
- **WHEN** the locked parser rejects the diagram as invalid syntax
- **THEN** validation fails with an invalid-syntax diagnostic naming the architecture target and performs no timeout retry

#### Scenario: Parser execution is unavailable
- **WHEN** the locked parser cannot start or its required package, DOM bootstrap, or Mermaid initialization fails before syntax validation
- **THEN** validation fails with an unavailable-execution diagnostic naming the architecture target and performs no timeout retry

#### Scenario: Existing architecture admission remains fail-closed
- **WHEN** an architecture target is malformed, empty, missing, unsafe, points at another repository, has no matching heading, or has no accepted non-empty Mermaid block
- **THEN** IssueOps rejects it under the existing bounded architecture contract

#### Scenario: Parallel workspace proof does not duplicate the real parser self-test
- **WHEN** the parallel Rust workspace gate verifies the IssueOps and workflow quality contract
- **THEN** it verifies that the explicit IssueOps self-test remains wired into the required local and hosted gates without launching a duplicate locked-parser self-test process, while the explicit IssueOps self-test continues to exercise the real locked validator
