## ADDED Requirements

### Requirement: Explicit local closing reference owns a pull request
ProjectAtlas IssueOps and the trusted PR-state workflow SHALL resolve a single unindented standalone closing reference (`Fixes #N`, `Closes #N`, or `Resolves #N`) to one local open issue before considering incidental issue numbers in the PR title or body. They MUST reject multiple distinct explicit local owners, foreign qualified references, and missing or mismatched issue/PR milestones. When no explicit closing line exists, the existing unique-local-reference compatibility rule SHALL remain.

#### Scenario: Dependabot changelog contains upstream issue numbers
- **WHEN** a PR body contains one standalone local `Fixes #N` line and HTML changelog links with other issue numbers
- **THEN** both gates select N as the sole owner and ignore the upstream links for ownership

#### Scenario: Explicit owners conflict
- **WHEN** a PR contains standalone closing lines for two distinct local issues
- **THEN** both gates refuse the PR as ambiguous rather than choosing either owner

#### Scenario: Foreign and embedded references do not become owners
- **WHEN** a PR has only foreign-repository links, HTML list links, or inline changelog `#number` text without an explicit local closing line
- **THEN** no explicit owner is inferred; the existing unique-local-reference fallback succeeds only if exactly one valid local reference remains

#### Scenario: Older one-reference PR remains valid
- **WHEN** a PR has no standalone closing line but exactly one local reference in its title or body
- **THEN** IssueOps and PR-state retain that issue as owner and still require matching milestones
