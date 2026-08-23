## ADDED Requirements

### Requirement: One active campaign issue per release
Each release/version that admits Dependabot updates SHALL declare exactly one active `dependabot_campaign_issue` in its owning release graph. The campaign SHALL be exactly one direct native child of the unparented release-acceptance issue, carry the same milestone, and use native blockers only for genuine prerequisites. The system SHALL NOT create one issue per Dependabot pull request or maintain a second campaign graph.

#### Scenario: Valid release campaign
- **WHEN** a release graph declares an active campaign
- **THEN** exactly one mapped issue is the campaign, is a direct child of the release root, and carries that release milestone

#### Scenario: Missing or conflicting campaign ownership
- **WHEN** a release has zero or multiple active campaign declarations, the campaign belongs to another parent or milestone, or native relationships differ from the declaration
- **THEN** campaign and release readiness fail closed

### Requirement: One machine-owned release-window inventory
The campaign issue SHALL contain one bounded machine-owned body region holding the exact published campaign-contract activation revision plus the union of all currently open Dependabot pull requests and every Dependabot pull request created, updated, closed, or merged from release-window start through campaign close. Automation SHALL update only that region, retain closed and merged records, update records idempotently by repository and PR number, and read back the resulting issue body.

#### Scenario: Existing and new pull requests are admitted
- **WHEN** a campaign is initialized or reconciled
- **THEN** its inventory contains every currently open Dependabot pull request plus every release-window Dependabot event record

#### Scenario: Pull request closes or changes head
- **WHEN** an inventoried pull request is updated, closed, merged, or superseded
- **THEN** the existing record is updated with the new exact head/state/timestamps and is not deleted

#### Scenario: Human prose surrounds the inventory
- **WHEN** automation reconciles inventory state
- **THEN** only the delimited machine-owned region changes
- **AND** the lean issue sections and single OpenSpec task mirror remain byte-for-byte outside that region

### Requirement: Inventory identity dispositions provenance and stage are complete
Every pull-request record SHALL bind repository, PR number, exact author, base, exact head, package ecosystem and update summary, first-seen and last-seen time, current PR state, milestone, campaign relationship, proof/review summary, and disposition. Final dispositions SHALL be exactly `accepted`, `deferred`, `declined`, or `superseded`; `pending` and `provisional` SHALL block the applicable campaign checkpoint. Provenance, including `retrospective_precontract`, SHALL be recorded separately and SHALL NOT act as a final disposition or imply a gate that did not run.

The same machine-owned region SHALL record exactly one stage from `collecting`, `candidate_ready`, and `stable_ready`. A ready stage SHALL bind the exact candidate or stable revision, inventory digest, full-union reconciliation high-water mark, `.github/dependabot.yml` digest, audit run and outcome, immutable CLI/updater/proxy identities, and timestamp. Missing, malformed, stale, or mismatched stage evidence SHALL be `collecting` for readiness purposes.

#### Scenario: Newly discovered update
- **WHEN** a Dependabot pull request first enters the release window
- **THEN** it is recorded as pending until reviewed and explicitly dispositioned

#### Scenario: Final disposition
- **WHEN** an authorized reconciliation records accepted, deferred, declined, or superseded with a repository-technical rationale
- **THEN** the record is final for that exact head and state

#### Scenario: Non-final record remains
- **WHEN** any pull-request record is pending or provisional
- **THEN** the applicable candidate-ready or stable-ready checkpoint fails

#### Scenario: Provenance is confused with disposition
- **WHEN** a record has `retrospective_precontract` provenance but lacks a final disposition and complete retrospective evidence
- **THEN** the record remains non-final
- **AND** no historical review or authorization is inferred

### Requirement: Campaign automation and CI planning remain separate
Campaign automation SHALL own intake, exact campaign relationship, inventory/disposition state, audit ingestion, and release reconciliation. `verify-affected-repository-contracts` SHALL exclusively own build, test, and quality selection. Neither capability SHALL duplicate the other's planner or mutable state.

#### Scenario: Dependabot head changes
- **WHEN** a Dependabot pull request receives a new head
- **THEN** campaign automation refreshes its inventory identity
- **AND** the shared affected-contract planner independently computes proof for that head

