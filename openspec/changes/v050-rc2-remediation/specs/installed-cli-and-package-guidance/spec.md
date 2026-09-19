## ADDED Requirements

### Requirement: Packaged guidance names its release and ordinary CLI route
Each native release artifact SHALL provide setup and repair guidance for its containing version, distinguish the stable channel from an explicit prerelease route, show `atlas` and `projectatlas` usage, and use links resolvable from the unpacked artifact or a versioned online target.

#### Scenario: RC2 package setup is self-consistent
- **WHEN** a user unpacks the `v0.5.0-rc2` Windows release artifact
- **THEN** its README identifies RC2 setup and verification, labels stable guidance as stable, teaches ordinary `atlas` invocation, and contains no broken local documentation link

#### Scenario: Parent environment predates installation
- **WHEN** a host inherited PATH before installation
- **THEN** guidance explains that a newly launched environment-owning host is required to inherit the saved path without claiming to modify the running host

### Requirement: Windows atlas preserves native argument behavior
The Windows installer SHALL provide a collision-safe `atlas` route to the verified matching runtime that preserves the complete native argument vector and result behavior of `projectatlas`.

#### Scenario: JSON entrypoint route has parity
- **WHEN** PowerShell invokes `atlas` and `projectatlas` with equivalent JSON-bearing `--entrypoint` or `--trace-target` arguments
- **THEN** both routes parse equivalent values and preserve stdout, stderr, exit status, version guard, and selected runtime identity

#### Scenario: Windows argument edge cases are preserved
- **WHEN** `atlas` receives empty arguments, spaces, Unicode, or shell metacharacters
- **THEN** the runtime receives them as data without command execution or transformation by a batch forwarding layer
