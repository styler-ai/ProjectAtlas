## Purpose

Keep stable promotion, release-candidate publication, and repairs bound to the correct acceptance owner without reopening completed release campaigns.

## ADDED Requirements

### Requirement: Stable publication selects its declared acceptance graph
For a stable tag, publication admission SHALL select the corresponding declared stable milestone graph when present. RC tags SHALL keep their existing milestone graph. Stable tags without a declared stable graph SHALL retain the historical milestone route.

#### Scenario: Stable promotion follows a completed RC campaign
- **WHEN** the RC owner is closed and a separate stable graph is declared
- **THEN** stable admission checks the stable owner and exact native membership without reopening the RC issue or milestone

#### Scenario: A historical stable release is repaired
- **WHEN** no independent stable graph is declared for that version
- **THEN** admission uses the existing historical graph and retains all existing repair checks

### Requirement: Invalid stable declarations fail closed
Publication admission SHALL reject a malformed, incomplete, unmapped, or inconsistent declared stable graph rather than falling back to an older RC graph or a looser milestone check.

#### Scenario: A declared stable owner has invalid blockers
- **WHEN** the declared stable graph violates owner, membership, dependency, or mapping rules
- **THEN** publication is refused without selecting a different graph or mutating release state

### Requirement: The release owner remains open through first publication
Admission SHALL allow the selected release owner to remain open while requiring all declared children to be closed and exact agreement between the graph and native milestone membership.

#### Scenario: A singleton stable owner is ready to publish
- **WHEN** the open stable owner is the sole declared member and native membership matches
- **THEN** the existing release gates may proceed while the owner remains open for published-state readback

### Requirement: Closed-owner repairs retain exact identity checks
A closed release owner SHALL admit only repair of an existing non-draft release whose tag, candidate revision, and stable/prerelease classification match exactly.

#### Scenario: The release is missing or belongs to another revision
- **WHEN** a closed-owner repair lacks an existing exact release or its tag resolves to another commit
- **THEN** publication admission refuses the operation and preserves existing release state
