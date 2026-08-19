## Context

Every writable or read-only ProjectAtlas database open first captures a content-free `DatabaseLocation` and revalidates it immediately before opening SQLite. On Linux, `whichdisk 0.5.0` resolves a path by exact equality between `stat.st_dev` and mountinfo `major:minor`. Btrfs subvolumes can expose different identifiers for the same containing local mount, so exact lookup returns `NotFound` before ProjectAtlas can classify the filesystem.

The dependency's enabled `list` feature already returns decoded mount points, devices, and filesystem types from the same process mount namespace. Reusing that inventory avoids a second mountinfo parser and keeps classification in the existing SQLite safety boundary.

## Goals / Non-Goals

**Goals:**

- Prove supported local Linux placement when exact device resolution fails but canonical path containment is unambiguous.
- Preserve existing network, unknown-filesystem, WAL, canonical-path, and time-of-check/time-of-use safeguards.
- Keep lookup linear in the bounded host mount inventory and allocate only owned selected-location fields.

**Non-Goals:**

- Guessing when mount inventory is missing or ambiguous.
- Accepting virtual, ephemeral, unknown, or network storage.
- Adding a ProjectAtlas mount parser or a new dependency.

## Decisions

### Exact resolution remains primary

`whichdisk::resolve` remains the fast path on every platform. The fallback is Linux-only and is attempted only when exact resolution fails because no device mount is found; permission, canonicalization, and inventory errors remain typed uncertainty.

Alternative considered: always use path ancestry. Rejected because exact device identity is stronger and already correct for ordinary mounts.

### Fallback selects the longest canonical component ancestor

The probe is canonicalized, `whichdisk::list` supplies mount candidates, and the fallback selects the unique candidate whose mount path is the longest `Path::starts_with` ancestor. Component-aware path comparison prevents string-prefix confusion between sibling components named `app` and `application`.

Alternative considered: invoke `findmnt`. Rejected because an external executable is not guaranteed in packaged or container hosts and the installed dependency already owns mount inventory decoding.

### Existing classification and revalidation remain authoritative

The fallback produces the same `DatabaseLocation` fields as exact resolution. Existing supported-local classification runs after selection, and pre-open revalidation resolves the location again and compares canonical probe, mount point, device, and filesystem type. Network or unknown types still fail before SQLite opens.

## Risks / Trade-offs

- [Mount inventory contains overlapping bind/subvolume mounts] -> choose the longest component ancestor and fail if equally specific candidates disagree.
- [Path or mount changes after preflight] -> keep immediate revalidation against all captured location fields.
- [Dependency inventory omits the containing mount] -> return `database_filesystem_uncertain`; never infer locality.
- [Large container mount table] -> one linear scan occurs only after the exact fast path fails; no repeated parser or filesystem walk is introduced.

## Migration Plan

No schema or stored-data migration is required. Ship the resolver change in all RC2 Linux packages, exercise a real Btrfs-host initialization smoke, and retain the prior fail-closed behavior as rollback.

## Open Questions

None.
