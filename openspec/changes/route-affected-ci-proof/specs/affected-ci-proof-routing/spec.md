## ADDED Requirements

### Requirement: Exact changes produce a closed proof plan
The source CI SHALL derive a bounded proof plan from the event's exact base-to-head change set, one Cargo workspace dependency graph, and a checked-in closed mapping of non-Cargo contract ownership. The plan SHALL identify fixed proof contracts and jobs rather than executable commands, and SHALL bind itself internally to the inputs used to create it.

#### Scenario: Production change selects reverse dependents and cross-contract owners
- **WHEN** a known Rust production path changes
- **THEN** the plan selects the owning unit and integration targets, Cargo reverse dependents, and every declared CLI, MCP, E2E, or platform contract that can observe that behavior

#### Scenario: Test-only change selects its owning domain
- **WHEN** only a known test target or post-split CLI E2E domain changes
- **THEN** the plan selects that test's compile prerequisites and owning domain without selecting unrelated product or platform contracts

#### Scenario: Rename or deletion preserves both ownership sides
- **WHEN** a known path is renamed or deleted
- **THEN** the plan unions the proof ownership of the old and new path representations before selecting jobs

#### Scenario: Exact plan inputs do not become public proof receipts
- **WHEN** the planner records base and head identity to reject stale reuse
- **THEN** that identity remains an internal run binding and acceptance is reported through behavior, tests, measurements, and required-context conclusions

#### Scenario: Pull request is retargeted without a head change
- **WHEN** an open pull request changes its base branch while retaining the same head
- **THEN** source CI replans and verifies the exact new base-to-head diff rather than retaining proof derived from the former base

### Requirement: Narrowing fails closed
The planner SHALL select complete normal-pull-request proof whenever it cannot prove that a narrower plan retains every causal defect-detection contract.

#### Scenario: Unknown or unmapped path
- **WHEN** the diff contains an unknown path or a path without complete contract ownership
- **THEN** the planner selects complete normal-pull-request proof and reports the fallback reason

#### Scenario: Shared or routing authority changes
- **WHEN** shared test support, a workflow, the planner or contract map, toolchain inputs, the lockfile, a workspace manifest, or a schema changes
- **THEN** the planner selects complete normal-pull-request proof

#### Scenario: Planning input fails
- **WHEN** diff parsing or Cargo metadata fails, is malformed, exceeds a bound, or cannot be tied to the current event inputs
- **THEN** source verification fails or selects complete proof and never accepts a narrow plan

#### Scenario: Human and automated pull requests share policy
- **WHEN** the same exact change is submitted by a human or dependency bot
- **THEN** both submissions receive the same proof selection and fail-closed behavior

### Requirement: Every selection and omission is explainable
The planner SHALL publish a bounded Actions summary that identifies every selected and omitted proof contract and states the path, dependency, or ownership rule responsible for that decision.

#### Scenario: Narrow plan report
- **WHEN** a documentation-only, leaf-crate, or CLI-domain change receives a narrow plan
- **THEN** the summary explains why each selected contract can observe the change and why each omitted Rust or platform contract cannot

#### Scenario: Fallback plan report
- **WHEN** uncertainty forces complete normal-pull-request proof
- **THEN** the summary identifies the uncertain input and does not claim that omitted proof is safe

#### Scenario: Report remains diagnostic rather than authoritative evidence
- **WHEN** a run completes
- **THEN** the summary helps reviewers understand routing while required job conclusions and behavior tests remain the acceptance authority

### Requirement: Local pre-push uses the same affected plan
The local pre-push hook SHALL bind the same closed proof plan to the exact clean candidate head and accepted base before running expensive proof, and SHALL execute only the fixed local commands that own selected contracts.

#### Scenario: Narrow candidate is pushed
- **WHEN** an exact clean candidate contains only a known documentation, independent leaf-crate, or CLI test-domain change
- **THEN** pre-push runs its IssueOps and selected owning proof without compiling or testing unrelated crates or domains

