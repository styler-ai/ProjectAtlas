## ADDED Requirements

### Requirement: Repository-owned quality policy and pinned tools
ProjectAtlas SHALL keep one machine-readable Rust test-quality policy that declares the canonical `styler-ai/ProjectAtlas` repository identity, applicable source scope, supported platform identities, exact quality-tool versions, timeout ceilings, coverage floors and agreed targets, viable-mutation floor and agreed target, exception records, raw/filtered inventory metadata, and evidence-binding rules. CI and release workflows SHALL install and verify exactly `cargo-nextest` 0.9.140, `cargo-llvm-cov` 0.8.7, and `cargo-mutants` 27.1.0 until an intentional policy update changes those pins. Every evidence manifest SHALL also record the exact Rust and LLVM versions used; the measured starting environment is Rust 1.93.1 with LLVM 21.1.8, not a portable coverage claim.

#### Scenario: Pinned tools are available
- **WHEN** a required quality job starts with tool versions matching the repository policy
- **THEN** it records those versions in the evidence manifest before running the gate

#### Scenario: A quality tool is missing or mismatched
- **WHEN** a required tool is missing, reports another version, or cannot report its version
- **THEN** the affected gate fails before accepting test, coverage, or mutation evidence

#### Scenario: The policy is incomplete
- **WHEN** a supported platform, required timeout, quality dimension, target, or evidence rule is absent from the policy
- **THEN** policy validation fails rather than applying a workflow default

### Requirement: Applicable ProjectAtlas-owned Rust scope
Coverage and source mutation SHALL evaluate applicable production Rust owned by every ProjectAtlas workspace crate, including the cargo-adjacent `projectatlas-lints` tool. Dependency source, test-only fixtures, generated outputs, and demonstrably unreachable platform code MAY be outside the adjusted applicable denominator only through a typed scope rule or valid exception, while raw reports SHALL retain every source row emitted by the native tool. The policy SHALL reject blanket workspace, crate, or production-source exclusions.

#### Scenario: A new owned production source file is added
- **WHEN** a ProjectAtlas workspace crate adds a compiled production `.rs` file
- **THEN** coverage and mutation discovery include it automatically unless an exact valid exception applies

#### Scenario: A blanket exclusion is proposed
- **WHEN** policy excludes an entire workspace, crate, or production source tree to improve a metric
- **THEN** policy validation fails and reports the prohibited scope reduction

#### Scenario: Raw and adjusted results differ
- **WHEN** valid exceptions remove specific source ranges or mutant selectors from the applicable denominator
- **THEN** evidence reports both raw and adjusted totals and identifies every adjustment

### Requirement: Repository-root isolation and no implicit ProjectAtlas mutation
Quality-policy and evidence paths SHALL resolve within the explicitly selected repository root, and evidence SHALL bind to that root's commit and lockfile. Validation SHALL NOT route through another repository, initialize or scan a ProjectAtlas index, edit source/configuration, or mutate `.projectatlas/projectatlas.db`; only native build output and the explicitly selected evidence directory may be written by quality commands.

#### Scenario: Quality validation runs from the wrong root
- **WHEN** policy, lockfile, commit, or evidence identity belongs to another repository root
- **THEN** validation fails as wrong-root input without reading or updating the other repository's quality state

#### Scenario: The ProjectAtlas index is missing
- **WHEN** `.projectatlas/projectatlas.db` is absent but the repository policy and source checkout are valid
- **THEN** quality validation operates on the repository inputs and performs no implicit `init`, `scan`, or index creation

#### Scenario: Validation fails
- **WHEN** a malformed policy or failing evidence set is inspected
- **THEN** the validator returns a nonzero status without changing source, policy, lockfile, hooks, workflows, or ProjectAtlas index state

### Requirement: Post-stabilization repository-wide quality closure
The repository-intelligence architecture, migrations, public contracts, and feature behavior SHALL stabilize before this change performs repository-wide legacy/refactored test saturation, final adjusted-coverage closure, the complete 16-shard source-mutation campaign, or final task-evidence reconciliation. Stabilized means the accepted feature behavior is implemented, focused behavior tests pass, blocking architecture findings are resolved, and no planned broad ownership, schema, service, or public-contract refactor is expected to invalidate the evidence. This ordering SHALL NOT defer tests for new stable behavior: each new algorithm, branch, migration, boundary, and public workflow SHALL receive its focused owning-logic unit test and every risk-required integration, CLI/MCP E2E, packaged, or platform test when implemented. The completed quality change SHALL remain a hard v0.4 release prerequisite, not a prerequisite to start feature implementation.

