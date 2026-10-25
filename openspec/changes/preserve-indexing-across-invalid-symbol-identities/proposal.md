## Why

A parser declaration whose semantic name cannot be extracted can currently fall back to the complete declaration text. A sufficiently large C# field initializer then exceeds the graph-identity contract and aborts the complete repository publication instead of preserving the usable file and symbol index.

This is a confirmed `v0.4.4` and `v0.4.5-rc1` availability regression and is ready for implementation in `v0.4.5-rc2`.

## What Changes

- Extract declaration names only from semantic declarator/name fields; never use the complete declaration statement as an identity.
- Enforce a bounded symbol-name admission rule at the shared symbol-extraction boundary without truncating distinct names into collisions.
- Preserve the rest of the file and repository publication when a declaration has no admissible name.
- Add parser and complete-scan regressions for the reported C# static-registry boundary and malformed declarations.

## Capabilities

### New Capabilities

- `bounded-symbol-identity-extraction`: Parser-owned symbol identities remain semantic and bounded, and an unadmittable declaration cannot abort repository publication.

### Modified Capabilities

None.

## Non-Goals

- Raising or removing the durable graph-identity limit.
- Hashing or truncating user-visible symbol names into ambiguous identities.
- Adding language-specific generated-file ignore policy.

## Impact

- Symbol extraction in `projectatlas-symbols`, repository graph projection in `projectatlas-cli`, and CLI scan regression coverage.
- No schema, database migration, CLI/MCP payload, or dependency change.
