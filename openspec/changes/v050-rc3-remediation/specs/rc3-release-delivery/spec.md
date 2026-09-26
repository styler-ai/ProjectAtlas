## ADDED Requirements

### Requirement: RC3 owns a reconciled release graph and exact installed acceptance
The `v0.5.0-00` release owner #492 SHALL be the sole direct native parent and direct blocker consumer for accepted RC3 work #620, #624, #625, and #627, implement no product fixes, and close last. An RC3 candidate MUST come from exact accepted main after every child task, review, and required gate passes.

#### Scenario: Planning and child implementation
- **WHEN** RC3 work is active
- **THEN** the milestone, OpenSpec task map, issue bodies, native sub-issues, blockers, and issue status remain synchronized, while incomplete tasks and review remain unchecked

#### Scenario: Installed candidate proof
- **WHEN** all child fixes are accepted and merged
- **THEN** the release owner exercises the exact package across Windows, Linux, macOS x64, and macOS arm64 with version-matched direct CLI, plugin, hook, generated config, MCP, database upgrade/rollback, document fixtures, fileless map output, and safe complete command/tool inventory

#### Scenario: Publication boundary
- **WHEN** RC3 has explicit publication authorization and exact candidate proof is complete
- **THEN** `v0.5.0-rc3` is a non-draft prerelease with Latest excluded, independent asset/checksum/install readback, and stable promotion still owned by #602
