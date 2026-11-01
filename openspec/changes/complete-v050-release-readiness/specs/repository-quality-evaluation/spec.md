## ADDED Requirements

### Requirement: The filtered custom-harness release step has one native timeout
#372 SHALL add only the existing GitHub Actions step-level `timeout-minutes` mechanism to `.github/workflows/release.yml` `Filtered custom harness compatibility`, with a value consistent with neighboring release verification. The Cargo command, output, exit status, and artifacts SHALL remain unchanged.

#### Scenario: Step completes inside the bound
- **WHEN** the existing filtered custom-harness command completes
- **THEN** the release job receives its exact result and artifacts

#### Scenario: Step exceeds the bound
- **WHEN** the command runs longer than the configured step limit
- **THEN** GitHub Actions fails that step through native timeout behavior and no application timeout/process-killing framework is added

#### Scenario: Workflow contract is checked
- **WHEN** the workflow changes
- **THEN** the existing narrow assertion verifies step name, unchanged command, and timeout at the correct YAML scope

### Requirement: CLI E2E tests move through one accepted responsibility map
#487 SHALL map every current `e2e.rs` test/helper/platform gate into the fewest cohesive owners: lifecycle, delivery, navigation, worktrees, maintenance, or an existing separate suite. Only process spawning, command execution, temporary repository construction, JSON assertions, platform-safe path/link helpers, and packaged-contract helpers used by at least two domains MAY enter `tests/support`.

#### Scenario: Proposed five-domain map is cohesive
- **WHEN** each domain has durable behavior/test ownership
- **THEN** `e2e_lifecycle.rs`, `e2e_delivery.rs`, `e2e_navigation.rs`, `e2e_worktrees.rs`, and `e2e_maintenance.rs` are moved one runnable domain at a time

#### Scenario: Proposed binary has no independent owner
- **WHEN** inventory proves a domain would be symmetry or line-count partitioning
- **THEN** it is merged with the nearest cohesive owner before movement

#### Scenario: Inventory after movement
- **WHEN** the split completes
- **THEN** no test, assertion, exact selector, ignored/platform attribute, timeout, cleanup, isolation, packaged path, or CI/release invocation is missing or weakened

### Requirement: Production modules change only for proven cohesive ownership
#488 SHALL create one accepted caller/state/data/transaction/concurrency/error/test/hot-path map for CLI `mcp.rs`, `runtime.rs`, database `lib.rs`, and `repository_graph.rs`. It SHALL apply only moves with an independent durable owner and SHALL preserve seven crates, public APIs/wire/CLI, SQL/schema/transaction authority, cancellation, error chains, and platform behavior.

#### Scenario: Move is accepted
- **WHEN** callers, state, tests, failure/transaction ownership, and dependency direction prove a cohesive responsibility
- **THEN** it moves to the fewest durable modules behind existing owning re-exports with compatibility, SQLite, fault, concurrency, E2E, and intended-scale proof

#### Scenario: Move is rejected
- **WHEN** only size, symmetry, phase, or temporary naming supports the split
- **THEN** the current module remains with a recorded no-change disposition and no generic utility/repository/facade layer is added

### Requirement: The named oversized benchmark trace leaves normal source history
#489 SHALL remove only `docs/benchmarks/v0.4-agent-navigation-failed-binary-init-29a4863.jsonl` (42,809,126 bytes) from the current tree without rewriting history and SHALL retain compact sanitized reproducibility metadata only where useful.

#### Scenario: Current cleanup
- **WHEN** the artifact is removed
- **THEN** retained metadata identifies candidate/repository/harness/runtime, failure class, digest/size, and reproduction command without the raw trace

#### Scenario: Future raw benchmark output is tracked
- **WHEN** the narrow existing benchmark/repository policy sees an unapproved oversized raw result
- **THEN** it rejects it while allowing normal fixtures, approved compact evidence, ignored local output, and compressed/release assets

### Requirement: The evaluation has current release and task ownership
#490 SHALL belong to v0.6.0 under sole parent #493, SHALL directly block #493, and SHALL be directly blocked by #310 and #563. The issue mirror, the existing #490 task slice 24.1–24.4 in this change, and the declared/native graph SHALL agree. Only that deferred evaluation slice is re-scoped for v0.6; other owners and historical completion states SHALL remain unchanged.

#### Scenario: Planning reconciliation
- **WHEN** the v0.6 evaluation is admitted as ready
- **THEN** it retains its sole established task mapping and matching milestone, parent, blockers and release-owner scope, while v0.5.1-rc1 gains no evaluation dependency

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

### Requirement: Every structural/evaluation owner preserves the release boundary
#372/#487/#488/#489/#490 SHALL keep positive, negative, failure, recovery, compatibility, affected-platform, cleanup, privacy, and intended-scale proof at the closest owner and SHALL not substitute structural compilation or file movement for behavior.

#### Scenario: Structural change alters behavior or proof
- **WHEN** a move changes discovery, serialization, transaction/lock lifetime, cancellation, workflow selection, or an executable assertion
- **THEN** the task remains incomplete until the owning behavior is restored and re-proven
