## Why

ProjectAtlas rejects otherwise valid `tsconfig.json` and `jsconfig.json` files when their UTF-8 text begins with the optional `EF BB BF` byte-order mark. Because the shared compiler-configuration loader runs during full and incremental graph publication, the decode failure blocks `init`, `scan`, `watch --once`, and MCP refresh.

## What Changes

- Accept one exact UTF-8 BOM prefix before compiler-configuration UTF-8 decoding and JSONC parsing.
- Preserve byte limits, source hashing/currentness, deadline and cancellation checks, and strict malformed or non-UTF-8 failures.
- Cover both compiler-configuration filenames through the shared loader and the narrow real CLI/MCP refresh boundary.

## Capabilities

### New Capabilities

- `compiler-config-utf8-decoding`: Defines equivalent compiler-configuration loading with or without an exact leading UTF-8 BOM.

### Modified Capabilities

None.

## Impact

The change affects only the existing compiler-configuration byte loader in `projectatlas-cli` and focused unit/E2E coverage. It adds no dependency, crate, schema, migration, public API, command, MCP tool, or persistence behavior.

## Non-Goals

- Accepting arbitrary leading bytes, malformed JSONC, or non-UTF-8 input.
- Rewriting user configuration files or changing alias-resolution semantics.
- Changing file-size accounting, source-currentness checks, refresh routing, or database publication.

This v0.4.2 bug fix is ready for implementation.
