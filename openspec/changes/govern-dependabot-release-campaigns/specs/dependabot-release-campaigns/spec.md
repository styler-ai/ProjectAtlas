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
The campaign issue SHALL contain one bounded machine-owned body region holding the union of all currently open Dependabot pull requests and every Dependabot pull request created, updated, closed, or merged from release-window start through campaign close. Automation SHALL update only that region, retain closed and merged records, update records idempotently by repository and PR number, and read back the resulting issue body.

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

### Requirement: Inventory identity and dispositions are complete
Every pull-request record SHALL bind repository, PR number, exact author, base, exact head, package ecosystem and update summary, first-seen and last-seen time, current PR state, milestone, campaign relationship, proof/review summary, and disposition. Final dispositions SHALL be exactly `accepted`, `deferred`, `declined`, or `superseded`; `pending` and `provisional` SHALL block campaign closure and release acceptance.

#### Scenario: Newly discovered update
- **WHEN** a Dependabot pull request first enters the release window
- **THEN** it is recorded as pending until reviewed and explicitly dispositioned

#### Scenario: Final disposition
- **WHEN** an authorized reconciliation records accepted, deferred, declined, or superseded with a repository-technical rationale
- **THEN** the record is final for that exact head and state

#### Scenario: Non-final record remains
- **WHEN** any pull-request record is pending or provisional
- **THEN** the campaign cannot close and the release cannot proceed

### Requirement: Campaign automation and CI planning remain separate
Campaign automation SHALL own intake, exact campaign relationship, inventory/disposition state, audit ingestion, and release reconciliation. `verify-affected-repository-contracts` SHALL exclusively own build, test, and quality selection. Neither capability SHALL duplicate the other's planner or mutable state.

#### Scenario: Dependabot head changes
- **WHEN** a Dependabot pull request receives a new head
- **THEN** campaign automation refreshes its inventory identity
- **AND** the shared affected-contract planner independently computes proof for that head

### Requirement: Dependabot IssueOps admission is exact and non-closing
The implementation context SHALL admit a Dependabot campaign pull request only when the pull-request event actor is exactly `dependabot[bot]` and authenticated `gh pr view` readback identifies the author exactly as the source-aware GitHub App login `app/dependabot`; its milestone maps to exactly one active campaign; its body contains exactly one standalone `Relates to #<campaign>` relationship and no closing campaign reference; and its PR number, base, exact head, body, milestone, and current inventory record all agree. Every other author, App, and bot SHALL continue through the normal implementation-owner contract or fail.

#### Scenario: Exact campaign relationship
- **WHEN** the exact `dependabot[bot]` event actor and exact `app/dependabot` PR-author readback have the active release milestone, one non-closing campaign relationship, and a matching current inventory record
- **THEN** `issueops-implementation` may accept campaign ownership while normal protected proof continues

#### Scenario: Bot or mapping mismatch
- **WHEN** the source-aware event actor or PR author identity, campaign declaration, relationship count/type, milestone, PR number, base, head, body, or inventory record is missing or mismatched
- **THEN** `issueops-implementation` fails

#### Scenario: Human attempts campaign path
- **WHEN** a human or another bot uses the campaign relationship shape
- **THEN** the Dependabot-only admission path rejects it

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

### Requirement: Weekly hosted Dependabot remains the pull-request producer
GitHub-hosted Dependabot SHALL remain the only automated pull-request producer and SHALL continue to use `.github/dependabot.yml` from the default branch. Campaign automation and the manual audit SHALL NOT claim or automate a public REST trigger for hosted version updates and SHALL NOT drive the GitHub UI.

#### Scenario: Weekly update production
- **WHEN** a configured Dependabot schedule runs
- **THEN** GitHub-hosted Dependabot produces or updates pull requests and campaign intake reconciles their events

#### Scenario: Manual release audit
- **WHEN** a pre-RC or pre-stable dependency audit is required
- **THEN** the CLI harness launches the repository's `workflow_dispatch` with `gh workflow run`
- **AND** it does not invoke a hosted-update REST endpoint or UI automation

### Requirement: Dependency audit is official pinned bounded and sequential
The trusted default-branch workflow SHALL provide one manually dispatchable dependency-audit job driven solely by the current `.github/dependabot.yml`. It SHALL validate supported entries, run them sequentially with an immutable official Dependabot CLI source pin and OCI-digest pins for every official updater/proxy image, use least job-level permissions and the official proxy isolation contract, and SHALL NOT embed `github/dependabot-action` or add a second `workflow_run` handoff.

#### Scenario: Supported audit configuration
- **WHEN** the audit is dispatched from the trusted default branch
- **THEN** one job processes each `.github/dependabot.yml` entry sequentially with pinned official tooling and images

#### Scenario: Configuration tooling or network failure
- **WHEN** configuration is invalid, an immutable pin cannot be verified, tooling fails, rate/API access fails, or any entry does not complete
- **THEN** the audit result is failed and release readiness is blocked

#### Scenario: Untrusted branch cannot own ingestion
- **WHEN** a pull-request branch changes audit code or configuration
- **THEN** it cannot use the trusted campaign-write permission or replace the default-branch workflow authority

### Requirement: Audit outcomes feed the campaign inventory
Each audit SHALL emit exactly `clean`, `findings`, or `failed` with exact configuration and tool identity into the same machine-owned campaign region. Findings SHALL create or refresh pending records; failed SHALL block release; clean SHALL record successful absence of new operations but SHALL NOT finalize another pending or provisional record.

#### Scenario: Clean audit
- **WHEN** every configured entry completes and reports no would-be update operation
- **THEN** the campaign records clean for that audit identity while preserving all existing record dispositions

#### Scenario: Audit findings
- **WHEN** one or more would-be update operations are reported
- **THEN** the campaign records findings and creates or refreshes pending inventory items for reconciliation

#### Scenario: Failed audit
- **WHEN** any audit entry fails or does not complete
- **THEN** the campaign records failed and release acceptance remains blocked

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
- **THEN** they begin provisional accepted and block release until each receives a final disposition