#### Scenario: A stable feature slice is implemented
- **WHEN** the slice has its focused owning-logic test and every risk-required boundary or public-workflow test but repository-wide saturation has not started
- **THEN** repository-intelligence implementation may continue while its checklist task remains unchecked until final evidence closure

#### Scenario: Broad quality evidence predates a planned refactor
- **WHEN** coverage, full-mutation, or final task evidence was produced before a planned ownership, schema, service, or public-contract refactor completed
- **THEN** that evidence is ineligible for v0.4 closure and must be regenerated against the stabilized source and test scope

#### Scenario: The product architecture is stable
- **WHEN** accepted feature behavior and focused tests pass with no unresolved blocking architecture finding or planned broad refactor
- **THEN** the repository-wide saturation, adjusted-coverage, full-mutation, and final evidence sequence may begin

### Requirement: Independent blocking quality conclusions
CI SHALL expose separate blocking conclusions for non-doctest nextest execution, stable doctests, LLVM source coverage, and changed-source mutation. A passing job in one dimension SHALL NOT satisfy, skip, or overwrite a failure in another. Existing format, workspace check, strict Clippy, rustdoc, source-lint, dependency, ProjectAtlas scan, and ProjectAtlas lint gates SHALL remain blocking and warning-free.

#### Scenario: Nextest passes but a doctest fails
- **WHEN** all nextest suites pass and stable `cargo test --doc` fails
- **THEN** the doctest conclusion and the overall required check set fail

#### Scenario: Tests pass below a coverage floor
- **WHEN** nextest and doctests pass but any required coverage dimension is below its platform floor
- **THEN** the coverage conclusion fails and no aggregate success masks it

#### Scenario: Changed mutation finds a survivor
- **WHEN** all ordinary tests pass but changed-source mutation reports a viable missed mutant
- **THEN** the mutation conclusion fails

#### Scenario: A workflow adds Tarpaulin
- **WHEN** repository policy or a required workflow introduces Tarpaulin as another source-coverage gate
- **THEN** workflow-policy validation fails because LLVM coverage is the declared source-coverage implementation

### Requirement: Deterministic nextest execution and inventory evidence
The nextest gate SHALL run the locked all-feature workspace through pinned `cargo-nextest` with repository-owned `.config/nextest.toml`, explicit per-test and job timeouts, deterministic retry policy, and machine-readable JUnit evidence. The policy SHALL treat test inventory as an integrity signal rather than a quality score. The measured starting inventory is 286 runnable non-doctests across nine suites with zero ignored tests; a current inventory change SHALL be measured and reviewed rather than silently preserving or inventing that count.

#### Scenario: All nextest suites pass
- **WHEN** every discovered runnable non-doctest completes successfully within policy and JUnit reconciles to the runner summary
- **THEN** the nextest gate records a passing manifest with suite, test, ignored, failed, and timed-out counts

#### Scenario: Nextest discovers no tests
- **WHEN** nextest exits successfully but discovers zero runnable tests for a non-empty workspace
- **THEN** the gate fails as an empty test inventory

#### Scenario: Inventory unexpectedly shrinks
- **WHEN** the runnable or suite count drops below the reviewed policy inventory without a matching approved inventory update and retained evidence
- **THEN** the gate fails and reports the missing inventory

#### Scenario: A test is ignored, retried, or times out
- **WHEN** nextest reports an ignored, retried, slow, or timed-out test outside the declared policy
- **THEN** the gate fails or applies only the exact checked-in policy disposition and retains the event in JUnit evidence

### Requirement: Stable doctest gate
Doctests SHALL run independently with stable `cargo test --doc --workspace --all-features --locked`, an explicit job timeout, and retained command/status evidence. Neither nextest nor unstable `cargo-llvm-cov --doctests` instrumentation SHALL count as this gate.

#### Scenario: Stable doctests pass
- **WHEN** the stable workspace doctest command completes successfully within its timeout
- **THEN** the doctest gate records a separate passing conclusion

#### Scenario: Only instrumented doctest evidence exists
- **WHEN** a workflow provides LLVM-instrumented or nextest evidence but does not run stable `cargo test --doc`
- **THEN** the doctest gate fails as missing

#### Scenario: A documentation example regresses
- **WHEN** a compiled documentation example fails while ordinary tests pass
- **THEN** the stable doctest conclusion blocks the pull request or release