### Requirement: Post-contract Dependabot IssueOps admission is exact and non-closing
The implementation context SHALL admit a Dependabot campaign pull request opened or updated after campaign-contract activation only when the pull-request event actor is exactly `dependabot[bot]` and authenticated `gh pr view` readback identifies the author exactly as the source-aware GitHub App login `app/dependabot`; its milestone maps to exactly one active campaign; its body contains exactly one standalone `Relates to #<campaign>` relationship and no closing campaign reference; and its PR number, base, exact head, body, milestone, and current inventory record all agree. Every other author, App, bot, or post-contract pull request missing any pre-merge gate SHALL continue through the normal implementation-owner contract or fail; it SHALL NOT use the retrospective path.

#### Scenario: Exact campaign relationship
- **WHEN** the exact `dependabot[bot]` event actor and exact `app/dependabot` PR-author readback have the active release milestone, one non-closing campaign relationship, and a matching current inventory record
- **THEN** `issueops-implementation` may accept campaign ownership while normal protected proof continues

#### Scenario: Bot or mapping mismatch
- **WHEN** the source-aware event actor or PR author identity, campaign declaration, relationship count/type, milestone, PR number, base, head, body, or inventory record is missing or mismatched
- **THEN** `issueops-implementation` fails

#### Scenario: Human attempts campaign path
- **WHEN** a human or another bot uses the campaign relationship shape
- **THEN** the Dependabot-only admission path rejects it

#### Scenario: Post-contract PR attempts retrospective admission
- **WHEN** a Dependabot pull request merged or seeks merge on or after the published campaign-contract activation revision
- **THEN** `retrospective_precontract` is rejected
- **AND** the complete exact admission and pre-merge authorization path remains mandatory

### Requirement: Merge authorization remains explicit and complete
Dependabot pull requests SHALL NOT auto-merge or bypass required proof. `issueops-merge-authorized` SHALL succeed only after an explicit Sol dispatch bound to the exact head and readback of exact author, campaign mapping, PR number/head/body/milestone, inventory record, accepted review decision, zero unresolved threads, branch synchronization, and every required protected context. A new head SHALL invalidate prior authorization.

#### Scenario: Exact-head authorization
- **WHEN** all required readbacks are current and green and Sol dispatches authorization for that exact Dependabot head
- **THEN** the merge-authorization context may succeed without closing the campaign issue

#### Scenario: New commit or incomplete review
- **WHEN** the head changes, a required context is not successful, review is not accepted, or a thread remains unresolved
- **THEN** merge authorization fails until fresh proof, readback, and Sol dispatch complete

#### Scenario: No automatic merge
- **WHEN** a Dependabot pull request is admitted to a campaign
- **THEN** admission alone does not merge it, enable auto-merge, or weaken a branch-protection context

### Requirement: Pre-contract merged updates reconcile without fictional authorization
A pull request merged before the published campaign-contract activation revision MAY use `retrospective_precontract` provenance only. Final `accepted` SHALL require exact dependency head, merge commit, changed-file set, merge time, current-`main` ancestry/inclusion, a safe metadata-only post-hoc `Relates to #<campaign>` relationship and release milestone, fresh Sol review of the exact merged diff plus its behavior integrated into current `main`, fresh complete current-`main` protected and platform proof, zero unresolved actionable findings, and an explicit historical-gate-gap record. Original checks SHALL remain historical evidence and SHALL NOT satisfy later IssueOps, review, or authorization gates.

#### Scenario: Retrospective record passes current proof
- **WHEN** every retrospective identity, post-hoc metadata, current-main proof, Sol review, and finding readback is exact and successful
- **THEN** the record may be finally `accepted` while retaining `retrospective_precontract` provenance and the historical gap

#### Scenario: Retrospective proof or review fails
- **WHEN** current proof or retrospective review fails or has an unresolved actionable finding
- **THEN** the record SHALL NOT be accepted
- **AND** an actual revert, corrective, or superseding delivery SHALL pass normal current gates before the historical record becomes `declined` when fully reverted or `superseded` with the exact accepted successor

#### Scenario: Historical checks are present
- **WHEN** a pre-contract merge had successful checks before the later campaign and IssueOps contexts existed
- **THEN** those checks are recorded as historical evidence only
- **AND** no post-hoc pre-merge Sol authorization or review is claimed

### Requirement: Weekly hosted Dependabot remains the pull-request producer
GitHub-hosted Dependabot SHALL remain the only automated pull-request producer and SHALL continue to use `.github/dependabot.yml` from the default branch. Campaign automation and the manual audit SHALL NOT claim or automate a public REST trigger for hosted version updates and SHALL NOT drive the GitHub UI.

