## Purpose

Keep maintainer instructions accurate about protected-branch checks and the existing main-based integration workflow.

## ADDED Requirements

### Requirement: Manual diagnostics are distinguished from required checks

Maintainer guidance SHALL explain that successful workflow_dispatch job checks do not satisfy protected-branch requirements. It SHALL identify the existing eligible pull_request route and link the official GitHub event restriction.

#### Scenario: Manual jobs pass but a protected update remains blocked

- **WHEN** a maintainer consults the manual-CI fallback instructions
- **THEN** the guide explains the event restriction instead of treating a green manual run as permission to update a protected ref.
