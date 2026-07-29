## ADDED Requirements

### Requirement: Published MCP Composition Digests Match Their Named Input

The human-readable and machine-readable MCP composition evaluations SHALL name the SHA-256 of their exact raw input file. The release gate SHALL compare both MCP composition representations with the computed digest.

#### Scenario: MCP composition metadata is current
- **WHEN** release readiness validates the published MCP composition evaluation
- **THEN** the Markdown and JSON digest fields equal the SHA-256 of the named raw input

#### Scenario: One representation drifts
- **WHEN** either published digest differs from the raw input or from the other representation
- **THEN** release readiness fails before promotion

### Requirement: Published Campaigns Are Opt-In And Honestly Bound

The published v0.4 system-scale and final agent-navigation results SHALL measure only the behavior each campaign claims. Their recorded runtime, skill, tool, platform, environment, artifact, and closed measurement-harness input identities SHALL match the measured artifacts; harness inputs SHALL be content-digested and commit SHAs SHALL remain provenance only. Standard CI, pre-push, prepublication, merge, and release paths SHALL NOT execute either full campaign. A later change to an owning behavior-relevant input, measurement-harness digest, or measured identity SHALL make the prior publication historical or unavailable for the candidate without blocking release or triggering a rerun. A full campaign SHALL run only after an explicit user request; focused harness unit tests and published-artifact integrity checks remain required.

#### Scenario: The release benchmark behavior is measured
- **WHEN** release readiness validates the system-scale and agent-navigation publications
- **THEN** both campaigns identify their measured runtime, skill, MCP surface, platform, environment, behavior-relevant inputs, and closed measurement-owner file set by content digest

#### Scenario: Release behavior changes after measurement
- **WHEN** a later commit changes behavior or an artifact identity owned by a campaign
- **THEN** the prior publication is labeled historical or unavailable for the candidate
- **AND** release proceeds without a replacement campaign unless the user explicitly requests one

#### Scenario: Standard validation or release runs
- **WHEN** pre-push, standard CI, prepublication, merge, or release validation executes
- **THEN** a routing-policy check rejects any full system-scale or agent-navigation campaign invocation
- **AND** focused harness unit tests and published-artifact integrity checks may still run

#### Scenario: Unrelated metadata or behavior changes
- **WHEN** a later commit changes no input or artifact identity owned by a campaign
- **THEN** the passed publication remains valid without a rerun

#### Scenario: The user explicitly requests a new campaign
- **WHEN** the user explicitly requests system-scale or agent-navigation measurement
- **THEN** the campaign follows its preregistered schedule and retains every scheduled, failed, or completed row
