## Context

RMCP derives each tool's JSON Schema from its typed `Parameters<T>`. Schemars emits reusable nested types under local `$defs` and points fields to them with `$ref`. ProjectAtlas v0.4.0 has three such inputs: the `atlas_purpose_review` item object, the `atlas_root_set` transition enum, and the `atlas_search` retrieval-mode enum.

Those schemas are standards-valid, but the supported Codex plugin/tool-schema bridge does not reliably retain local references. This is an adapter representation mismatch: Serde deserialization, MCP request routing, and the service/storage layers are already typed and correct.

## Goals / Non-Goals

**Goals:**

- Make every current ProjectAtlas MCP input schema self-contained at the field where a connected host needs its shape.
- Keep purpose-review `path` required and all five optional fields discoverable.
- Detect any future local input-schema reference through packaged raw-stdio contract coverage.
- Preserve runtime validation, project-root isolation, CLI behavior, tool inventory, and request types.

**Non-Goals:**

- A generic JSON Schema resolver or rewriter.
- Untyped `serde_json::Value` request models.
- Changes to database, service, or conditional-write semantics.
- General Codex support for local JSON Schema references or the broader #310 tool-surface redesign.

## Decisions

### Inline at the nested typed-schema boundary

Apply Schemars' type-level `#[schemars(inline)]` attribute to the three affected nested request types. Schemars owns schema generation and those types own the reusable schema representation consumed by the MCP parameter structs, so this is the narrowest responsibility-coherent boundary supported by the pinned Schemars release. The attribute does not alter Serde deserialization.

The simpler alternative of changing only `atlas_purpose_review` is insufficient because the released raw-schema audit found the same bridge-sensitive shape in `atlas_root_set` and `atlas_search`. A post-generation schema walker is rejected because it would duplicate standards-library behavior, add failure modes, and create a generic framework for three closed compile-time types.

### Gate the complete advertised input inventory

Packaged stdio coverage will inspect every `tools/list` input schema and fail if a local `$defs` or `$ref` remains. Focused assertions will also verify that `atlas_purpose_review.items.items` is an object, requires `path`, exposes the five optional fields without making them required, and contains types that allow host-side rejection of an empty item.

The test is bridge-facing to the locally observable boundary: it enforces the self-contained schema subset consumed by Codex without claiming to control or test Codex's upstream renderer inside ProjectAtlas CI.

### Preserve runtime admission as independent defense

Runtime deserialization and purpose-review admission remain unchanged. Existing raw MCP behavior continues to reject a missing `path`, stale or incomplete conditional-write identities, oversized requests, wrong roots, and missing indexes independently of host validation.

## Risks / Trade-offs

- [Codex later adds complete local-reference support] → Keep the standards-valid typed models and treat inline schemas as a compatible representation; removing the attributes later is optional, not required.
- [A future nested tool type reintroduces a local reference] → Audit all advertised input schemas in the packaged contract test rather than maintaining a hand-written affected-tool list.
- [Inlining increases `tools/list` output] → Only three small, single-use nested definitions are affected. Generation remains startup/metadata work with constant CPU and memory; tool-call serialization, database I/O, locks, WAL behavior, and persistent bytes are unchanged.
- [Structural tests overstate host integration] → Report Codex's general reference handling as an upstream dependency and describe the local regression as compatibility-shape evidence, not hosted Codex proof.

## Migration Plan

No data migration is required. Ship the schema-generation change with the next ProjectAtlas runtime/plugin build. Rollback is the ordinary code rollback; runtime request compatibility is unchanged in either direction.

## Open Questions

General local `$defs`/`$ref` support remains owned by the Codex plugin/tool-schema bridge. ProjectAtlas cannot close that upstream capability from this repository; it can only emit the smallest compatible self-contained shape for its own tools.
