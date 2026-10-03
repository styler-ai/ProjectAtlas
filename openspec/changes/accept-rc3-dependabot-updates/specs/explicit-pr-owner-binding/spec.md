## ADDED Requirements

### Requirement: Explicit local closing reference owns a pull request
ProjectAtlas IssueOps and the required base-controlled PR-state workflow SHALL resolve a single unindented standalone closing reference (`Fixes #N`, `Closes #N`, or `Resolves #N`) to one local open issue before considering incidental issue numbers in the PR title or body. They MUST reject multiple distinct explicit local owners, foreign qualified references, and missing or mismatched issue/PR milestones. The required check MUST run protected workflow code, execute no PR-head code, and report a current-head status. When no explicit closing line exists, the existing unique-local-reference compatibility rule SHALL remain; it is not origin-aware for a lone bare upstream `#N`.

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

#### Scenario: PR changes its own workflow
- **WHEN** a PR edits PR-state workflow or IssueOps files while claiming an owner
- **THEN** the required owner/milestone result comes from protected base workflow code, never from executed PR-head code, and is verified on the current PR head
