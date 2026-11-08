## Purpose

Accept the complete large-project indexing performance release through real installed behavior and independently verified publication, while preserving existing compatibility and project state.

## ADDED Requirements

### Requirement: The release has one feature-free acceptance owner
#715 SHALL own `v0.5.1-00` without implementing product changes. Every accepted implementation issue SHALL have #715 as its sole direct parent and directly block release acceptance. The complete #708 scope SHALL be admitted and accepted before candidate publication.

#### Scenario: Performance work enters the release
- **WHEN** #708's complete 0.5.1 specification is synchronized
- **THEN** its milestone, parent, blockers and declared graph agree, and the old 0.6.0 membership is removed

#### Scenario: Initial planning graph
- **WHEN** only the release-owner plan has been admitted
- **THEN** the singleton graph records planning ownership and does not count the performance work as implemented or authorize candidate acceptance

### Requirement: Acceptance binds the exact complete candidate
The release owner SHALL freeze accepted main and exact artifacts only after every child, required review and compatibility disposition is complete. Changed behavior-relevant inputs SHALL invalidate the affected acceptance proof.

#### Scenario: Unfinished performance work
- **WHEN** any required indexing path, performance target, correctness case or review remains incomplete
- **THEN** candidate acceptance and publication remain blocked

#### Scenario: Windows linker metadata breaks exact runtime identity
- **WHEN** matching ordinary Windows x64 MSVC shipping inputs are constructed independently for measurement and publication
- **THEN** the normal target configuration produces reproducible linker metadata, and the Windows package gate verifies two independent fresh optimized target-tree constructions have identical complete executable SHA and size before packaging
- **AND** default features, shipping optimization, debug/installer capabilities, all native package gates, other targets' compiler arguments and the existing construction bound remain intact
- **AND** configuration checks or cached copies do not replace real native construction; a mismatch retains both original identities and bytes and blocks acceptance without PE postprocessing, metadata masking or a weaker identity
- **AND** changed Windows build inputs receive the preregistered existing full Windows performance campaign with unchanged samples, limits and controls before publication; unaffected platform measurements are retained only after their own exact runtime and input reconciliation

### Requirement: Every supported installed route is exercised
The candidate SHALL reconcile and safely execute the complete CLI command/nested-command and MCP tool inventory, including unchanged routes, on Windows x64, Linux x64, macOS Intel and Apple Silicon as supported. Mutations SHALL use isolated project and host state.

#### Scenario: Installed navigation and administration
- **WHEN** native CLI, MCP, plugin hooks and generated host configurations are exercised
- **THEN** versions, root/database bindings, actual behavior, typed refusals, output formats and bounds match their accepted contracts without ambient mutations

#### Scenario: Wrong root or missing index
- **WHEN** a selected root/database mismatches or initialized state is missing
- **THEN** the installed candidate returns typed recovery guidance without substituting or resetting a database

#### Scenario: Installer fixtures preserve the operator installation
- **WHEN** an installer acceptance fixture constructs or runs a native installer child
- **THEN** its host home, profile, Codex and XDG paths are explicitly bound to fixture-owned state, with controlled per-test overrides retained
- **AND** missing fixture-home authority is rejected before spawning instead of inheriting the operator home
- **AND** an older runtime, forwarder and host state outside that fixture remain unchanged while the installer keeps its intentional operator upgrade/quarantine behavior inside the selected test context

#### Scenario: Installer and readiness children inherit conflicting outer state
- **WHEN** the acceptance host exports an XDG state directory outside the installer fixture
- **THEN** the readiness-hook child uses the fixture state directory that owns the installer's receipt, while stale runtime and registry/configuration checks remain effective
- **AND** outside state and operator runtime/configuration remain unchanged; the product continues to respect explicitly selected XDG paths

#### Scenario: Native MCP acceptance failures retain causal evidence
- **WHEN** a native MCP acceptance request fails or reaches its existing deadline
- **THEN** the fixture records adapter/root/phase timing and actual MCP child state separately from the profiler outcome, and retains the typed primary failure alongside clean child completion evidence or any shutdown failure
- **AND** stderr collection retains a fixed prefix and total/truncation metadata while draining the remaining bytes, bounded diagnostic artifacts use an explicit host directory outside mounted fixture images, queued stdout producers cannot block reader cleanup, and owned diagnostic files are removed
- **AND** the original request deadline, single attempt and failure outcome remain authoritative; diagnostics do not retry an operation or replace its required acceptance proof

