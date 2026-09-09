## ADDED Requirements

### Requirement: Metadata events preserve source verification

Title, body, issue, and milestone activity SHALL validate current PR state without
creating, replacing, satisfying, or cancelling source proof or its required
`verify` context. Native conversation resolution SHALL remain authoritative for
review threads.

#### Scenario: Title or body changes after passing source proof
- **WHEN** an open pull request changes only its title or body
- **THEN** current issue and milestone validation runs without executing source steps
  or emitting a source aggregate, and valid protected merge eligibility is retained

#### Scenario: Metadata changes during incomplete retarget proof
- **WHEN** metadata changes after a base retarget whose exact comparison proof is
  pending, failed, skipped, cancelled, or missing
- **THEN** metadata validation cannot make that unverified comparison merge-ready

#### Scenario: Issue activity refreshes a previous retarget validation
- **WHEN** an owning issue changes after PR-state validation of a base retarget
- **THEN** live ownership validation refreshes without repeating source proof

### Requirement: Base retargets require exact source proof

A base retarget at an unchanged PR head SHALL execute the existing affected-proof
planner and selected proof against the actual new base and current head. Required
checks SHALL reject incomplete or unrelated proof, preserving GitHub Actions
application identity, existing protected contexts, and source concurrency rules.

#### Scenario: New base with unchanged head
- **WHEN** a pull request changes its target branch without a head commit change
- **THEN** the executed plan and aggregate bind to that new base and unchanged head
  and normal protected merge readiness requires successful selected proof

#### Scenario: Selected proof is unsuccessful
- **WHEN** any selected job is missing, skipped, cancelled, or failed
- **THEN** protected merge readiness fails without accepting prior proof of a
  different base comparison or a skipped source aggregate

#### Scenario: Source and metadata events overlap
- **WHEN** source synchronization or a base retarget overlaps metadata activity
- **THEN** only newer source work for the same PR can supersede source work and
  metadata activity cannot cancel it or overwrite its required outcome

### Requirement: Hosted protection proves event routing

Acceptance SHALL inspect real GitHub Actions runs, exact plan bindings, emitted
checks, and ordinary protected merge behavior after title/body edits and a base
retarget. Local workflow assertions SHALL complement that proof, not replace it.

#### Scenario: Protected PR is accepted
- **WHEN** the implementation is accepted for merge
- **THEN** real hosted events demonstrate metadata isolation, exact retarget proof,
  and normal protection with required `verify` and `pr-state` from GitHub Actions,
  without fabricated statuses, administrative bypass, or weakened requirements