#### Scenario: Weekly update production
- **WHEN** a configured Dependabot schedule runs
- **THEN** GitHub-hosted Dependabot produces or updates pull requests and campaign intake reconciles their events

#### Scenario: Manual release audit
- **WHEN** a pre-RC or pre-stable dependency audit is required
- **THEN** the CLI harness launches the repository's `workflow_dispatch` with `gh workflow run`
- **AND** it does not invoke a hosted-update REST endpoint or UI automation

### Requirement: Dependency audit translation is exact official pinned bounded and sequential
The trusted default-branch workflow SHALL provide one manually dispatchable dependency-audit job driven solely by the current `.github/dependabot.yml`. One standard-library translator SHALL accept only a closed lossless mapping: top-level `version: 2` and `updates`; the current `cargo` and `github-actions` ecosystems mapped to official CLI ecosystem names; exactly one `directory`; `target-branch`; hosted `schedule.interval`; supported group names/rules/update types; supported allow/update and ignore/version/update-type policies; and any other field currently present in the repository configuration. It SHALL preserve allow-then-ignore precedence, use the official pinned CLI `update -f` job-description/output semantics, and fail before container startup on every unknown, unsupported, ambiguous, or lossy field.

Each entry SHALL run sequentially with an immutable official Dependabot CLI source pin and OCI-digest pins for its official updater and proxy images. Checked-in positive bounds SHALL cover per-entry CLI `--timeout`, total workflow timeout, per-entry and total output bytes, and per-entry and total recorded operation counts. The workflow SHALL NOT embed `github/dependabot-action`, run a parallel matrix, or add a second `workflow_run` handoff.

#### Scenario: Supported audit configuration
- **WHEN** the audit is dispatched from the trusted default branch
- **THEN** one job translates and processes each `.github/dependabot.yml` entry sequentially with pinned official tooling and images
- **AND** deterministic fixtures cover the current Cargo minor/patch group and current ungrouped GitHub Actions entry

#### Scenario: Unsupported or lossy configuration
- **WHEN** any configuration field or policy cannot be represented exactly in the pinned CLI job model
- **THEN** no updater or proxy container starts
- **AND** the audit records failed

#### Scenario: Configuration tooling or network failure
- **WHEN** configuration is invalid, an immutable pin cannot be verified, tooling fails, rate/API access fails, a positive timeout/output/operation bound is exceeded, cancellation or signal handling is incomplete, or any entry or cleanup does not complete certainly
- **THEN** the audit result is failed and release readiness is blocked

#### Scenario: Untrusted branch cannot own ingestion
- **WHEN** a pull-request branch changes audit code or configuration
- **THEN** it cannot use the trusted campaign-write permission or replace the default-branch workflow authority

### Requirement: Audit mutation credentials are isolated from updater execution
The audit checkout SHALL use `persist-credentials: false`. The issues-write token SHALL NOT appear in updater or proxy input, environment, mounts, arguments, credentials, logs, generated job data, or persisted Git configuration. The updater for this public repository SHALL use no repository token, or only a distinct read-only credential supplied to the official proxy and never to updater code. Every updater/proxy container and network SHALL exit, temporary outputs SHALL be bounded, schema-valid, and sanitized, and cleanup SHALL be verified before a later reconciliation step receives the issues-write token.

#### Scenario: Updater phase is inspected
- **WHEN** generated input, environment, mounts, arguments, proxy/updater configuration, logs, and checkout Git configuration are inspected
- **THEN** the issues-write token is absent from every surface

#### Scenario: Containers complete successfully
- **WHEN** all sequential updater entries finish
- **THEN** updater/proxy containers and networks exit and bounded output passes schema and sanitization checks before the issues-write token is injected into the reconciliation step

#### Scenario: Cancellation or cleanup is uncertain
- **WHEN** cancellation, signal propagation, container/network exit, temporary-output cleanup, or credential absence cannot be proven within its bound
- **THEN** reconciliation receives no issues-write token
- **AND** the audit records failed

### Requirement: Audit outcomes feed the campaign inventory
Each audit SHALL emit exactly `clean`, `findings`, or `failed` with exact configuration, repository revision, stage, and tool/image identity into the same machine-owned campaign region. `clean` and `findings` are successful complete executions; findings SHALL create or refresh pending records. `failed` SHALL block the applicable readiness checkpoint. `clean` SHALL record successful absence of new operations but SHALL NOT finalize another pending or provisional record.