#### Scenario: Isolated native host needs administrative utilities
- **WHEN** the clean Mac host setup selects fixture-owned home and runtime paths for native acceptance
- **THEN** its selected `PATH` retains trusted standard bin/sbin directories without inheriting operator paths or losing fixture-runtime precedence
- **AND** the privileged Mac owner-change control selects trusted native utilities and reports command status/output and writer/observed UID on failure, while all ACL, ownership and foreign-entry checks remain required
- **AND** the existing causal fixture gate exercises the actual setup and utility discovery, while acceptance still requires the corrected real ARM ownership and native route within its original bounds

#### Scenario: Native image construction encounters a busy host resource
- **WHEN** macOS fixture construction exits with status 1 and the exact diagnostic `hdiutil: create failed - Resource busy`
- **THEN** setup may make at most three construction attempts using fresh private image paths, retaining each attempt's error and requiring verified absence of attachments for every preceding attempt before retrying
- **AND** an existing output, another error, a mapped image or unverifiable attachment state stops construction without overwriting or forcibly detaching an image
- **AND** teardown refuses to delete the fixture while any image beneath it remains mapped, including a partial creation without an explicit attachment receipt
- **AND** all four filesystem variants and both Mac architectures retain their required native checks, original deadlines and one execution per successfully constructed fixture; setup recovery never retries a failed product request or numerical cohort

#### Scenario: Cold document contracts require optimized construction
- **WHEN** the native document acceptance executable has not been compiled for the required release profile
- **THEN** CI constructs the locked `e2e_navigation` target with `--no-run` before the existing document resource step, within a separate 60-minute Intel or 30-minute other-platform construction bound
- **AND** the resource step retains its default shipping build after precompilation, original 30-minute bound, sampler limits, samples and required execution selectors
- **AND** completed reports survive a later failure, while a successful resource step without its required report fails instead of silently skipping artifact validation
- **AND** compilation, console medians without complete report identity, and skipped execution do not establish product acceptance

### Requirement: Security and review findings receive current evidence-based disposition
The release owner SHALL evaluate current dependency, scanner and automated-review findings against exact accepted source and supported trust boundaries before RC1. Confirmed defects and material unresolved proof gaps SHALL block acceptance. Repairs SHALL preserve legitimate product capabilities.

#### Scenario: A finding arrives after dependency remediation is accepted
- **WHEN** a scanner or self-hosted workflow reports a new potential defect after #732 closes
- **THEN** task 2.2 evaluates reachability, safeguards, positive workflows and proof gaps without blindly dismissing the lead or treating it as a confirmed vulnerability
- **AND** a confirmed defect enters its owning RC1 child such as #731, #738, #739 or #741, and publication waits for its independently reviewed, accepted correction and affected installed proof

### Requirement: Shipped guidance explains intent, triggers and evidence limits
The packaged skill SHALL explain when and why to use the existing command families, require exact-checkout Atlas reorientation after every compaction, and guide bounded source/architecture/specification/freshness reflection during the active task. The short-command reference SHALL retain the corresponding functionality and call-trigger descriptions. Guidance SHALL preserve supported root selection and typed recovery without an automatic full scan or an inferred specification association.

#### Scenario: An agent resumes after compaction
- **WHEN** project, source-ownership or architecture context needed for the active task has been lost
- **THEN** the skill directs the agent to read repository continuity instructions, select its own exact checkout and use the existing Atlas startup funnel to recover that context
- **AND** existing valid project state and authored purposes are preserved, with refresh selected from actual freshness evidence

#### Scenario: An agent reflects before completion
- **WHEN** a meaningful task boundary, review feedback or completion review prompts reconsideration of obsolete paths, duplication, simpler designs or stale requirements
- **THEN** the skill uses bounded navigation and current source/caller/export/registry evidence, records uncertainty and keeps improvements within the authorized task
- **AND** partial graphs, generated purposes, resolved links or zero inbound rows are not treated as proof of dead code or specification conformance

#### Scenario: The reviewed guide reaches an installed candidate
- **WHEN** native packages and plugin delivery are accepted
- **THEN** the installed skill and command reference match the reviewed candidate's guidance and version bindings
- **AND** the separate complete specification/source relation and purpose-integration verification remains outside RC1 under #751

### Requirement: Performance proof covers the complete indexing topic
Acceptance SHALL verify #708's preregistered speed, work-count and resource targets using the exact optimized installed candidate with normal telemetry, applying only its explicitly accepted RC1 tradeoffs without changing original verdicts. Initial indexing, full reindexing, cold CLI refresh, warm MCP, no-op, sparse/dense changes and required reconciliation SHALL have distinct measured cases.

