## ADDED Requirements

### Requirement: Cache scope is exact and independently invalidated
The optional parser-pack workflow SHALL reuse only Cargo target state left after all ProjectAtlas-owned package outputs have been removed. The cache key SHALL include the target ABI, pinned Rust identity, native compiler and SDK identity, Cargo lockfile and workspace manifest state, and an explicit cache-policy ABI version that changes when reusable artifact compatibility changes. The workflow SHALL use exact keys without broad fallback prefixes and SHALL NOT invalidate dependency state for unrelated workflow or diagnostic edits.

#### Scenario: Every cache input is unchanged
- **WHEN** a trusted run uses the same target, toolchains, lockfile, manifests, and cache-policy ABI as a previously saved run
- **THEN** the workflow restores that target's dependency build layer and reports an exact cache hit

#### Scenario: One exact compatible v1 layer exists
- **WHEN** the v2 key misses but the same target, toolchains, lockfile, and manifests have an exact sanitized v1 layer under the frozen compatible policy digest
- **THEN** the workflow restores and validates that exact layer, rebuilds all candidate-owned outputs, and may save the sanitized result under v2 after every trusted gate succeeds

#### Scenario: One cache input changes
- **WHEN** any declared target, toolchain, lockfile, manifest, feature, or cache-policy ABI input changes
- **THEN** the workflow reports a cache miss and constructs from an empty Cargo target directory

### Requirement: Candidate code is always rebuilt and reverified
The workflow SHALL remove artifacts for all seven ProjectAtlas-owned workspace crates before any restored target state is used and again before reusable state is saved. It SHALL freshly build the worker, assembler, verifier, and platform broker from the exact clean candidate and SHALL run the unchanged digest, native-import, license, containment, lifecycle, deterministic-assembly, package, and fresh-runner verification gates.

#### Scenario: Restored dependencies are accepted
- **WHEN** a valid dependency layer is restored
- **THEN** all ProjectAtlas-owned packages are cleaned before construction, the exact candidate is freshly built, and every existing verification gate still runs

#### Scenario: Reusable state is saved
- **WHEN** trusted construction and verification complete successfully
- **THEN** candidate-owned Cargo outputs and the platform broker are absent from the saved cache path

### Requirement: Restored state is untrusted and bounded
The contained construction SHALL validate a restored target root before use, SHALL reject path indirection and non-file entries, and SHALL enforce fixed entry-count and byte-size ceilings. Invalid restored state SHALL be quarantined without traversal and replaced by a clean target directory. Cache state SHALL never include release archives, receipts, secrets, credentials, ProjectAtlas databases, purposes, telemetry, or arbitrary workspace state.

#### Scenario: Restored target is valid
- **WHEN** the restored tree contains only bounded regular directories and files beneath the expected target root
- **THEN** construction may reuse its third-party dependency artifacts after candidate-package cleanup

#### Scenario: Restored target is corrupt or unsafe
- **WHEN** validation observes path indirection, an unexpected entry type, an exceeded bound, or unreadable state
- **THEN** the workflow quarantines that root, performs a clean build, and publishes no claim that the rejected layer was reused

### Requirement: Cache writes respect workflow trust boundaries
Pull-request and labeled proof runs SHALL be restore-only. Only explicitly dispatched repository construction jobs that reach final cache sanitation after their contained construction, platform verification, candidate revalidation, and artifact upload gates succeed SHALL be eligible to save dependency state. A failed cache-relevant gate SHALL save nothing.

#### Scenario: Pull request uses a cache
- **WHEN** a pull-request proof finds an exact cache key
- **THEN** it may restore and verify the dependency layer but cannot create or update cache state

#### Scenario: Trusted dispatch succeeds
- **WHEN** an explicitly dispatched construction job completes its cache-relevant gates successfully
- **THEN** it may save the sanitized dependency layer under the exact key

### Requirement: Clean release proof remains explicit
The workflow SHALL expose an explicit clean-construction dispatch mode that bypasses both cache restore and cache save. Final v0.4.0 release acceptance SHALL record one successful clean construction on Linux and Windows.

#### Scenario: Clean construction is requested
- **WHEN** a trusted operator dispatches the workflow with clean construction enabled
- **THEN** both targets start with empty Cargo target directories and no cache state is restored or saved

### Requirement: Reuse benefit and disposition are measurable
The workflow SHALL record target-specific cold and repeated-run wall times, cache disposition, and a non-secret digest of the selected cache key. An unchanged-input repeated construction SHALL reduce contained construction wall time by at least 60 percent on both accepted targets, or reuse SHALL remain disabled for the target with the measured reason recorded.

#### Scenario: Repeated-run benchmark completes
- **WHEN** a cold run has populated eligible cache state and an unchanged-input run completes
- **THEN** the Linux and Windows receipts distinguish clean, miss, hit, rejected, and save-eligible states and report the measured wall-time comparison
