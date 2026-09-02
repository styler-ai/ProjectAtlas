## Why

The mandatory IssueOps architecture check currently collapses a locked Mermaid parser timeout into an invalid-diagram result. Under ordinary concurrent validation, that can misreport a healthy diagram as defective and block delivery for the wrong reason.

## What Changes

- Classify locked Mermaid parser attempts as valid, invalid, timed out, or unavailable.
- Retry the same exact diagram once only after the first timeout, using the existing fixed per-attempt bound.
- Keep all existing architecture-link, heading, repository, and syntax checks fail-closed while reporting the actual failure class and target.
- Add focused causal tests at the existing IssueOps boundary.

## Capabilities

### New Capabilities

- `issueops-mermaid-validation`: Defines bounded, failure-class-preserving Mermaid validation for IssueOps architecture links.

### Modified Capabilities

None.

## Impact

- Affects `.github/scripts/issue-checklists.py`, `.github/mermaid-parser/validate.mjs`, and their existing tests.
- Keeps the repository-locked Node/Mermaid parser, fixed timeout, and current issue/architecture contract.
- Adds no dependency, daemon, pool, configurable retry policy, generic subprocess framework, Rust behavior, database behavior, or public ProjectAtlas runtime surface.
- Planned for implementation in `v0.5.0-00` through issue #544.

## Non-Goals

- Disabling Mermaid validation or treating timeout as success.
- Retrying invalid syntax or unavailable execution.
- Changing issue architecture content or GitHub heading rules.

This change is ready for implementation after its non-closing planning pull request is accepted on `main`.
