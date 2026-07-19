## ADDED Requirements

### Requirement: Optional Semantic Capability Has An Explicit Lifecycle

Semantic retrieval SHALL remain optional and project-local. Its typed lifecycle SHALL cover absent, installed-disabled, building, ready, stale, updating, rollback-ready, incompatible, failed, removing, and removed behavior with explicit allowed operations and generation identity. Normal scan and lexical/graph queries SHALL not require or implicitly download a model/runtime.

#### Scenario: Semantic mode is requested while unavailable
- **WHEN** no compatible ready generation exists
- **THEN** ProjectAtlas returns a typed capability/state error and does not silently relabel lexical results

#### Scenario: Semantic update fails
- **WHEN** model verification, vector build, cancellation, or activation fails
- **THEN** structural/lexical data remains valid and any rollback generation remains explicit

### Requirement: Default Core Is Independent From Optional Runtime Cost

An absent parser/semantic pack SHALL contribute no runtime initialization, network call, model/ANN/WASM linkage, or pack process to default-core operation. Optional dependencies and artifacts SHALL remain at the outer supervised boundary and receive their own size, startup, memory, security, and platform decisions.

#### Scenario: Default core is packaged without packs
- **WHEN** dependency, binary-link, and packaged-file inventories are inspected
- **THEN** optional parser/model/ANN/WASM runtimes are absent while built-in scan/navigation/search/graph behavior works

### Requirement: Installed Packs Are Supply-Chain Bound And Contained

Every installed pack SHALL bind pinned provenance, digest, license, ABI/runtime compatibility, and accepted capability rows. Normal use SHALL be offline and SHALL run through a supervised out-of-process boundary or a capability-denied WASM/native boundary with no repository-code execution, shell/build/compiler execution, or network capability. Hard time, progress-aware no-progress, process-tree memory, output, and cancellation limits SHALL apply. Pack crash, timeout, invalid output, or containment failure SHALL leave the MCP process responsive and SHALL NOT publish, invalidate, or damage the active structural generation.

#### Scenario: Pack violates a resource or capability boundary
- **WHEN** a pack crashes, hangs, exceeds a limit, requests a forbidden capability, or returns invalid output
- **THEN** the pack operation fails with bounded diagnostics while built-in navigation and the active structural generation remain available

### Requirement: Semantic And Hybrid Retrieval Are Evaluated And Explainable

A selected optional semantic pack SHALL bind vectors to one structural generation plus model/tokenizer/preprocessing identity, support bounded changed-row rebuild and cancellation, and expose deterministic versioned semantic/hybrid score reasons. Hybrid results SHALL preserve lexical completeness. Semantic or hybrid capability SHALL not be advertised until labeled retrieval quality, determinism, update cost, latency, process-tree memory, package size, licensing, and required-platform checks pass.

#### Scenario: Vector update or model evaluation fails
- **WHEN** a vector build, changed-row update, quality gate, timeout, or resource gate fails
- **THEN** the candidate generation is not advertised or activated and lexical/structural behavior remains unchanged

### Requirement: Derived Snapshots Are Integrity-First

Snapshot export SHALL use the SQLite backup API only to obtain a private consistent capture. The distributable snapshot SHALL then be constructed freshly from an explicit allowlist of derived tables and columns, include a content inventory plus bounded compression, schema/runtime/root/generation metadata, and a digest, and SHALL exclude project identity, reviewed purposes, health resolutions, telemetry, settings, future Memory Atlas state, secret-bearing raw values, nonportable machine-local paths, and deleted/free-page remnants. Import SHALL validate in a temporary path, enforce size/path/content-inventory/integrity/schema/root rules, preserve authored data and destination project identity, and publish derived data only through the normal atomic generation path.

Snapshot metadata SHALL also bind the source-state and capability/registry identity used to produce the derived generation. Import MAY enforce an explicit trust/signature policy for shared snapshots; local-only export/import SHALL NOT require signatures. When a configured policy requires a signature, an absent, unknown, invalid, or mismatched signature SHALL reject the temporary artifact before activation.

#### Scenario: Snapshot is torn or incompatible
- **WHEN** digest, expansion, integrity, schema, root, or generation checks fail
- **THEN** import is rejected before live activation and current project data remains unchanged

#### Scenario: Shared snapshot fails trust policy
- **WHEN** import requires a trusted signer and the temporary archive is unsigned, signed by an unknown key, or binds different source/capability identity
- **THEN** import fails before publication and the destination identity, authored data, and active generation remain unchanged

#### Scenario: Export source contains authored or private state
- **WHEN** a live project database also contains purposes, telemetry, health resolutions, machine-local values, or future Memory Atlas records
- **THEN** the distributable artifact is built from the derived allowlist and an archive/free-page inspection proves that excluded values were not copied merely because the private capture was consistent