#### Scenario: Complete performance result
- **WHEN** representative large repositories are tested on supported platforms
- **THEN** every required path meets its own target or its specifically authorized #708 RC1 tradeoff with correct complete results; faster warm MCP or a longer timeout cannot substitute for another path's missing improvement

### Requirement: Upgrades and recovery preserve authored state
The installed candidate SHALL preserve native identity, approved purposes and the last valid publication through supported upgrades, concurrent operations, cancellation, deadlines and injected failures, with safe retry and compatible rollback proof.

#### Scenario: Interrupted refresh or upgrade
- **WHEN** the operation fails or is cancelled at an owning failure boundary
- **THEN** no partial index or wrong-root state is exposed and recovery preserves authored data

#### Scenario: Confirmed Python fix upgrades an unchanged stable index
- **WHEN** the exact installed candidate refreshes an unchanged 0.5.0 index containing the confirmed parenthesized Python import reproduction
- **THEN** both local import targets and supported aliased callers resolve, native identity and approved purpose text/status survive, and the next unchanged refresh does not publish again
- **AND** predecessor rollback preserves authored state, with required one-time semantic reconciliation measured separately from current-contract opens

#### Scenario: Candidate provenance advances the semantic contract
- **WHEN** the candidate's version-bound accepted language set changes the persisted derivation contract from the published 0.5.0 contract
- **THEN** each exact native package passes the existing semantic-upgrade cohort against a checksum-verified 0.5.0 runtime with three paired samples and unchanged latency/RSS targets
- **AND** the affected proof verifies complete required parsing and graph/Python results, exactly one publication, native identity and approved-purpose preservation, a subsequent no-op and compatible rollback
- **AND** the explicit diagnostic may select only that cohort while preserving the default full campaign, its manual-only guard, the separate 0.4.5 schema proof and accepted unaffected measurements; a scoped result cannot represent a complete matrix

#### Scenario: Only one platform has missing or invalidated measurements
- **WHEN** an operator explicitly enables the prepublication indexing diagnostic and selects `all` or one of the four supported platform labels
- **THEN** selection gates only measurement preparation, execution and output; every native package and installed-route gate remains required, and the default remains `all`
- **AND** unknown labels or a non-`all` selection without explicit performance opt-in fail before packaging instead of silently skipping every measurement
- **AND** earlier reports are reused only after positive exact measured-runtime, container/checksum, baseline, harness/fixture/target/requirement, interpreter/OS/toolchain/configuration and measurement-step reconciliation; routing-only changes need explicit comparison rather than a workflow-file exemption
- **AND** original report provenance, samples, limits and failures remain intact, every missing or affected result is obtained, and a skipped cohort without accepted matching evidence cannot satisfy release acceptance

#### Scenario: Scoped upgrade execution does not imply detailed-navigation samples
- **WHEN** the explicitly selected campaign is `semantic-upgrade`
- **THEN** it retains every scoped latency/RSS, parsing/graph, state, no-op and rollback assertion and its original report, without executing or claiming the supplemental full-navigation profiler
- **AND** the full campaign still runs its selected supplemental attribution and upload within their original bounds, requires its original detailed samples and fixtures, and refuses an incomplete full report
- **AND** causal scope-routing and incomplete-report controls pass without repeating an unchanged numerical cohort or reclassifying the original failed workflow

### Requirement: Publication and closure follow verified acceptance
An authorized 0.5.1 RC SHALL be a non-draft prerelease with Latest excluded. Later authorized stable promotion SHALL repeat exact hosted and installed proof and independently verify assets, checksums, identities, Latest and downstream pins. #715 SHALL close last after every child and stable readback pass.

#### Scenario: Candidate publication
- **WHEN** complete acceptance and candidate publication authority exist
- **THEN** tag/head, assets and installed behavior are independently read back while the preceding stable remains Latest

#### Scenario: Final stable closeout
- **WHEN** stable promotion and complete readback succeed
- **THEN** 0.5.1 is Latest, current feedback and task mirrors are reconciled, and #715 closes before the milestone is finalized


#### Scenario: Reuse of the specifically accepted Windows warm result
- **WHEN** #708 has accepted the exact Windows report from campaign `42489987038` under its narrowly specified RC1 warm-latency tradeoff
- **THEN** the release owner retains the original failed target and workflow conclusion alongside the separately authorized acceptance decision, and independently verifies complete data and every other required assertion and workflow contract
- **AND** documentation-only source revisions may reuse the reports and official clean parser handoff only after positive behavior-input and exact runtime/package reconciliation, keeping original producer and report revisions
- **AND** the publishing workflow still executes all four native Codex-convergence and installed-product gates before publication, and all four post-publication installer gates afterward; the tradeoff waives none of those gates
