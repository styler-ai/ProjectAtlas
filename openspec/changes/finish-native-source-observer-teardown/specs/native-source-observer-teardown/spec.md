## ADDED Requirements

### Requirement: Complete owned Windows watcher cancellation

A source-observation entry SHALL finish native Windows cancellation of its root and applicable external configuration-parent watch before its lifetime ends. Teardown SHALL preserve the existing owner and root binding and SHALL NOT mutate source or indexed data.

#### Scenario: MCP server teardown permits repository cleanup
- **WHEN** MCP overview calls establish observations in two server instances and those instances are dropped
- **THEN** their native directory watches finish cancellation before subsequent repository removal
- **AND** existing server identity and usage assertions remain valid.

#### Scenario: External configuration parent is watched
- **WHEN** an entry watches a configuration parent outside its repository root and is dropped
- **THEN** cancellation completes for both owned directory watches.

### Requirement: Partial startup retains cleanup ownership

An entry SHALL own watcher cleanup before registering paths and SHALL retain existing typed startup errors.

#### Scenario: External parent registration fails after root admission
- **WHEN** root registration succeeds but the external configuration-parent registration fails
- **THEN** startup returns its existing observer error after cleaning up the admitted root watch
- **AND** subsequent repository removal does not overlap that watch.

#### Scenario: Root registration fails
- **WHEN** the repository root cannot be watched
- **THEN** startup returns the existing observer error without leaving owned native watches.

### Requirement: Preserve observation compatibility

The repair SHALL preserve normal observation, per-project isolation, and non-Windows watcher behavior. It SHALL NOT initialize a missing index or select another root during cleanup.

#### Scenario: Unrelated root remains active
- **WHEN** one source-observation entry is dropped while another bound root remains observed
- **THEN** only the dropped entry's watches are removed and the other observation remains usable.

#### Scenario: Cleanup performs no implicit index mutation
- **WHEN** an entry is dropped with an absent or existing index
- **THEN** cleanup neither creates nor modifies that index and does not retarget any other project.