### Requirement: Platform-specific LLVM coverage evidence
Coverage SHALL run through pinned `cargo-llvm-cov` with matching `llvm-tools-preview` on every supported release operating-system identity and SHALL export machine-readable LLVM JSON plus a human-reviewable report. Each platform SHALL report covered and total lines, regions, and functions separately, together with missed counts and source scope. Cross-platform pooling and substitution SHALL be forbidden.

The pinned local starting observation is 24,130 of 27,495 lines (87.76%, 3,365 missed), 34,041 of 40,094 regions (84.90%), and 2,045 of 2,369 functions (86.32%). It SHALL initialize only the platform and exact scope identified by retained eligible provenance; until then it remains an observation, not a floor. Linux, macOS, or another platform without retained baseline evidence SHALL remain unestablished and blocking; its floors SHALL NOT be copied from the measured host.

#### Scenario: Every platform meets all floors
- **WHEN** each supported platform produces complete evidence whose line, region, and function counts meet that platform's committed floors
- **THEN** each platform coverage job passes independently

#### Scenario: One coverage dimension regresses
- **WHEN** line and function coverage pass but region coverage is below its platform floor
- **THEN** that platform coverage job fails and names the region count and deficit

#### Scenario: A platform baseline is missing
- **WHEN** a supported release platform has no complete retained baseline and committed floor
- **THEN** the coverage gate remains unestablished and release verification fails rather than borrowing another platform's result

#### Scenario: The initial snapshot is reported
- **WHEN** documentation or a job summary presents the measured starting percentages
- **THEN** it labels the originating platform, commit, scope, toolchain, exact covered/total counts, and 3,365 missed production lines and does not call the result complete coverage

### Requirement: Monotonic coverage ratchets and explicit targets
The policy SHALL store numeric line, region, and function floors and the agreed hard v0.4 targets for every supported platform: 98% raw/100% adjusted lines, 95% raw/98% adjusted regions, and 98% raw/100% adjusted functions. A policy update SHALL NOT lower a floor, reduce source scope, increase an exception's reach, or relabel uncovered production Rust as non-applicable. A floor SHALL rise only to a value demonstrated by retained passing evidence with the same platform and scope. CI and release SHALL fail any dimension below its floor. Final PR-review and release evidence SHALL use the typed `release_quality` enforcement mode and also fail any dimension below its target; a tracking issue or expiring target-gap record SHALL document work but SHALL NOT make an unmet target pass. During the separately specified repository-intelligence implementation phases, typed `implementation_checkpoint` evidence MAY validate complete LLVM structure, scope, count reconciliation, exceptions, and platform floors without enforcing the final targets, but that mode SHALL be retained in the validator summary and manifest and SHALL be ineligible for final PR-review or release aggregation. Omitted enforcement SHALL default to `release_quality`. Public 100% language SHALL be allowed only when the corresponding raw or explicitly adjusted count is actually complete and its exceptions are disclosed.

#### Scenario: A floor is lowered
- **WHEN** a pull request changes any platform coverage floor below its merge-base value
- **THEN** ratchet validation fails even if current coverage exceeds the lowered value

#### Scenario: A floor is raised without evidence
- **WHEN** policy raises a floor but no retained same-platform evidence proves that value for the same scope
- **THEN** the policy update fails validation

#### Scenario: Coverage improves
- **WHEN** complete retained evidence exceeds a committed floor on the same platform and scope
- **THEN** the floor update may adopt the demonstrated value without changing the declared near-complete target

#### Scenario: The agreed target is missed
- **WHEN** a v0.4 release candidate meets the historical floor but misses any agreed line, region, or function target
- **THEN** CI/review and release verification fail, the gap remains linked to an open tracking issue, and no waiver or expiry record converts it to success

#### Scenario: A feature implementation checkpoint precedes saturation
- **WHEN** a repository-intelligence phase produces structurally complete, reconciled, floor-compliant coverage before the post-stabilization target campaign
- **THEN** `implementation_checkpoint` evidence may support that phase checkpoint but cannot satisfy final PR-review or release aggregation

### Requirement: Reviewed and expiring quality exceptions
Every coverage or mutation exception SHALL be machine-readable, narrowly identify an exact path and range or stable mutant selector, declare its category and rationale, name an owner and tracking issue, record approval provenance, and expire on a future date or release. Expired, ambiguous, overlapping, broadened, orphaned, unused, or source-mismatched exceptions SHALL fail. Exceptions SHALL NOT delete raw native evidence or improve the reported raw metric.

