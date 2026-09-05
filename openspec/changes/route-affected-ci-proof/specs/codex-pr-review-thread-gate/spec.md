## MODIFIED Requirements

### Requirement: Codex Review Thread CI Gate
ProjectAtlas branch protection SHALL require every pull-request review conversation to be resolved, replacing the Codex-only Actions polling gate with GitHub's native live conversation-resolution rule.

#### Scenario: Unresolved Codex review thread exists
- **WHEN** a pull request contains an unresolved review conversation with a GitHub Codex bot comment
- **THEN** native required conversation resolution SHALL block the pull request without requiring a workflow rerun.

#### Scenario: Codex review thread is resolved
- **WHEN** the last unresolved Codex review conversation is resolved
- **THEN** native required conversation resolution SHALL refresh merge readiness without requiring a workflow rerun.

#### Scenario: Human-only unresolved review thread exists
- **WHEN** a pull request contains an unresolved human review conversation
- **THEN** native required conversation resolution SHALL block the pull request until that conversation is dispositioned and resolved.

#### Scenario: Unresolved review thread is outdated
- **WHEN** any review conversation is outdated but unresolved
- **THEN** native required conversation resolution SHALL continue to block because outdated is not resolved.

### Requirement: GitHub API Thread State
The review-conversation gate SHALL use GitHub's native conversation-resolution state rather than a workflow result derived from GraphQL polling.

#### Scenario: Review conversation state changes
- **WHEN** a review conversation is resolved or reopened
- **THEN** GitHub branch protection SHALL update readiness directly without an Actions event or API polling step.

#### Scenario: Many review conversations exist
- **WHEN** a pull request contains many human or automated review conversations
- **THEN** GitHub's native branch rule SHALL remain the single complete conversation-state authority.

### Requirement: Bounded CI Glue
The superseded Codex-only review polling script and CI step SHALL be removed rather than retained as duplicate or stale authority.

#### Scenario: Pull-request state workflow runs
- **WHEN** `pr-state` validates a pull request
- **THEN** it SHALL validate issue and milestone metadata without querying review threads.

#### Scenario: Review activity occurs without source change
- **WHEN** a human or automated review conversation is created, resolved, or reopened
- **THEN** no source compilation, Rust test, platform E2E, or review-thread polling workflow SHALL run because of that activity.
