## ADDED Requirements

### Requirement: Supported Optional-Parser Archives Ship Through A Clean Input-Bound Handoff

An explicit all-platform clean optional-parser run SHALL emit one bounded release handoff containing exactly the supported Linux and Windows archives, their clean construction receipts, and the aggregate proof. `02-Release` SHALL accept that handoff only from the same repository's successful `optional-parser-pack` workflow-dispatch run whose behavior-relevant inputs match the release candidate. It SHALL validate the supported target set, version, provenance revision, clean receipts, archive names, sizes, and SHA-256 digests before staging versioned release assets.

#### Scenario: A supported clean candidate is released
- **WHEN** the release inputs have a successful clean all-platform handoff
- **THEN** both supported optional-parser archives and the aggregate proof are included in the release assets and covered by the release checksum manifest

#### Scenario: The handoff is stale, partial, reused, or altered
- **WHEN** the run identity, behavior-relevant inputs, target set, proof, receipt, archive size, archive digest, or version does not match
- **THEN** prepublish and publish fail before any optional-parser asset is staged

#### Scenario: Automatic release follows a content-preserving promotion
- **WHEN** `main` receives the verified `dev` promotion through an identical-tree merge commit
- **THEN** `03-Auto-Release` supplies the newest successful unexpired handoff with matching behavior-relevant inputs to `02-Release`
