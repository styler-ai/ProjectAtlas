## ADDED Requirements

### Requirement: CI and release action uses remain immutably pinned

The repository SHALL use the reviewed immutable `taiki-e/install-action` commit for the existing cargo-deny installation step in both `.github/workflows/ci.yml` and `.github/workflows/release.yml`. The two steps MUST continue to install `cargo-deny@0.20.2` under their existing conditions and workflow structure.

#### Scenario: Both workflow uses resolve to the reviewed release commit

- **WHEN** the two named workflow files are inspected at the accepted head
- **THEN** each cargo-deny installation step uses commit `5bf6ce016fd2e72eefc647cbca1e4213f65955b8`, the upstream `v2.87.5` commit
- **AND** each step retains `cargo-deny@0.20.2`

#### Scenario: A mutable or stale action reference is detected

- **WHEN** either named workflow uses a tag, branch, old SHA, or a different commit for this step
- **THEN** the exact-source acceptance check fails
- **AND** the workflow remains unaccepted until both uses match the reviewed immutable commit

#### Scenario: Workflow behavior remains unchanged

- **WHEN** the accepted head is compared with the release-owner baseline
- **THEN** the workflow diff contains only the two action SHA value changes in the two named workflow files
- **AND** conditions, permissions, jobs, and tool version remain unchanged while the owning OpenSpec and issue-map metadata may be added
