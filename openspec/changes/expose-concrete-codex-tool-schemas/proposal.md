## Why

ProjectAtlas v0.4.0 emits valid typed MCP input schemas, but its local `$defs` references are not reliably retained by the supported Codex tool-schema bridge. Agents can therefore see nested inputs such as `atlas_purpose_review.items` as unknown even though runtime deserialization remains strict.

## What Changes

- Publish self-contained MCP input schemas for every current ProjectAtlas tool whose nested request type otherwise produces a local `$ref`.
- Keep `atlas_purpose_review.items` concrete, including required `path` and the optional purpose and conditional-write fields.
- Add raw-stdio and bridge-facing regression coverage for reference-free schemas, required-field rejectability, and unchanged optional fields.
- Preserve typed Rust request models and existing CLI, raw MCP, runtime validation, and conditional-write behavior.
- Record that Codex still owns general support for valid local JSON Schema references; ProjectAtlas supplies a narrow compatibility representation for its current affected tools.

This change is ready for implementation.

## Capabilities

### New Capabilities

- `codex-tool-schema-compatibility`: MCP tool inputs remain concrete and agent-readable through the supported Codex plugin bridge.

### Modified Capabilities

None.

## Impact

The MCP adapter schema annotations and packaged MCP contract tests are affected. No database schema, storage behavior, CLI contract, dependency, or tool inventory changes are required.

## Non-Goals

- Replacing typed request structs with untyped JSON values.
- Building a generic JSON Schema rewriting framework.
- Redesigning or rationalizing the broader MCP tool surface tracked by #310.
- Weakening runtime validation in reliance on host-side validation.