#### Scenario: Local plan cannot be trusted
- **WHEN** the pushed ref input, candidate identity, accepted base, metadata, or ownership mapping is invalid, stale, ambiguous, or unknown
- **THEN** pre-push fails or selects complete local proof and never guesses a narrower command set

### Requirement: Live PR state does not repeat unchanged source proof
Issue-reference and milestone validation SHALL run in a lightweight required `pr-state` workflow. An owning issue close, reopen, milestone assignment, or milestone removal SHALL make an isolated issue-event job rerun the existing `pr-state` workflow on every affected open pull request's current head without launching source verification. GitHub native required conversation resolution SHALL own live review-thread state so resolving or reopening a thread cannot leave a stale workflow conclusion. Review and review-comment activity SHALL NOT launch `pr-state`, Rust compilation, Rust tests, or platform E2E jobs for an unchanged source tree. Automatic cancellation SHALL be disabled for `pr-state`.

#### Scenario: Review or review-comment changes
- **WHEN** a review is submitted, edited, or dismissed or a review comment is created, edited, or deleted without a source change
- **THEN** native conversation resolution owns thread blockage and no `pr-state`, source-verification, or platform job starts

#### Scenario: Pull-request title or body changes
- **WHEN** a pull-request `edited` event changes no base ref
- **THEN** `pr-state` revalidates metadata while source planning, compilation, tests, platform proof, and the required `verify` context do not run or cancel an in-flight source run

#### Scenario: Source and review events overlap
- **WHEN** source CI is running while a review event arrives
- **THEN** the native review condition cannot launch or cancel source CI

#### Scenario: PR metadata invalidates readiness
- **WHEN** the owning issue reference or milestone is invalid
- **THEN** `pr-state` fails independently of any previously successful source proof

#### Scenario: Owning issue metadata changes after source proof
- **WHEN** the referenced issue closes, reopens, receives a milestone, or loses its milestone after `pr-state` ran
- **THEN** the `pr-state` issue-event job reruns the existing exact current-head workflow so live validation writes the new success or failure without running repository, Rust, or platform proof

#### Scenario: Owning issue refresh cannot reach the PR head
- **WHEN** pull-request enumeration, current-head run lookup, bounded waiting, or the Actions rerun request fails
- **THEN** the issue-event job fails visibly without publishing a green replacement or running source proof, and final acceptance rereads the live owning issue and milestone and blocks merge until manual recovery succeeds

#### Scenario: Review thread is reopened
- **WHEN** any pull-request review conversation is reopened after prior source and metadata proof succeeded
- **THEN** native required conversation resolution blocks the pull request immediately without rerunning source proof

#### Scenario: Review thread is resolved
- **WHEN** the last unresolved pull-request review conversation is resolved
- **THEN** the native branch rule refreshes readiness without requiring a workflow rerun

### Requirement: Cancellation is limited to superseded same-PR source verification
Automatic workflow cancellation SHALL apply only when a newer `pull_request` source-verification run supersedes an older `pull_request` source-verification run for the same pull-request number. Pull-request source verification, `pr-state`, push, merge-group, workflow-dispatch, schedule, IssueOps, release, publish, and deploy owners SHALL use separate deterministic concurrency namespaces, and automatic cancellation SHALL be disabled for every namespace except same-number pull-request source verification.

#### Scenario: New source run supersedes the same pull request
- **WHEN** a newer `pull_request` source-verification run starts while an older source-verification run for the same pull-request number is active
- **THEN** the newer run may cancel only that older same-number source-verification run

#### Scenario: Different pull requests overlap
- **WHEN** source-verification runs for different pull-request numbers overlap
- **THEN** neither run cancels the other

#### Scenario: Metadata-only edit overlaps source verification
- **WHEN** a title or body edit occurs while source verification for that pull request is active
- **THEN** the metadata-only event uses a unique non-cancelling namespace and cannot cancel or satisfy the source run

