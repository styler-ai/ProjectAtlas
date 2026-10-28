## Purpose

Keep existing document navigation within the selected source boundary while preserving intentional operator filesystem capabilities.

## ADDED Requirements

### Requirement: Derived targets are admitted before policy reads
Every document-target projection path SHALL admit the native target prefixes against its captured source root before evaluating per-target ignore policy. An escaping or unsupported link SHALL NOT authorize reading ignore-file or source contents from an unselected location. Parser and recovered facts SHALL retain their current selector-admission rules.

#### Scenario: A derived target uses an unsupported native prefix
- **WHEN** an admitted document reference traverses an escaping or unsupported source link
- **THEN** projection returns the existing bounded unresolved result before per-target policy contents are read, including when a later prefix would return inside the root

#### Scenario: Normal source and operator policy remain supported
- **WHEN** ordinary local, ignored, missing, folder or case-conflicting targets and explicitly configured global/shared-worktree excludes are evaluated
- **THEN** existing results and legitimate operator root/database/config/state capabilities remain available through CLI and MCP

### Requirement: Compatible projections repair atomically
The existing derivation contract SHALL invalidate prior document projections even when source bytes are unchanged. Ordinary refresh SHALL preserve identity, approved purposes and last-valid state, converge with a clean rebuild and avoid work/publication on a later unchanged refresh.

#### Scenario: Unchanged prior source and failed repair
- **WHEN** a compatible prior index is refreshed or repair is cancelled/fails before publication
- **THEN** successful repair publishes the admitted results atomically, failure preserves the old valid publication, and compatible rollback/re-upgrade preserves authored state

### Requirement: Shared and platform paths have causal proof
The owning boundary SHALL have pre-fix negative evidence and positive compatibility proof through actual installed CLI/MCP and supported native platforms. Unavailable link privilege SHALL be reported rather than counted as a passed platform case.

#### Scenario: Root admission and ordinary navigation
- **WHEN** the corrected runtime indexes and refreshes isolated fixtures on supported platforms
- **THEN** source navigation stays usable, unwanted policy reads stop, and the complete RC1 source and installed acceptance consume this exact correction
