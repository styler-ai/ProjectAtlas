## ADDED Requirements

### Requirement: One closed affected-contract plan
The repository SHALL use one checked-in closed impact contract and one Python-standard-library planner to select existing build, test, and quality contracts for human and Dependabot pull requests. The planner SHALL reuse existing repository classifiers where their contracts fit and SHALL NOT require a new Rust crate, database, build system, generalized dependency graph, service, or third-party package.

#### Scenario: Known pull-request input
- **WHEN** a pull-request diff contains only paths classified by the closed impact contract
- **THEN** the planner emits the deterministic union of affected existing contract identifiers
- **AND** both human-authored and Dependabot pull requests use that same plan

#### Scenario: Planner-owning or unknown input
- **WHEN** a diff changes the planner, impact contract, aggregation ownership, shared metadata, or an unknown or unclassifiable path
- **THEN** the planner selects the complete proof set

### Requirement: Cargo owns Rust dependency direction
The planner SHALL derive Rust workspace nodes and reverse dependencies from one successful `cargo metadata` result and SHALL declare only cross-contract edges that Cargo cannot know: database/schema, CLI/MCP, Python, Node/Mermaid, documentation/OpenSpec/IssueOps, installers/packages, fixtures/generated inputs, and platform gates.

#### Scenario: Rust crate change
- **WHEN** a changed Rust target belongs to a workspace package
- **THEN** the plan includes that package's existing proof contracts and every affected reverse-dependent package contract reported by Cargo

#### Scenario: Cross-contract change
- **WHEN** a changed path matches a declared non-Cargo edge
- **THEN** the plan includes every existing contract reached by that closed edge

#### Scenario: Cargo graph cannot be trusted
- **WHEN** `cargo metadata` fails, is malformed, or cannot map an affected Rust path
- **THEN** the planner selects the complete proof set

### Requirement: Plans are bound to exact evidence
Every plan SHALL bind its base commit, head commit, pull-request event class, impact-contract digest, planner/workflow identity, toolchain identity, applicable platform, selected contracts, and canonical plan digest. A consumer SHALL reject a plan or result whose identity is missing, malformed, stale, or mismatched.

#### Scenario: Exact plan consumption
- **WHEN** a downstream job receives a well-formed plan whose identity matches its current run, toolchain, platform, base, and head
- **THEN** it may execute the selected contract or consume the plan's not-applicable record

#### Scenario: Stale or malformed evidence
- **WHEN** any plan field, digest, base, head, event, workflow, toolchain, platform, result, or not-applicable record is absent, stale, malformed, or mismatched
- **THEN** affected selection is not trusted
- **AND** the required context fails or the event reruns complete proof

### Requirement: Pull requests run the smallest contract-complete proof
For a trusted pull-request plan, the workflows SHALL execute every affected existing contract and SHALL omit work only through an explicit plan-bound not-applicable result. Only ordinary additions and modifications MAY select the union of known effects. Every rename or deletion SHALL select complete proof even when both old and new paths are classified, and any other fail-closed input SHALL select complete proof.

#### Scenario: Narrow known change
- **WHEN** a trusted plan proves that only a closed subset of contracts is affected
- **THEN** the pull request executes that complete subset without rebuilding or retesting unrelated contracts

#### Scenario: Rename or deletion
- **WHEN** a diff contains any rename or deletion
- **THEN** the planner selects complete proof regardless of whether the old and new paths are both classified

#### Scenario: Ordinary mixed diff
- **WHEN** a diff contains only ordinary additions and modifications with known impacts
- **THEN** the planner selects the deterministic union of those known impacts
- **AND** any fail-closed input in the same diff selects complete proof

### Requirement: Shared truth boundaries always run full proof
Default-branch pushes, scheduled drift checks, release-candidate paths, release paths, and manually invoked release proof SHALL execute the complete repository proof set without accepting affected-plan not-applicable state.

#### Scenario: Default branch or release boundary
- **WHEN** proof runs for the default branch, a schedule, a release candidate, or a release
- **THEN** every unit, integration, E2E, platform, security, dependency, packaging, installer, OpenSpec, IssueOps, Mermaid, and release contract applicable to that boundary executes

### Requirement: Stable required contexts aggregate fail closed
Every protected required context SHALL always aggregate either successful affected execution or trusted not-applicable evidence bound to the current plan. Missing, skipped, canceled, failed, stale, malformed, or mismatched plan/proof SHALL NOT satisfy a required context.

#### Scenario: Unaffected platform contract
- **WHEN** a trusted current plan proves a platform contract is not affected
- **THEN** the stable platform context succeeds only after validating its exact plan-bound not-applicable record

#### Scenario: Missing or canceled proof
- **WHEN** a selected execution is missing, skipped, canceled, failed, or does not match the current plan
- **THEN** its stable aggregator fails

### Requirement: Proof coverage and dependency reuse are preserved
Affected selection SHALL choose among existing proof contracts without deleting, weakening, replacing, or renaming away their behavior. Dependency layers SHALL reuse #341's digest-addressed contract and parser proof reuse SHALL retain #366's fail-closed input contract. #372 SHALL remain separate timeout work.

#### Scenario: A contract is selected
- **WHEN** the plan selects an existing quality or behavior contract
- **THEN** that contract runs with its existing quality bar and dependency-layer identity

#### Scenario: Dependabot pull request
- **WHEN** a Dependabot pull request changes dependency inputs
- **THEN** it uses the same affected planner and all selected security, dependency, build, test, platform, packaging, installer, and release-relevant contracts without a bot exemption

### Requirement: Cancellation is read-only and supersession-bound
Concurrency cancellation SHALL apply only to an older read-only pull-request CI run superseded by a newer head for the same pull request. It SHALL NOT cancel IssueOps, merge authorization, default-branch proof, scheduled proof, release, deployment, publication, or another mutation path.

#### Scenario: Superseded pull-request head
- **WHEN** a newer head starts read-only CI for the same pull request
- **THEN** the older read-only CI run may be canceled
- **AND** the newer head must independently satisfy every required context

#### Scenario: Stateful or shared-boundary work
- **WHEN** IssueOps, release, deployment, publication, scheduled, or default-branch work is running
- **THEN** pull-request CI concurrency grouping does not cancel it

### Requirement: Milestone planning separates specification from delivery
The accepted #497 proposal, design, capability, dependency, task-ownership, and architecture contract SHALL be checked in section 1 before milestone planning. The impact data, planner/workflow implementation, proof coverage, and final implementation-versus-diagram review SHALL remain unchecked section 2 delivery until substantively complete.

#### Scenario: #497 enters the release milestone
- **WHEN** exact published-main readback proves the checked section 1 contract and the authoritative/native release graph assigns #497 to v0.5
- **THEN** planned-issue readiness admits the open issue with its section 2 delivery tasks unchecked
- **AND** implementation validation and closure continue to require those delivery tasks to complete normally
