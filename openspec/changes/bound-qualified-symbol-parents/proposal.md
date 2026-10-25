## Why

RC2 bounds every parser-provided symbol name and immediate parent, but graph projection later rebuilds an unbounded `A::B::...` containment identity and submits it to the 4,096-byte graph boundary. Deep valid nesting can therefore still abort whole-repository publication in any tree-sitter language.

## What Changes

- Bound graph-only qualified parent identities at the shared projection owner.
- Preserve valid nested symbols with deterministic collision-resistant qualification when the readable chain exceeds the graph identity ceiling.
- Reserve the core-owned compact-scope prefix from raw source names and immediate parents so exact source identities cannot collide with derived compact identities.
- Prove shallow compatibility, exact boundary behavior, deep language-neutral nesting, and full/incremental publication.
- Keep every other raw symbol admission rule, graph identity limit, and relation semantic unchanged.

## Capabilities

### New Capabilities

- `bounded-symbol-parent-qualification`: Project arbitrarily deep valid containment without producing an invalid graph identity or aborting publication.

### Modified Capabilities

None. The reserved prefix belongs to the new `bounded-symbol-parent-qualification` capability because it separates that capability's compact derived namespace from exact source identities; every other `bounded-symbol-identity-extraction` admission and omission rule remains unchanged.

## Impact

The change affects the graph identity byte-bound contract in `projectatlas-core`, shared source-symbol admission in `projectatlas-symbols` including Markdown symbol projection, derived qualified-parent construction in the CLI graph projection, and their unit/E2E publication tests. It adds one language-neutral reserved-prefix admission rule and no language-specific branch, dependency, schema, query, or platform behavior.

## Non-Goals

- Raising the graph identity byte ceiling.
- Truncating, hashing, or admitting invalid raw parser symbol names.
- Changing raw source-symbol admission outside the one reserved compact-scope namespace.
- Changing parser grammars, source spans, containment discovery, or public commands.
- Adding a generic identity framework.

This change is ready for implementation in `v0.4.5-rc3`.
