## ADDED Requirements

### Requirement: Current tracked source rejects machine-specific paths
ProjectAtlas SHALL reject genuine machine-specific home and checkout paths in the current tracked UTF-8 source tree and SHALL keep diagnostics privacy-safe.

#### Scenario: Tracked source contains a private path
- **WHEN** tracked source contains a forbidden machine-specific path family
- **THEN** the repository lint fails with only repository-relative file, line, column, rule identity, and portable guidance
- **AND** the diagnostic does not echo the matched path

#### Scenario: Source contains a portable or test path
- **WHEN** source uses a portable placeholder, derived runtime path, explicit fixture marker, or owning test/fixture path
- **THEN** the repository lint accepts it

#### Scenario: Tracked data is not UTF-8 text
- **WHEN** a tracked file is not valid UTF-8 text
- **THEN** the path rule skips it

### Requirement: The current-tree lint precedes product builds
Every local or hosted ProjectAtlas product build, package, parser-pack construction, documentation deployment, and release SHALL run the same current-tree lint first.

#### Scenario: A build workflow compiles ProjectAtlas
- **WHEN** pre-push, CI, release, documentation, or optional-parser construction reaches a product build command
- **THEN** the current tracked source tree has already passed the repository lint