#### Scenario: A valid exception applies
- **WHEN** an exact source range or mutant matches a reviewed non-expired exception and its source identity still matches
- **THEN** the adjusted denominator records the exception while raw evidence and exception metadata remain visible

#### Scenario: An exception expires
- **WHEN** the current date or release reaches an exception's expiry
- **THEN** quality validation fails until the code is covered, the mutant is killed, or a newly reviewed record replaces it

#### Scenario: An exception silently broadens
- **WHEN** a glob, range, or selector change covers more production code or mutants than the reviewed record
- **THEN** validation rejects the scope growth

#### Scenario: An exception matches nothing
- **WHEN** source changes leave a checked-in exception unused or mismatched
- **THEN** validation fails and requires removal or an intentional reviewed update

### Requirement: Commit-bound retained evidence
Every required gate SHALL produce a normalized manifest that binds results to the canonical repository identity, repository root identity, commit SHA, target OS/architecture, Rust/LLVM and quality-tool versions, `Cargo.lock` digest, policy and native-config digests, command/profile, applicable scope, timeout policy, start/completion timestamps, raw artifact digests, and a typed gate-specific result/status. Wrong-repository, wrong-commit, stale, partial, truncated, internally inconsistent, or manually unverifiable evidence SHALL fail validation. Required artifacts SHALL be retained for 90 days, exceeding the 30-day release-decision window; expired or unavailable evidence SHALL fail.

Required raw and normalized artifacts SHALL upload with `if: always()` and explicit retention even when the gate fails. At minimum, this includes nextest JUnit, doctest logs/status, LLVM JSON and a human-readable coverage report, mutation master/shard inventories and native outcomes, normalized manifests, and failure diagnostics. Artifact upload SHALL NOT convert the originating gate to success.

#### Scenario: Evidence matches the run
- **WHEN** native outputs, manifest digests, commit, policy, platform, and final status reconcile
- **THEN** the evidence is eligible for its quality and release decisions

#### Scenario: Evidence belongs to another commit
- **WHEN** a release candidate attempts to consume a passing manifest from another commit or lockfile
- **THEN** release verification fails as stale evidence

#### Scenario: A gate fails before normal report generation
- **WHEN** a test, coverage, mutation, timeout, or infrastructure step fails
- **THEN** available raw outputs and diagnostics upload with `if: always()` while the job remains failed

#### Scenario: An artifact is edited or truncated
- **WHEN** a retained artifact digest or native count no longer matches its manifest
- **THEN** validation rejects the evidence

### Requirement: Pull-request changed-source mutation gate
Every pull request that changes applicable ProjectAtlas-owned Rust SHALL resolve and record a trusted merge base, use pinned `cargo-mutants` native diff selection, run a successful unmutated baseline, and completely disposition the selected candidate set. The gate SHALL require zero viable missed mutants, zero timed-out mutants, zero untested or unresolved candidates, and only valid non-expired exclusions. Missing base history, diff failure, tool failure, baseline failure, or incomplete output SHALL fail closed.

#### Scenario: All changed mutants are killed
- **WHEN** the trusted diff selects applicable candidates and every viable candidate is caught without timeout
- **THEN** the changed-source mutation gate passes with complete inventory and outcome evidence

#### Scenario: A changed viable mutant survives
- **WHEN** any selected viable mutant is missed
- **THEN** the gate fails and reports its stable identity, source span, mutation, and native evidence

#### Scenario: A changed mutant times out
- **WHEN** any selected mutant reaches the mutant test timeout
- **THEN** the gate fails rather than counting the timeout as a kill

#### Scenario: The selected set is empty
- **WHEN** native diff selection returns no candidates
- **THEN** the gate passes only if retained diff and scope evidence proves no eligible production mutation exists

#### Scenario: Merge-base history is unavailable
- **WHEN** checkout depth or repository state prevents resolving the trusted merge base
- **THEN** the gate fails rather than treating the diff as empty

### Requirement: Deterministic full mutation inventory and 16-shard reconciliation
A full source-mutation run SHALL generate one pinned unfiltered raw `cargo-mutants --list --json` master inventory for the exact commit, source/generator policy, no-policy-exclusion config, and tool version. Because native config exclusions remove candidates before listing, reviewed quality exceptions SHALL NOT be present in that raw-list configuration. The validator SHALL apply only exact valid policy exceptions to produce a disjoint excluded inventory and filtered execution inventory, with retained filter/config identities. Exactly 16 deterministic native shards SHALL execute the filtered inventory. Aggregate reconciliation SHALL prove that every filtered candidate appears exactly once across shard outcomes, no excluded candidate executes, and the disjoint union of filtered plus exact excluded candidates equals the raw master. Missing, duplicate, omitted, foreign, inconsistently filtered, cancelled, or timed-out required shards and incomplete native outcomes SHALL fail.