#### Scenario: Non-PR-source events overlap
- **WHEN** a pull-request source run overlaps a push, merge-group, workflow-dispatch, or scheduled run
- **THEN** separate namespaces and disabled cancellation preserve every run

#### Scenario: Governance or delivery workflow overlaps source verification
- **WHEN** source verification overlaps `pr-state`, IssueOps, release, publish, or deploy work
- **THEN** neither owner cancels the other and no shared concurrency namespace connects them

### Requirement: Selected proof runs concurrently behind a fail-closed aggregate
Static quality, Rust test-domain, and platform jobs SHALL run concurrently when they do not consume one another's outputs, and one stable required `verify` aggregate SHALL decide whether the source proof is complete.

#### Scenario: All selected jobs succeed
- **WHEN** the current plan is valid and every selected job succeeds
- **THEN** `verify` succeeds after confirming that every omitted job was explicitly not applicable

#### Scenario: Selected job does not succeed
- **WHEN** a selected job is missing, skipped, cancelled, or failed
- **THEN** `verify` fails even if every other selected job succeeded

#### Scenario: Stale or mismatched plan reaches aggregation
- **WHEN** the aggregate cannot bind a plan or job results to the current event inputs
- **THEN** `verify` fails rather than reusing the results

#### Scenario: Unselected static job is skipped
- **WHEN** the valid current plan marks a static job not applicable
- **THEN** that skipped job is acceptable only through the final aggregate and is not itself a required branch-protection context

#### Scenario: All current readiness inputs succeed
- **WHEN** the current `pr-state` and current `verify` for the same pull-request source input both exist and succeed and every review conversation is resolved
- **THEN** their explicit logical AND with native conversation resolution permits readiness

#### Scenario: One readiness input is not current and successful
- **WHEN** either `pr-state` or `verify` is absent, stale, skipped, cancelled, or failed, or a review conversation is unresolved
- **THEN** the pull request remains blocked and the other inputs cannot replace it

#### Scenario: Final acceptance follows an issue-refresh failure
- **WHEN** an owning issue event could not request the exact current-head `pr-state` rerun
- **THEN** the final live issue and milestone read blocks merge until metadata is valid and the failed refresh is recovered, regardless of an older green PR-head check

### Requirement: Platform proof follows affected behavior
Ordinary pull-request CI SHALL run each platform job only when that operating system or architecture can add defect-detection value for an affected contract, while retaining complete proof for ambiguous and shared changes.

#### Scenario: Platform-neutral behavior
- **WHEN** the affected contract's owning tests establish that operating-system and architecture variation cannot change its outcome
- **THEN** unrelated Windows and macOS jobs are omitted with an explicit rationale

#### Scenario: Operating-system-sensitive behavior
- **WHEN** process, path, filesystem, watcher, installer, packaging, or runtime behavior is owned by multiple operating systems
- **THEN** the plan selects every owning operating system

#### Scenario: Target-gated production source has a neutral filename
- **WHEN** an affected Rust production file declares a supported operating-system, target-family, or architecture condition in its exact base or head bytes
- **THEN** the plan selects a focused affected-package compile on every supported target matching each positive predicate even when the file path carries no platform name

#### Scenario: Target ownership cannot be mapped safely
- **WHEN** a target-bearing predicate is conjunctive, negated, unsupported, unreadable, unsupported-mode, individually oversized, or exceeds the bounded aggregate blob-read budget
- **THEN** the plan selects complete proof instead of inferring platform ownership

#### Scenario: macOS architecture-sensitive behavior
- **WHEN** an affected macOS contract can vary between Intel and ARM
- **THEN** the plan selects both macOS architectures

#### Scenario: Ownership is ambiguous
- **WHEN** no existing test or ownership rule proves that a platform cannot add detection value
- **THEN** the plan includes that platform in the complete fallback

### Requirement: Integrated release acceptance retains complete installed-product proof
The release-candidate boundary SHALL execute the complete installed CLI-command and MCP-tool inventory on Linux, Windows, macOS Intel, and macOS ARM independently of affected pull-request routing.

