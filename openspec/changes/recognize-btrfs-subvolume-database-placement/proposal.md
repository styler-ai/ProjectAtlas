## Why

On supported local Linux Btrfs subvolumes, `stat.st_dev` can differ from the containing mount's mountinfo device identifier. `whichdisk 0.5.0` then fails exact device lookup and ProjectAtlas reports `database_filesystem_uncertain`, preventing every database-backed command from initializing.

This is a confirmed `v0.4.4` startup failure on a supported local filesystem and is ready for implementation in `v0.4.5-rc2`.

## What Changes

- Retain exact device resolution as the primary filesystem-placement route.
- On the specific Linux resolution failure, select the longest component-boundary mount ancestor from the dependency-provided mount inventory.
- Apply the existing supported-local, unsupported-network, and uncertain filesystem classification unchanged to the fallback result.
- Preserve canonical-path capture and pre-open revalidation so a path or mount swap still fails closed.
- Add deterministic mismatch, component-boundary, network, ambiguity, and revalidation coverage plus a hosted Linux Btrfs smoke.

## Capabilities

### New Capabilities

- `linux-database-filesystem-placement`: Supported Linux local mounts remain provable by canonical path when kernel device identifiers do not map one-to-one, without weakening SQLite WAL placement safeguards.

### Modified Capabilities

None.

## Non-Goals

- Accepting an unknown, virtual, ephemeral, or network filesystem.
- Weakening the mandatory SQLite WAL and synchronous durability profile.
- Shipping a custom mountinfo parser or replacing `whichdisk`.

## Impact

- Linux filesystem resolution and revalidation in `projectatlas-db`, release CI Linux platform coverage, and database initialization diagnostics.
- No schema, database content, CLI/MCP payload, or new dependency.