#### Scenario: Clean audit
- **WHEN** every configured entry completes and reports no would-be update operation
- **THEN** the campaign records clean for that audit identity while preserving all existing record dispositions

#### Scenario: Audit findings
- **WHEN** one or more would-be update operations are reported
- **THEN** the campaign records findings and creates or refreshes pending inventory items for reconciliation

#### Scenario: Failed audit
- **WHEN** any audit entry, schema/sanitization check, bound, or cleanup fails or does not complete certainly
- **THEN** the campaign records failed and release acceptance remains blocked

### Requirement: Candidate-ready and stable-ready are separate release checkpoints
`candidate_ready` SHALL bind one exact RC candidate revision to a successful final pre-RC audit and the final full-union inventory reconciled through that checkpoint. It MAY permit `v0.5.0-rc1` publication only while #499 and #492 remain open. After independent RC acceptance, newly created or updated records SHALL return the campaign to `collecting`. `stable_ready` SHALL require a later successful pre-stable audit on accepted current `main`, final disposition of every audit finding and every record newly observed since candidate readiness, and exact full-window readback; only then MAY #499 close and unblock stable #492 acceptance.

#### Scenario: RC candidate checkpoint succeeds
- **WHEN** the pre-RC audit is complete as clean or findings, every resulting finding and inventory record is final for the exact candidate snapshot, and the publication preflight rereads matching revision/inventory/config/audit identities with no intervening event
- **THEN** the campaign records `candidate_ready`
- **AND** RC1 may publish while #499 and #492 remain open for the stable window

#### Scenario: New record appears before RC publication
- **WHEN** a Dependabot pull request is created or updated after candidate readiness but before RC publication readback
- **THEN** the active campaign returns to `collecting`
- **AND** RC publication is blocked until the candidate checkpoint is recomputed

#### Scenario: New record appears after accepted RC
- **WHEN** a Dependabot pull request is created or updated after RC publication is independently accepted
- **THEN** the active campaign returns to `collecting` without invalidating the accepted RC
- **AND** the new record must be final before stable readiness

#### Scenario: Stable checkpoint succeeds
- **WHEN** an accepted RC exists, the later pre-stable audit is complete as clean or findings, every resulting finding and every newly observed release-window record is final, and the exact full inventory readback matches
- **THEN** the campaign records `stable_ready`
- **AND** #499 may close so stable #492 acceptance can begin

#### Scenario: Stage evidence is stale or incomplete
- **WHEN** a stage revision, inventory digest, high-water mark, config digest, audit identity/outcome, immutable tool identity, or current union readback is missing, stale, malformed, or mismatched
- **THEN** the campaign is `collecting` for readiness purposes
- **AND** RC or stable publication is blocked at that checkpoint

### Requirement: v0.5.0 campaign sequencing and classification are explicit
The v0.5.0 campaign issue #499 SHALL be blocked by #498 and exact Rust #482. #498 SHALL be blocked by #497, while #497 SHALL have no blocker and SHALL NOT become an artificial blocker for independent product lanes. After #482 is accepted on `main`, open updates SHALL be delivered in order #453, #454, then #455, with each branch refreshed or rebased onto accepted current `main` before proof and disposition.

#### Scenario: Foundations unlock parallel work
- **WHEN** #497 is accepted on `main`
- **THEN** #498 and #482 may proceed in parallel when their own dependencies permit
- **AND** independent release lanes remain parallel

#### Scenario: Campaign delivery begins
- **WHEN** #498 and #482 are accepted on `main`
- **THEN** #499 may reconcile #453 for Rust/parser, then #454 for Rust/database/SQLite, then #455 for Rust/MCP

#### Scenario: Stale branch-global failure
- **WHEN** an existing Dependabot branch is red because stale shared IssueOps state predates current `main`
- **THEN** the campaign records baseline drift rather than a dependency regression
- **AND** refreshes or rebases the branch and reruns current affected proof before technical disposition

#### Scenario: Retrospective merged records
- **WHEN** #457 and #468 enter the v0.5.0 inventory
- **THEN** they begin provisional with `retrospective_precontract` provenance and explicit exact head/merge/history evidence
- **AND** each blocks the applicable readiness checkpoint until the complete retrospective contract yields a final disposition
