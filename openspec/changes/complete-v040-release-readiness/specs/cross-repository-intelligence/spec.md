## ADDED Requirements

### Requirement: Federated Rendezvous Respects The Anchored Traversal

Federated detailed-relation and analysis responses SHALL derive the eligible typed external identities from the primary project's already bounded anchored traversal. Secondary roots SHALL contribute rendezvous evidence only for those identities, under the existing relation, confidence, resolution, row, edge, intermediate-byte, time, cancellation, and output limits.

#### Scenario: An anchored outbound identity is shared
- **WHEN** the primary outbound traversal reaches an exact external identity and at least one secondary root contains the same typed identity
- **THEN** the federated response retains the project-qualified evidence from the participating roots

#### Scenario: An unrelated relation shares the requested family
- **WHEN** two or more roots contain another external relation of the requested family that the primary anchored traversal did not reach
- **THEN** that identity and its evidence do not appear in detailed or analysis rendezvous output

#### Scenario: Inbound traversal reaches no external identity
- **WHEN** the requested inbound traversal contains no exact external identity
- **THEN** rendezvous output is empty and no secondary family scan is needed