The earlier audit inventory is 4,911 candidates: 2,189 CLI, 951 DB, 587 service, 570 symbols, 441 core, 97 filesystem, and 76 lint candidates. It used cargo-mutants' native default call skip. The current unfiltered observation disables that default and contains 4,931 candidates: 2,205 CLI, 955 DB, and unchanged counts for the other crates. The first implemented full run SHALL reconcile its exact current master and retain source/tool/config-backed evidence for any drift; it SHALL NOT force either observed count into current evidence.

#### Scenario: All shards reconcile
- **WHEN** all 16 shards complete against the same raw/filtered identities and their candidate union plus exact excluded inventory equals the raw master with no duplicate or overlap
- **THEN** the aggregate is eligible for mutation-score validation

#### Scenario: One shard is absent
- **WHEN** only 15 required shard manifests are available
- **THEN** aggregation fails and identifies the missing shard

#### Scenario: A candidate appears twice or not at all
- **WHEN** shard union contains a duplicate, omission, or candidate absent from the master
- **THEN** aggregation fails with the candidate and shard identities

#### Scenario: Current inventory differs from an earlier observation
- **WHEN** the pinned current master inventory differs from the measured starting snapshot because source, policy, config, or tool identity changed
- **THEN** the run reports and explains the drift from native evidence instead of fabricating parity

### Requirement: Full mutation disposition and monotonic viable kill-rate
The full mutation aggregate SHALL classify every raw-master candidate into exactly one executed native outcome or exact policy-excluded outcome, report caught, missed, timed-out, unviable, excluded, and unresolved counts separately, and compute both raw and adjusted viable kill rates without allowing exclusions, unviable candidates, missing candidates, or unresolved outcomes to improve the raw metric. A command or shard timeout SHALL fail the run. The first floor SHALL be established only from a complete retained 16-shard run; later floors SHALL be monotonic and SHALL rise only from retained passing evidence. The agreed hard v0.4 viable-mutation targets are 90% raw and 95% adjusted; CI, review, and release SHALL fail below either target, and no target-gap record SHALL waive it.

#### Scenario: Agreed mutation target is missed
- **WHEN** a complete aggregate meets its historical mutation floor but its agreed raw or adjusted viable-mutation target is not met
- **THEN** CI/review and release remain failed and the tracking issue stays open until eligible evidence meets the target

#### Scenario: A complete baseline establishes the floor
- **WHEN** the first 16-shard aggregate has complete inventory, complete dispositions, valid exclusions, and no infrastructure failure
- **THEN** policy may record its measured viable kill rate as the initial floor

#### Scenario: No complete baseline exists
- **WHEN** only an estimate, partial shard set, or prior tool version is available
- **THEN** no mutation-strength percentage is claimed or committed as the measured floor

#### Scenario: Mutation strength regresses
- **WHEN** a later complete aggregate reports a viable kill rate below the committed floor
- **THEN** the full mutation gate fails even if ordinary tests and coverage pass

#### Scenario: The kill-rate floor is lowered
- **WHEN** policy proposes a mutation floor below its merge-base value
- **THEN** ratchet validation fails

#### Scenario: Outcomes are unresolved
- **WHEN** any master candidate lacks a native outcome or valid exception disposition
- **THEN** the aggregate fails its 100% disposition requirement

### Requirement: Bounded execution and fail-closed statuses
Every nextest, doctest, coverage, changed-mutation, full-inventory, mutation-shard, aggregation, and release-consumption job SHALL declare an explicit job timeout, and every long-running native command SHALL use the applicable command/test/build/mutant timeout. Status handling SHALL distinguish test failure, baseline failure, missed mutant, mutant timeout, command timeout, job timeout, cancellation, missing tool, empty inventory, corrupt output, and infrastructure failure. No retry, `continue-on-error`, empty default, or artifact-upload step SHALL reinterpret a required failure as success.

#### Scenario: A command exceeds its timeout
- **WHEN** any required command exceeds its declared ceiling
- **THEN** its job fails with a timeout status and retains available diagnostics

#### Scenario: A workflow is cancelled
- **WHEN** a required platform or shard job is cancelled before complete evidence is published
- **THEN** aggregate and release gates treat it as incomplete failure

