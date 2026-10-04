## ADDED Requirements

### Requirement: Explicit local ownership reference owns a pull request
ProjectAtlas IssueOps and the required base-controlled PR-state workflow SHALL resolve a single unindented standalone ownership reference (`Fixes #N`, `Closes #N`, `Resolves #N`, or non-closing `Refs #N`) to one local open issue before considering incidental issue numbers in the PR title or body. They MUST reject multiple distinct explicit local owners, mixed closing and non-closing references even when they name the same issue, standalone ownership declarations naming a foreign repository, and missing or mismatched issue/PR milestones. Ordinary foreign-qualified references and foreign-repository changelog links MUST NOT invalidate a PR with one valid explicit local owner or be selected as that owner. Repeated references of the same kind to the same issue SHALL remain valid. The required check MUST be a native job named `pr-state` in the protected-base `pull_request_target` workflow, execute no PR-head code, and report its result on the exact current PR head, including for Dependabot. The job MUST validate live owner/milestone metadata and refuse stale heads with read-only API permissions. Issue-event refresh MUST rerun that eligible source. A `workflow_run` API check MUST NOT be the required gate, because its green API result alone does not prove GitHub evaluates it for merging. Before every merge, the native check-run and its associated protected workflow path, pull_request_target event, workflow revision, current candidate head, successful result, and actual required-gate readiness MUST be read back independently through the API; the shared Actions app and check name alone are not provenance proof. Same-repository write access remains trusted. When no explicit ownership line exists, the existing unique-local-reference compatibility rule SHALL remain; it is not origin-aware for a lone bare upstream `#N`.

#### Scenario: Dependabot changelog contains upstream issue numbers
- **WHEN** a PR body contains one standalone local `Fixes #N` line and HTML changelog links with other issue numbers
- **THEN** both gates select N as the sole owner and ignore the upstream links for ownership

#### Scenario: Explicit owners conflict
- **WHEN** a PR contains standalone closing lines for two distinct local issues
- **THEN** both gates refuse the PR as ambiguous rather than choosing either owner

#### Scenario: Foreign and embedded references do not override an explicit owner
- **WHEN** a PR has one explicit local closing line plus foreign-repository links, HTML list links, or inline changelog `#number` text
- **THEN** only the explicit local issue owns the PR

#### Scenario: A standalone ownership declaration names a foreign repository
- **WHEN** a PR contains a standalone `Refs other/repo#518` line, with or without an explicit local owner
- **THEN** both gates refuse the non-local ownership declaration rather than treating it as incidental changelog context

#### Scenario: Closing and non-closing references name the same issue
- **WHEN** a PR contains both standalone `Refs #499` and `Closes #499` lines
- **THEN** both gates refuse the conflicting reference kinds rather than deduplicating them to one valid owner

#### Scenario: Repeated references preserve one ownership kind
- **WHEN** a PR repeats standalone non-closing references to one issue, or uses only closing references to that issue
- **THEN** both gates retain that issue as the sole owner

#### Scenario: Older one-reference PR remains valid
- **WHEN** a PR has no explicit ownership line but exactly one local reference in its title or body
- **THEN** IssueOps and PR-state retain that issue as owner and still require matching milestones

#### Scenario: Dependabot receives a native current-head result
- **WHEN** a Dependabot PR triggers the protected-base validation job with read-only token permissions
- **THEN** GitHub reports the native job result on the exact current PR head, while trusted validation refuses stale heads and candidate-provided results

#### Scenario: PR changes its own workflow
- **WHEN** a PR edits PR-state workflow or IssueOps files while claiming an owner
- **THEN** the required owner/milestone result comes from protected base workflow code, never from executed PR-head code, and is verified on the current PR head

#### Scenario: Aggregate issue remains open across partial delivery
- **WHEN** a bot PR has one standalone non-closing `Refs #499` line plus upstream changelog references
- **THEN** both gates select #499 without closing it, and reject any distinct explicit closing or non-closing owner

#### Scenario: A PR metadata change also emits an issues event
- **WHEN** an `issues` event contains an `issue.pull_request` payload
- **THEN** issue-contract validation and owner-issue refresh skip that payload; the native PR-state job still validates the PR's actual owner and milestone through its `pull_request_target` event

#### Scenario: A real owner issue changes
- **WHEN** an `issues` event addresses a real owner issue without an `issue.pull_request` payload
- **THEN** issue-contract validation remains active and owner refresh reruns the eligible native source for current PRs referencing that issue
