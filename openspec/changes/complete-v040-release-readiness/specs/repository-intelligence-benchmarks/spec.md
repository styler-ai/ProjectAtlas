## ADDED Requirements

### Requirement: Published MCP Composition Digests Match Their Named Input

The human-readable and machine-readable MCP composition evaluations SHALL name the SHA-256 of their exact raw input file. The release gate SHALL compare both MCP composition representations with the computed digest.

#### Scenario: MCP composition metadata is current
- **WHEN** release readiness validates the published MCP composition evaluation
- **THEN** the Markdown and JSON digest fields equal the SHA-256 of the named raw input

#### Scenario: One representation drifts
- **WHEN** either published digest differs from the raw input or from the other representation
- **THEN** release readiness fails before promotion

### Requirement: Published Campaigns Match The Functional Release Candidate

The published v0.4 system-scale and final agent-navigation results SHALL measure a functional head containing every release-affecting runtime, packaged-skill, MCP inventory/schema, relation-service, and repository-graph behavior change. Their recorded source, runtime, skill, and tool identities SHALL match the measured artifacts. Later commits MAY change only benchmark locks, raw results, evaluations, landing copy, or finite release checklist state; any later product-behavior change SHALL invalidate the affected publication.

#### Scenario: The final functional candidate is measured
- **WHEN** release readiness validates the system-scale and agent-navigation publications
- **THEN** both campaigns identify and measure the current functional release candidate and its exact runtime, skill, and MCP surface

#### Scenario: Release behavior changes after measurement
- **WHEN** a later commit changes runtime, packaged-skill, MCP, relation-service, or repository-graph behavior
- **THEN** the affected campaign is relocked and rerun before promotion