#### Scenario: A run is intentionally repeated
- **WHEN** a maintainer reruns a failed or flaky quality job
- **THEN** the new attempt receives a distinct evidence identity and the prior failure remains retained rather than being erased

#### Scenario: Upload executes after failure
- **WHEN** an `if: always()` artifact step succeeds after the quality command failed
- **THEN** the job conclusion remains failed

### Requirement: Release, local workflow, and documentation parity
The release workflow SHALL block packaging and publication until independent quality evidence for the exact release commit satisfies nextest, stable doctest, every platform coverage floor and agreed target, changed-source mutation where applicable, and a complete current 16-shard mutation floor and agreed viable target. `.githooks/pre-push` and `docs/workflow.md` SHALL publish the exact bounded local commands and pinned versions, distinguish default local checks from the intentional full mutation run, and preserve the existing strict Rust and ProjectAtlas verification flow.

Repository workflow-policy E2E tests SHALL enforce independent job names, dependency ordering, exact tool pins, explicit timeouts, stable doctest use, platform matrices, `if: always()` evidence uploads, 16-shard reconciliation, release commit binding, hook commands, and documentation parity.

#### Scenario: Release evidence is complete
- **WHEN** every required manifest is passing, current, unexpired, and bound to the exact release commit and policy
- **THEN** release packaging may proceed after the existing verification gates

#### Scenario: Release evidence is stale or incomplete
- **WHEN** any platform, shard, target agreement, tool pin, or exact-commit manifest is missing or ineligible
- **THEN** packaging and publication do not start

#### Scenario: Local guidance drifts from CI
- **WHEN** the pre-push hook or workflow documentation omits or weakens a required bounded command
- **THEN** workflow-policy E2E validation fails

#### Scenario: The full mutation command is documented
- **WHEN** a developer reads the local workflow
- **THEN** it clearly identifies the expensive full 16-shard verification as intentional and does not run it implicitly on every push

### Requirement: Rust-native minimal architecture and strict quality gates
Implementation SHALL prefer native tool configuration and GitHub workflow composition. A typed validator MAY extend the existing `projectatlas-lints` binary only where native tools cannot enforce cross-platform policy, exception lifecycle, evidence binding, or shard aggregation. It SHALL use concrete structs, closed enums, exhaustive matching, typed errors, and existing workspace serialization crates; it SHALL NOT add a production crate, runtime service, trait hierarchy, unsafe code, or shared mutable orchestration state for this change.

Every Rust/config/workflow implementation slice SHALL include focused unit, integration, E2E, or smoke evidence appropriate to its failure mode and SHALL pass `cargo fmt --check`, `cargo check --workspace --all-targets --all-features --locked`, `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`, workspace tests, stable doctests, strict rustdoc, source lints, and the new quality gates with no warnings.

#### Scenario: Native configuration is sufficient
- **WHEN** nextest, LLVM coverage, or cargo-mutants natively enforces a rule
- **THEN** implementation uses the native option or configuration instead of duplicating it in Rust

#### Scenario: Typed aggregation is necessary
- **WHEN** cross-file evidence or shard reconciliation cannot be enforced by a native exit code
- **THEN** the smallest owning `projectatlas-lints` module uses concrete typed inputs and errors with focused tests

#### Scenario: An unnecessary abstraction is introduced
- **WHEN** implementation adds a new production crate, one-implementation trait, generic framework, unsafe block, or runtime service without a required variability boundary
- **THEN** architecture review and strict policy tests reject the change

#### Scenario: A strict Rust gate warns or fails
- **WHEN** format, check, Clippy, test, doctest, rustdoc, source-lint, coverage, or mutation verification is not clean
- **THEN** CI and release remain blocked

### Requirement: Quality-gate self-verification
The quality policy and validator SHALL have focused fixtures that prove valid inputs pass and malformed policy, threshold regression, scope reduction, expired or widened exception, wrong root, stale commit, wrong platform, wrong tool version, empty tests, empty mutants, missing merge base, incomplete native output, missing/duplicate/foreign shard, unresolved disposition, kill-rate regression, timeout, cancellation, and artifact mismatch fail. Workflow smoke tests SHALL exercise command wiring and failure propagation rather than checking only for text presence.

#### Scenario: A valid fixture is evaluated
- **WHEN** a complete fixture satisfies policy, identity, coverage, inventory, disposition, and ratchet rules
- **THEN** unit and integration validation pass deterministically on every supported host

