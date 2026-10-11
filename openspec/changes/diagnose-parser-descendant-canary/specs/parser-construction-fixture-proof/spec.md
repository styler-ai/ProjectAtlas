## Purpose

Make parser construction fixture failures causally observable while proving admitted live-descendant cleanup within the existing containment boundary.

## ADDED Requirements

### Requirement: Construction fixtures distinguish failed readiness states

Construction canaries SHALL distinguish early child exit, missing or invalid readiness evidence, and readiness deadline expiry using bounded non-secret outcomes. They MUST preserve containment admission, finite observation bounds, and owned cleanup.

#### Scenario: Child exits before readiness
- **WHEN** a construction-canary child exits before producing valid readiness evidence
- **THEN** the failure reports the numeric child outcome separately from a readiness deadline
- **AND** cleanup retires only the owned process tree

#### Scenario: Readiness evidence is missing or invalid
- **WHEN** the child remains live but readiness evidence is missing or has invalid contents at the finite deadline
- **THEN** the canary reports that specific state and bounded elapsed time
- **AND** it fails without claiming containment or cleanup proof succeeded

#### Scenario: Created marker becomes valid before the deadline
- **WHEN** a live child creates an incomplete marker and publishes valid readiness contents before the finite deadline
- **THEN** marker existence alone does not end readiness observation
- **AND** the canary accepts only the valid contents from the still-live child within that deadline

### Requirement: Descendant cleanup proof begins with a live admitted child

The descendant canary SHALL establish a live child with valid containment evidence before asserting job-close cleanup. It MUST prove that closing the existing containment owner retires that child. Diagnostics or fixture repair MUST NOT bypass token checks, extend workflow deadlines, or accept retries as proof.

#### Scenario: Admitted descendant is retired
- **WHEN** valid readiness evidence is observed from the live contained child and its owning job closes
- **THEN** the child is retired within the existing cleanup bound
- **AND** clean construction remains fail-closed on missing readiness or failed retirement

#### Scenario: Marker remains after child exit
- **WHEN** readiness evidence exists but the corresponding child has already exited
- **THEN** the canary refuses to count that state as live-descendant cleanup proof

#### Scenario: Live descendant has not written its completion marker
- **WHEN** the exact descendant remains live after the existing job-close cleanup bound
- **THEN** the canary fails retirement proof even if the completion marker is absent
