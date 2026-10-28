## ADDED Requirements

### Requirement: The evaluation has current release and task ownership
#490 SHALL belong to v0.6.0 under sole parent #493, SHALL directly block #493, and SHALL be directly blocked by #310 and #563. The issue mirror, this change and the declared/native graph SHALL agree. The old v0.5 task slice SHALL remain explicitly deferred history rather than current or completed work.

#### Scenario: Planning reconciliation
- **WHEN** the v0.6 evaluation is admitted as ready
- **THEN** it has one current task mapping and matching milestone, parent, blockers and release-owner scope, while v0.5.1-rc1 gains no evaluation dependency

### Requirement: Inputs and outcome rules are frozen before observation
The evaluation SHALL preregister representative tasks from accepted v0.6 behavior, repositories, baseline/candidate/runtime/tool identities, equivalent arm prompts and capabilities, success and stop rules, cold/warm state, rotated ordering, repeats, measurements, uncertainty, privacy and output bounds before running either arm.

#### Scenario: Candidate changes or a task is unsupported
- **WHEN** a candidate input changes or the accepted contract cannot support a scenario
- **THEN** affected proof is invalidated or the preregistered exclusion is reported explicitly; neither arm gains an undisclosed advantage and no unsupported behavior is claimed

### Requirement: All outcome classes remain available and reports are honest
The existing harness SHALL retain success, failure, timeout, invalid/setup trace, uncertainty and self-audit outcomes, and SHALL publish compact sanitized measurements that distinguish observed correctness, time, calls, wrong-file work and context from modeled token estimates.

#### Scenario: Null, unfavorable or incomplete result
- **WHEN** ProjectAtlas has no measured advantage, a run fails, or a result is uncertain
- **THEN** that outcome remains visible under the preregistered rules and the report does not replace it with modeled savings or filter it away

#### Scenario: Private or excessive artifacts
- **WHEN** a run produces source content, private paths, secrets or excessive traces
- **THEN** publication refuses unsafe output until sanitization and bounds pass without silently removing the outcome itself

### Requirement: Evaluation reuses established product boundaries
The implementation SHALL first audit and reuse the existing harness. A no-product-code outcome SHALL be acceptable. Any demonstrated correction SHALL have causal proof, positive compatibility coverage and applicable repository gates; evaluation SHALL NOT implicitly index another root, initialize missing state, mutate ambient projects or add product capabilities.

#### Scenario: Wrong root or missing index
- **WHEN** a task encounters an unselected root or absent index
- **THEN** the existing explicit selection and initialization contracts govern both arms, and no hidden mutation is counted as successful navigation

#### Scenario: Existing harness is sufficient
- **WHEN** existing preregistration, execution and reporting satisfy the frozen contract
- **THEN** reuse them unchanged and accept a reproducible no-product-code result after independent review
