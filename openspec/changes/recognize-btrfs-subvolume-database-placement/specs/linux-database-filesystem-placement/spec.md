## ADDED Requirements

### Requirement: Linux database placement uses exact identity then canonical path containment

ProjectAtlas SHALL keep exact device-based filesystem resolution as the primary route. When Linux reports no mount for the probe device, ProjectAtlas SHALL use the dependency-provided mount inventory to select the unique longest component-boundary ancestor of the canonical probe path.

#### Scenario: Ordinary local filesystem uses exact resolution

- **WHEN** `stat.st_dev` matches the containing mountinfo device on a supported local filesystem
- **THEN** ProjectAtlas uses exact resolution without enumerating a fallback placement

#### Scenario: Btrfs subvolume device identifiers differ

- **WHEN** a canonical database probe is under a local Btrfs mount but its `stat.st_dev` differs from that mount's mountinfo device identifier
- **THEN** ProjectAtlas selects the containing Btrfs mount by longest component-boundary ancestry and admits the normal SQLite profile

#### Scenario: Similar path prefix is not an ancestor

- **WHEN** the probe is under a sibling component named `application` and an inventory entry ends at the adjacent `app` component
- **THEN** that entry is not eligible solely because its string bytes are a prefix

#### Scenario: Nested mounts select the most specific owner

- **WHEN** multiple inventory mount points are component ancestors of the canonical probe
- **THEN** ProjectAtlas selects the unique longest mount point

#### Scenario: Equally specific candidates disagree

- **WHEN** equally specific eligible inventory entries disagree on device or filesystem type
- **THEN** ProjectAtlas reports `database_filesystem_uncertain` and does not open SQLite

### Requirement: Path fallback does not weaken storage safety

Fallback-selected placement MUST pass the existing filesystem classification, canonical-path capture, and immediate pre-open revalidation before a database connection opens. Unknown, virtual, ephemeral, or network placement SHALL remain rejected or uncertain according to the existing typed contract.

#### Scenario: Network mount is selected by path

- **WHEN** the longest containing inventory mount has a known network filesystem type
- **THEN** ProjectAtlas reports `database_filesystem_unsupported` and does not create or open the database

#### Scenario: Unknown filesystem is selected by path

- **WHEN** the containing inventory mount has no supported local classification
- **THEN** ProjectAtlas reports `database_filesystem_uncertain` and does not create or open the database

#### Scenario: Inventory cannot prove a containing mount

- **WHEN** exact resolution fails and mount inventory is unavailable or has no component ancestor
- **THEN** ProjectAtlas preserves the bounded root cause in `database_filesystem_uncertain` and does not open SQLite

#### Scenario: Placement changes before open

- **WHEN** the canonical probe, mount point, device, or filesystem type differs between preflight and immediate revalidation
- **THEN** ProjectAtlas fails closed before opening the database

#### Scenario: Missing database uses an existing parent

- **WHEN** the database file and intermediate directories do not yet exist on a supported Btrfs subvolume
- **THEN** ProjectAtlas resolves the nearest existing canonical parent without creating filesystem state during inspection

### Requirement: All database-backed adapters share the corrected placement gate

CLI and MCP operations SHALL use the same corrected database preflight and MUST NOT create an index implicitly when the requested project root has no initialized ProjectAtlas database.

#### Scenario: CLI initializes on Btrfs

- **WHEN** `projectatlas init` targets a supported local Btrfs subvolume with differing device identifiers
- **THEN** initialization creates the project-local database with the mandatory WAL profile

#### Scenario: MCP addresses an initialized Btrfs project

- **WHEN** an MCP call explicitly addresses an initialized Btrfs project root through `project_path`
- **THEN** it opens that root through the same verified placement gate and does not route through another repository

#### Scenario: MCP addresses a missing index

- **WHEN** an MCP call explicitly addresses a root without `.projectatlas/projectatlas.db`
- **THEN** it returns the existing missing-index or initialization guidance without implicitly creating a database
