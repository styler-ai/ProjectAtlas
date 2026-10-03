## ADDED Requirements

### Requirement: Explicit local ownership reference owns a pull request
ProjectAtlas IssueOps and the required base-controlled PR-state workflow SHALL resolve a single unindented standalone ownership reference (`Fixes #N`, `Closes #N`, `Resolves #N`, or non-closing `Refs #N`) to one local open issue before considering incidental issue numbers in the PR title or body. They MUST reject multiple distinct explicit local owners, foreign qualified references, and missing or mismatched issue/PR milestones. The required check MUST run protected workflow code, execute no PR-head code, and publish a status to the exact current PR head through a trusted writable path, including for Dependabot. A default-branch `workflow_run` publisher MUST validate the triggering workflow identity/event, re-read live owner/milestone metadata, refuse stale heads, and consume no candidate code or artifacts. Before every merge, the exact trusted workflow path, event, revision, current PR head, and independently API-published result MUST be verified; the shared Actions app and check name alone are not provenance proof. Same-repository write access remains trusted. When no explicit ownership line exists, the existing unique-local-reference compatibility rule SHALL remain; it is not origin-aware for a lone bare upstream `#N`.

#### Scenario: Dependabot changelog contains upstream issue numbers
- **WHEN** a PR body contains one standalone local `Fixes #N` line and HTML changelog links with other issue numbers
- **THEN** both gates select N as the sole owner and ignore the upstream links for ownership

#### Scenario: Explicit owners conflict
- **WHEN** a PR contains standalone closing lines for two distinct local issues
- **THEN** both gates refuse the PR as ambiguous rather than choosing either owner

#### Scenario: Foreign and embedded references do not override an explicit owner
- **WHEN** a PR has one explicit local closing line plus foreign-repository links, HTML list links, or inline changelog `#number` text
- **THEN** only the explicit local issue owns the PR

#### Scenario: Older one-reference PR remains valid
- **WHEN** a PR has no standalone closing line but exactly one local reference in its title or body
- **THEN** IssueOps and PR-state retain that issue as owner and still require matching milestones

#### Scenario: Dependabot requires writable current-head publication
- **WHEN** a Dependabot-triggered validation run cannot write a head status
- **THEN** a trusted default-branch follow-up with narrowly scoped write permission revalidates live metadata and publishes the result to the current head, refusing stale runs and candidate-provided results

#### Scenario: PR changes its own workflow
- **WHEN** a PR edits PR-state workflow or IssueOps files while claiming an owner
- **THEN** the required owner/milestone result comes from protected base workflow code, never from executed PR-head code, and is verified on the current PR head

#### Scenario: Aggregate issue remains open across partial delivery
- **WHEN** a bot PR has one standalone non-closing `Refs #499` line plus upstream changelog references
- **THEN** both gates select #499 without closing it, and reject any distinct explicit closing or non-closing owner
