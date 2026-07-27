## ADDED Requirements

### Requirement: Published Evaluation Digests Match Their Named Inputs

Every human-readable and machine-readable benchmark evaluation that names a raw input SHA-256 SHALL match the bytes of that exact repository file. The release gate SHALL compare all published representations with the computed digest.

#### Scenario: Evaluation metadata is current
- **WHEN** release readiness validates a published benchmark evaluation
- **THEN** the Markdown and JSON digest fields equal the SHA-256 of the named raw input

#### Scenario: One representation drifts
- **WHEN** either published digest differs from the raw input or from the other representation
- **THEN** release readiness fails before promotion