#### Scenario: A failure fixture is evaluated
- **WHEN** one required invariant is violated
- **THEN** the focused test receives a nonzero result and a typed diagnostic naming that invariant

#### Scenario: Workflow failure propagation is smoked
- **WHEN** a controlled nextest, doctest, coverage, or mutation fixture fails in the workflow harness
- **THEN** the corresponding independent job fails while its diagnostic artifact remains available

### Requirement: Task-specific unit-test evidence and checklist synchronization
Every OpenSpec task SHALL declare at least one unique stable task-specific unit-test identifier. Before a task is marked complete, a machine-readable evidence ledger SHALL contain a successful run for every declared identifier bound to the exact assertion, command, tested implementation commit, normalized digest of every task-owned covered input, relevant platform, timestamp, and retained local or hosted result. Integration, E2E, smoke, coverage, mutation, or benchmark results MAY supplement but SHALL NOT replace the required unit-level assertion.

For source behavior, the identifier SHALL resolve to a focused Rust unit test. For workflow and configuration tasks, it SHALL resolve to a focused parser or policy assertion over the artifact. For planning, documentation, benchmark-policy, and other non-production artifacts, it SHALL resolve to an automated unit-level validator assertion over the artifact's required structure and claims rather than a fabricated production test.

The versioned verification plan SHALL store bounded commands as executable plus argument arrays, not shell strings, and SHALL declare covered inputs and timeout. Issue text and pull-request artifacts SHALL be display/evidence inputs only and SHALL never become executable commands. A hosted-run URL and success state SHALL be derived from validated repository, run ID/attempt, head SHA, job conclusion, artifact identity/digest, and Actions API state rather than trusted from caller-provided text.

After a passing run, one metadata-only closure commit MAY change only `tasks.md` checkbox state, task-evidence pointer/ledger metadata, and mapped GitHub issue state without invalidating the run. Validation SHALL recompute the covered-input digest and accept that closure only when every task-owned implementation, test, policy, configuration, documentation, and generated artifact byte remains identical to the tested inputs. Any covered-input change, assertion/command change, evidence-row rewrite that changes substantive identity, merge, or conflict resolution SHALL require a new successful run. The final PR aggregate gates SHALL still run on the PR head SHA.

`tasks.md` and its mapped GitHub checklist SHALL remain synchronized before check-in, status reporting, task completion, or PR review. Check-in and PR policy SHALL reject a missing or duplicate identifier, absent/failed/stale evidence, evidence whose tested commit or covered-input digest does not match the permitted state, an orphan ledger row, a completed task without all required successful runs, a closure commit that changes covered inputs, or local/remote checkbox drift.

Each mapped issue SHALL also expose clickable full-commit-SHA permalinks to the corresponding committed OpenSpec proposal, design, capability specifications, and task list. IssueOps SHALL resolve those links through the GitHub API, verify the mapped repository/change paths and referenced commit, and compare the committed task content with the authoritative local/remote checklist state. Missing, abbreviated-SHA, branch-only, foreign-repository, wrong-change, nonexistent, stale, or content-mismatched links SHALL block check-in, PR review, issue closure, and release.

Pull-request IssueOps SHALL resolve the GitHub issues explicitly linked by that PR and validate only the deterministic task ranges those issues authoritatively own. Every declared in-scope task SHALL be complete with current evidence, and changed artifacts SHALL be attributable to that declared scope. An unlinked PR, ambiguous range ownership, changed artifact outside scope, reordered task, duplicate task, extra remote task, or missing remote task SHALL fail. Other incomplete issues in the same release milestone SHALL NOT block an otherwise complete incremental PR. Release IssueOps SHALL retain full-milestone validation and SHALL block packaging until every authoritative issue/range and evidence record in the release milestone is complete.

PR validation SHALL execute untrusted branch tests only with read permissions and SHALL NOT use `pull_request_target` or another write-token path. A managed evidence renderer MAY run after a completed CI workflow only from trusted default-branch code with narrowly scoped `contents: read`, `actions: read`, `pull-requests: read`, and `issues: write`. Before updating an exact versioned marker, it SHALL validate source repository, source event, head repository/SHA, run ID/attempt, job conclusion, evidence schema/size/digests, and artifact identity. It SHALL escape Markdown cells, remain idempotent under retries/concurrent runs, never execute artifact/issue content, and refuse untrusted fork writeback without an explicitly authorized trusted run.

#### Scenario: A source task has focused evidence
- **WHEN** a source implementation task declares a unique Rust unit-test identifier and its focused assertion passes on an implementation commit whose covered-input digest remains unchanged through the metadata-only closure commit
- **THEN** the ledger may record the successful run and checklist synchronization may mark the task complete