#### Scenario: Candidate enters release acceptance
- **WHEN** an integrated candidate is eligible for release acceptance
- **THEN** the complete installed-product four-platform boundary runs, subject only to input-equivalent proof reuse already authorized by the release-proof contract

#### Scenario: Candidate proof finds a defect
- **WHEN** any full-boundary contract fails and the defect is fixed in its owning issue
- **THEN** the entire release-candidate boundary restarts for the updated integrated candidate

#### Scenario: Pull-request plan is narrow
- **WHEN** one or more ordinary pull requests omitted unaffected platform proof
- **THEN** those omissions do not remove, satisfy, or replace any part of the complete release-candidate boundary

### Requirement: Routing speedup is demonstrated without weakening proof
The implementation SHALL measure required-check wall time including queueing and aggregate raw runner-minutes before and after for documentation-only, independent leaf-crate, CLI test-domain, shared-core, and platform-sensitive changes.

#### Scenario: Representative ordinary change meets the target
- **WHEN** a documentation-only, leaf-crate, or ordinary CLI-domain change is measured on hosted Actions
- **THEN** its required checks complete within ten minutes where runner availability permits and the result retains every causal proof contract

#### Scenario: Broad affected change meets the design ceiling
- **WHEN** shared-core, platform-sensitive, or fail-closed proof is measured
- **THEN** required checks complete within fifteen minutes or the measured indivisible job or external queue limitation is recorded without omitting causal proof

#### Scenario: Claimed optimization is material
- **WHEN** a routing rule is proposed as a speed improvement
- **THEN** representative before/after data shows at least a 30 percent and 30 second improvement in the claimed metric and reports both wall time and raw runner-minutes

#### Scenario: Measurement is inconclusive or coverage regresses
- **WHEN** measurements are noisy, below materiality, or reveal loss of a causal proof contract
- **THEN** the narrowing rule is rejected or returned to complete fallback rather than accepted on assertion

### Requirement: Default-branch and drift backstops preserve coverage
Protected-branch pushes SHALL run affected proof for the merged change, scheduled and manual drift checks SHALL run complete normal-pull-request proof, and ambiguous merge-group inputs SHALL fail closed.

#### Scenario: Merged change reaches the protected branch
- **WHEN** a pull request merges
- **THEN** the protected-branch workflow verifies the affected contracts against the merged tree

#### Scenario: Scheduled or manual drift check runs
- **WHEN** the complete drift backstop is scheduled or explicitly dispatched
- **THEN** every normal-pull-request proof contract runs regardless of a narrow prior plan

#### Scenario: Merge-group inputs are unavailable or ambiguous
- **WHEN** a merge-group event cannot provide a trustworthy exact diff
- **THEN** the merge-group source workflow selects complete normal-pull-request proof

### Requirement: Planned issue tasks literally mirror mapped OpenSpec tasks
For `--planned-issue`, IssueOps SHALL compare the live issue's `Implementation Tasks` with the mapped `tasks.md` task slice by literal task text, order, and checkbox state. Equal task counts or matching identifiers SHALL NOT excuse any difference.

#### Scenario: Literal task mirror matches
- **WHEN** the live task text, order, and checkbox state match the mapped `tasks.md` slice
- **THEN** planned-issue task comparison succeeds

#### Scenario: Same-count task text drifts
- **WHEN** the live issue and local task slice contain the same number of tasks but any task text differs
- **THEN** planned-issue IssueOps fails and identifies the first differing task

#### Scenario: Same-count task order drifts
- **WHEN** the live issue and local task slice contain the same tasks in a different order
- **THEN** planned-issue IssueOps fails before implementation handoff

#### Scenario: Checkbox state drifts
- **WHEN** any live task checkbox state differs from the mapped local task state
- **THEN** planned-issue IssueOps fails even when task count, identifiers, text, and order otherwise match
