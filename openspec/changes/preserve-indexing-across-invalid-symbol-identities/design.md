## Context

Tree-sitter declaration projection currently asks `node_name` for a semantic name and falls back to compacting the complete declaration node. C# `field_declaration` nodes can contain multi-kilobyte collection initializers, so that fallback turns source text into `CodeSymbol.name`. Repository graph projection later validates the value through `GraphIdentityText` and aborts the atomic scan above 4,096 bytes.

Signatures and relation context are already bounded during extraction, and all built-in parser output passes through `push_symbol_with_metadata`. The symbol crate therefore owns the earliest complete admission boundary.

## Goals / Non-Goals

**Goals:**

- Keep symbol names semantic, exact, and bounded before persistence or graph projection.
- Omit only an unadmittable declaration while retaining all other file and repository facts.
- Preserve atomic publication and existing graph-identity validation.
- Keep extraction linear in parsed declarations with bounded retained bytes.

**Non-Goals:**

- Changing graph keys, schema, identity limits, or public payloads.
- Inventing identities by truncation or hashing.
- Ignoring generated C# files as a class.

## Decisions

### Semantic extraction has no whole-declaration fallback

`push_tree_symbol` will emit only when `node_name` finds a declaration-specific name, a declarator, a named field, or an identifier child. The complete node text is a display/signature source, not identity material.

Alternative considered: special-case C# `field_declaration`. Rejected because the unsafe fallback is shared by every tree-sitter language and has no sound identity semantics.

### Shared symbol admission rejects overlong names without truncation

`push_symbol_with_metadata` will apply the existing symbol snippet bound to the compacted name and omit a row that exceeds it. This is the single extraction path used by tree-sitter and fallback providers, remains comfortably below the graph byte limit even for UTF-8, and avoids collision-prone truncation.

Alternative considered: increase `MAX_IDENTITY_BYTES`. Rejected because it only moves the failure threshold and permits unbounded parser source to enter stable keys.

### Repository projection keeps strict contracts

Graph projection will continue to reject invalid stored graph contracts. The fix prevents built-in extraction from creating invalid names instead of weakening the downstream data-integrity boundary.

## Risks / Trade-offs

- [A language grammar previously relied on the whole declaration fallback] -> run the full parser matrix and retain focused declaration-name fixtures for every built-in family.
- [A legitimate exceptionally long identifier is omitted] -> preserve the file and other symbols; do not manufacture a colliding identity. Revisit only with a measured language requirement.
- [A non-name field still expands unexpectedly] -> enforce the shared final admission bound independently of grammar-specific extraction.

## Migration Plan

No schema migration is required. Bump the semantic graph projection fingerprint if existing persisted invalid names can survive a normal source-freshness check, then rebuild affected graph facts during the ordinary RC2 scan. Rollback is the prior binary and a normal rescan; no user-authored data changes.

## Open Questions

None.
