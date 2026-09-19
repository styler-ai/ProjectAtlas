## ADDED Requirements

### Requirement: Published parser-pack filenames are accepted
For a supported target, parser-pack verify and install SHALL accept the exact versioned archive filename published by the matching release while still requiring the expected target, manifest, digest, content, and containment contract.

#### Scenario: Published RC2 archive verifies without rename
- **WHEN** a user supplies the checksum-verified parser-pack asset named by the RC2 release
- **THEN** `atlas parser-pack verify` and `atlas parser-pack install` pass filename admission without a manual rename and continue through normal validation

#### Scenario: Invalid archive remains rejected
- **WHEN** an archive has the wrong platform, release identity, digest, or content
- **THEN** parser-pack admission refuses it before selection and does not relax containment

### Requirement: Windows containment failure is causal and recoverable
On supported Windows, parser-pack containment admission SHALL either complete from the final published artifact or return a causal bounded diagnosis for the failing admission phase. A failed admission SHALL select no partial slot and a subsequent retry SHALL remain safe.

#### Scenario: Containment admission succeeds
- **WHEN** the final target-matching Windows parser-pack asset is verified and installed on a supported host
- **THEN** admission completes under containment and the selected pack is usable by the optional parser lifecycle

#### Scenario: Admission failure retains safe state
- **WHEN** a broker, worker, protocol, resource, timeout, or cleanup failure occurs during containment admission
- **THEN** the error identifies the owning phase, no partial selection is active, containment remains enforced, and a retry can run safely
