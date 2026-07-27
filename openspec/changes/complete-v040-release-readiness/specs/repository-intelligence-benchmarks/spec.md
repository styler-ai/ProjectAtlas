## ADDED Requirements

### Requirement: Published MCP Composition Digests Match Their Named Input

The human-readable and machine-readable MCP composition evaluations SHALL name the SHA-256 of their exact raw input file. The release gate SHALL compare both MCP composition representations with the computed digest.

#### Scenario: MCP composition metadata is current
- **WHEN** release readiness validates the published MCP composition evaluation
- **THEN** the Markdown and JSON digest fields equal the SHA-256 of the named raw input

#### Scenario: One representation drifts
- **WHEN** either published digest differs from the raw input or from the other representation
- **THEN** release readiness fails before promotion