#### Scenario: Closure commit changes a covered input
- **WHEN** the commit that checks a task or updates its evidence pointer also changes task-owned implementation, test, policy, configuration, documentation, or generated artifact bytes
- **THEN** the prior run is stale, task completion is rejected, and the task-specific test must run again before closure

#### Scenario: A documentation task has no production logic
- **WHEN** a documentation or planning task is ready for completion
- **THEN** its declared unit-test identifier runs an automated artifact assertion and records success rather than claiming unrelated production coverage

#### Scenario: Aggregate CI is green but task evidence is missing
- **WHEN** a task is checked locally or remotely without a successful current ledger row for every declared identifier
- **THEN** IssueOps, check-in, and PR policy fail even if the aggregate workflow passed

#### Scenario: Local and GitHub states drift
- **WHEN** a task checkbox or evidence-backed completion state differs between `tasks.md` and its authoritative GitHub issue
- **THEN** checklist validation fails before check-in, status reporting, closure, release, or PR review

#### Scenario: An issue links an uncommitted or stale specification
- **WHEN** a mapped issue omits its OpenSpec links or references a branch, abbreviated/foreign/wrong commit, wrong change path, missing artifact, or task content that differs from the authoritative committed checklist
- **THEN** IssueOps fails and names the invalid link before check-in, PR review, issue closure, or release

#### Scenario: Incremental PR shares an incomplete release milestone
- **WHEN** a PR links one authoritative phase issue whose declared tasks and evidence are complete while unrelated v0.4 issues remain open
- **THEN** PR IssueOps validates the linked scope without requiring the entire milestone to be complete

#### Scenario: Release contains an incomplete phase
- **WHEN** release validation finds any authoritative milestone issue, task range, or required evidence incomplete
- **THEN** release packaging remains blocked even if earlier incremental PRs passed their linked scopes

### Requirement: GitHub checklist authority within body limits
The primary mapped GitHub issue SHALL be the remote checklist index and SHALL identify the sole authoritative checkbox location for every local OpenSpec task. When the exact issue specification and checklist would exceed GitHub's 65,536-character body limit, IssueOps MAY create deterministic phase issues containing disjoint task ranges. The primary issue SHALL map those ranges, each task SHALL appear as authoritative in exactly one issue, and aggregate validation SHALL reconcile every phase with `tasks.md` and the evidence ledger. Summary or duplicated informational checkboxes SHALL NOT satisfy completion.

When the primary checklist fits, IssueOps SHALL keep it authoritative and SHALL render or update one non-checkbox managed evidence comment per top-level OpenSpec section using an exact marker. Each row SHALL expose task ID, unit-test ID, assertion, bounded command, tested commit, covered-input identity, derived retained run/artifact link, and validated status. Evidence comments SHALL not create a second authoritative checkbox. The body and generated evidence comments together SHALL keep every task's declared test and latest validated run visible on GitHub.

#### Scenario: One issue fits the complete checklist
- **WHEN** the exact mirrored specification and task checklist fit within the GitHub body limit
- **THEN** the primary mapped issue owns every authoritative checkbox, managed per-section comments expose test/run evidence, and no phase issue is created

#### Scenario: The primary issue would exceed the body limit
- **WHEN** generated issue content would exceed 65,536 characters
- **THEN** IssueOps splits disjoint deterministic task ranges into mapped phase issues while the primary issue indexes every authoritative range

#### Scenario: A task has two authoritative remote checkboxes
- **WHEN** the same local task is marked authoritative in the primary issue and a phase issue or in two phase issues
- **THEN** aggregate checklist validation fails as ambiguous authority

#### Scenario: A phase checklist drifts
- **WHEN** any authoritative phase issue changes task text, order, identifier, evidence state, or checked state independently of `tasks.md`
- **THEN** check-in and PR policy fail and name the drifting task and issue

#### Scenario: Remote checklist has reordered, duplicate, or extra tasks
- **WHEN** set membership would appear complete but the authoritative remote sequence contains a reordered, duplicate, or extra checkbox
- **THEN** IssueOps fails exact sequence and one-owner validation rather than accepting set equality

#### Scenario: Untrusted artifact attempts IssueOps writeback
- **WHEN** a fork artifact, issue field, or caller-provided URL/status/command has not been reconciled to a trusted completed run and digest
- **THEN** the renderer performs no write and reports the rejected provenance without executing the content
