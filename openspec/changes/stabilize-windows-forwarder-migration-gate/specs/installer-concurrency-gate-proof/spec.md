## ADDED Requirements

### Requirement: Installer concurrency proof follows causal process phases

The Windows opposite-forwarder migration E2E SHALL run real installer children and SHALL require both migrations to complete with intact managed pairs and project configurations within finite child-process observation bounds. It MUST NOT classify aggregate fixture wall time, including host scheduling or output collection, as a production lock timeout. Held-lock contenders SHALL prove their configured lock refusal and unchanged managed state independently of the outer process-observation envelope; child cleanup SHALL remain bounded and ownership-specific.

#### Scenario: Opposite migrations complete under suite load
- **WHEN** two installer children are released from opposite discovery gates while unrelated tests consume host capacity
- **THEN** the gate accepts both successful exits and intact managed pairs/configurations without a separate aggregate wall-time rejection

#### Scenario: Held lock refuses a contender
- **WHEN** the first or second forwarder lock is held and a contender reaches its configured bounded lock attempt
- **THEN** the contender reports the lock refusal, managed state remains unchanged, and release/retry succeeds, even if process startup and output collection exceed the product lock wait

#### Scenario: Child remains live or exits late
- **WHEN** an installer child exceeds its finite outer process-observation deadline or cannot be safely reaped
- **THEN** the gate fails with phase-specific diagnostics and exact-owned cleanup status rather than accepting an unverified migration
