## ADDED Requirements

### Requirement: RC2 remediation has exact child task ownership
The reopened v0.5.0 release owner SHALL map #604 through #609 to non-overlapping OpenSpec task slices that mirror their live issue implementation tasks. A release-owner planning PR SHALL introduce only task owners that are direct children in its declared release graph.

#### Scenario: Release graph maps all RC2 children
- **WHEN** the RC2 planning boundary is checked
- **THEN** #604 through #609 have exact local task ownership and no unrelated issue gains authority

### Requirement: RC2 repairs the six audited installed-product boundaries
RC2 SHALL deliver bounded lifecycle guidance (#604), real-indexed classified discovery (#605), version-matched package guidance (#606), versioned parser-pack filename admission (#607), argument-preserving Windows atlas invocation (#608), and causal recoverable Windows parser-pack containment admission (#609).

#### Scenario: Lifecycle guidance is independent of host PATH
- **WHEN** a trusted Codex SessionStart hook emits ProjectAtlas guidance while PATH contains a shadow `projectatlas` executable
- **THEN** the hook reads its bounded guidance asset through the installed plugin root and does not execute the shadow or any ProjectAtlas runtime

#### Scenario: All owning fixes are accepted
- **WHEN** each mapped remediation issue is completed
- **THEN** its focused regression and installed-product proof pass without relaxing containment, trust, or stable-release policy

### Requirement: RC2 remains a prerelease
The release owner SHALL publish v0.5.0-rc2 only after every child is accepted and exact-package acceptance passes. The release SHALL be non-draft and prerelease, while v0.4.5 remains GitHub Latest.

#### Scenario: Release readback succeeds
- **WHEN** the RC2 package is published
- **THEN** its tag, assets, checksums, installed behavior, prerelease metadata, and Latest release state match the declared contract
