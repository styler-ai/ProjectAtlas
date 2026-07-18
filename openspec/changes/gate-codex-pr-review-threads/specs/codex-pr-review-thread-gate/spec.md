## ADDED Requirements

### Requirement: Codex Review Thread CI Gate
ProjectAtlas CI SHALL fail pull request verification while a GitHub Codex bot review thread remains unresolved.

#### Scenario: Unresolved Codex review thread exists
- **WHEN** a pull request contains a review thread with `isResolved = false`
- **AND** at least one thread comment author matches a configured GitHub Codex bot login
- **THEN** the CI gate SHALL fail.
- **AND** the failure output SHALL include the thread path, line when available, author, outdated state, and URL.

#### Scenario: Codex review thread is resolved
- **WHEN** a pull request contains a review thread with a GitHub Codex bot comment
- **AND** the thread has `isResolved = true`
- **THEN** the CI gate SHALL NOT fail because of that thread.

#### Scenario: Human-only unresolved review thread exists
- **WHEN** a pull request contains an unresolved review thread with no configured GitHub Codex bot author
- **THEN** this Codex-specific CI gate SHALL ignore that thread.

#### Scenario: Unresolved Codex review thread is outdated
- **WHEN** a pull request contains a GitHub Codex bot review thread with `isOutdated = true`
- **AND** the thread has `isResolved = false`
- **THEN** the CI gate SHALL still fail because outdated is not the same as resolved.

### Requirement: GitHub API Thread State
The Codex review gate SHALL use GitHub review-thread state rather than raw review-comment lists.

#### Scenario: Review comments are fetched
- **WHEN** the CI gate inspects a pull request
- **THEN** it SHALL query GitHub GraphQL review threads so it can read `isResolved`.

#### Scenario: Many review threads or comments exist
- **WHEN** GitHub paginates review threads or review-thread comments
- **THEN** the CI gate SHALL continue pagination until all relevant thread/comment pages are inspected.

### Requirement: Bounded CI Glue
The Codex review gate SHALL remain a small GitHub CI script and SHALL NOT become ProjectAtlas product logic.

#### Scenario: Script self-test runs
- **WHEN** CI runs the Codex review gate
- **THEN** it SHALL first run the script's local self-test.

#### Scenario: Bot login differs by API surface
- **WHEN** GitHub reports the Codex bot as `chatgpt-codex-connector` or `chatgpt-codex-connector[bot]`
- **THEN** the default gate configuration SHALL recognize both forms.

### Requirement: OpenSpec Task Review Surface
ProjectAtlas GitHub templates SHALL direct future planned bugs, features, improvements, and chores into the existing OpenSpec task checklist flow.

#### Scenario: New issue is created from a maintained template
- **WHEN** a bug, improvement, or chore issue is opened from a repository template
- **THEN** the issue body SHALL include an OpenSpec change field and an `OpenSpec Tasks` section for mirrored tasks.

#### Scenario: Placeholder task section exists before planning
- **WHEN** a template includes an `OpenSpec Tasks` section before real tasks are mirrored
- **THEN** the template SHALL NOT include fake checkbox tasks that can satisfy or pollute the checklist gate.

#### Scenario: Milestone readiness uses canonical OpenSpec tasks
- **WHEN** `.github/scripts/issue-checklists.py` validates a milestone for release readiness
- **THEN** it SHALL count checklist tasks only from exact `OpenSpec Tasks` or `OpenSpec Task Checklist` sections in the GitHub issue body
- **AND** it SHALL NOT count checkbox tasks from comments
- **AND** it SHALL NOT count checkbox tasks from unrelated generic headings such as `Completed Task Checklist`.

#### Scenario: Pull request is prepared for review
- **WHEN** a ProjectAtlas PR is opened
- **THEN** the PR template SHALL remind authors to map OpenSpec changes in `openspec/issue-map.json`, mirror task checklists into linked issues, check off every OpenSpec task before merge, and run `.github/scripts/issue-checklists.py`.
