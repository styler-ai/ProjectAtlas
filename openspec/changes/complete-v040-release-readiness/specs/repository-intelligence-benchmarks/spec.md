## ADDED Requirements

### Requirement: Published MCP Composition Digests Match Their Named Input

The human-readable and machine-readable MCP composition evaluations SHALL name the SHA-256 of their exact raw input file. The release gate SHALL compare both MCP composition representations with the computed digest.

#### Scenario: MCP composition metadata is current
- **WHEN** release readiness validates the published MCP composition evaluation
- **THEN** the Markdown and JSON digest fields equal the SHA-256 of the named raw input

#### Scenario: One representation drifts
- **WHEN** either published digest differs from the raw input or from the other representation
- **THEN** release readiness fails before promotion

### Requirement: Published Campaigns Match Their Measured Behavior And Artifacts

The published v0.4 system-scale and final agent-navigation results SHALL measure the behavior each campaign claims. Their recorded runtime, skill, tool, platform, environment, artifact, and closed measurement-harness input identities SHALL match the measured artifacts; harness inputs SHALL be content-digested and commit SHAs SHALL remain provenance only. A later change SHALL invalidate only a campaign whose owning behavior-relevant input, measurement-harness digest, or measured artifact identity changed.

#### Scenario: The release benchmark behavior is measured
- **WHEN** release readiness validates the system-scale and agent-navigation publications
- **THEN** both campaigns identify their measured runtime, skill, MCP surface, platform, environment, behavior-relevant inputs, and closed measurement-owner file set by content digest

#### Scenario: Release behavior changes after measurement
- **WHEN** a later commit changes behavior or an artifact identity owned by a campaign
- **THEN** the affected campaign is relocked and rerun before promotion

#### Scenario: Unrelated metadata or behavior changes
- **WHEN** a later commit changes no input or artifact identity owned by a campaign
- **THEN** the passed publication remains valid without a rerun
