## ADDED Requirements

### Requirement: RC2 reopens the complete v0.5.0 remediation graph
The existing v0.5.0 release owner #492 and milestone SHALL reopen for the confirmed RC1 remediation set. Issues #604 through #609 SHALL be its direct native children and direct blockers; each SHALL have an exact mapped OpenSpec task slice, one owning implementation PR, and completed acceptance before release acceptance resumes.

#### Scenario: Planned remediation is graph-consistent
- **WHEN** the RC2 planning state is checked before implementation handoff
- **THEN** the issue map, milestone, parent relationships, blockers, issue task mirrors, labels, and OpenSpec task ownership agree

### Requirement: RC2 is an independently verified prerelease
After all six remediation issues land, release acceptance SHALL build, install, and execute the exact current-main `v0.5.0-rc2` package and optional-parser assets on the supported platform matrix, read back the published identities and SHA256SUMS, and retain `v0.4.5` as GitHub Latest.

#### Scenario: Exact RC2 publication readback succeeds
- **WHEN** the RC2 workflow publishes the candidate from its accepted exact revision
- **THEN** the release is non-draft and prerelease, all expected assets/checksums/runtime/plugin/skill/hook identities match, installed CLI/MCP/host behavior passes, and Latest remains v0.4.5

#### Scenario: Remediation defect restarts acceptance
- **WHEN** a required RC2 installed-product or platform check finds a defect
- **THEN** it is returned to its owning issue and the final release acceptance does not close or publish a replacement candidate until the fix and affected proof complete
