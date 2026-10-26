## Context

The Tree-sitter import fact already retains the parenthesized statement. Python parse_import currently rejects all parentheses, and resolution_keys explicitly calls a valid list unsupported. A minimal isolated example was reproduced on installed stable 0.5.0.

## Goals / Non-Goals

Restore equivalent static named-import semantics and downstream file/call resolution, including indexes made by the prior semantic contract. Keep the existing parser, graph publisher, root isolation and conservative ambiguity behavior. Do not execute Python or redesign resolution.

## Decisions

Use borrowed string slices in the existing concrete Python module: trim one balanced outer pair, reject remaining nested delimiters, and reuse the flat comma/alias normalization with Unicode XID identifier validation (underscore permitted at the start) and the existing hard-keyword guard. Reuse the already-locked unicode-ident crate directly rather than reimplementing Unicode tables or compiling/caching a regex. The locked crate uses Unicode 17.0 XID tables; exclude U+200C/U+200D join controls for the Python profile. Preserve source spelling; no target-interpreter Unicode-version selection or NFKC name-equivalence rewriting belongs to this correction. Preserve ordinary imports. Comments inside parenthesized lists remain unsupported: extraction has already collapsed line boundaries while retaining comment text, so the semantic owner must safely abstain rather than infer comment endings. Add a negative regression and do not claim all valid Python import syntax. Processing is linear in the bounded extracted statement plus existing output references; no additional source traversal, database query or global state. Merely deleting the parenthesis guard would admit malformed lists; introducing a general parser or trait layer duplicates the existing grammar owner.

Advance SEMANTIC_RESOLUTION_CONTRACT_VERSION through its existing digest/fingerprint invalidation route. Reconcile cached raw facts with the new semantics; do not reset databases or suppress invalidation to preserve a benchmark. Verify project identity and approved purposes survive and a second unchanged refresh does not publish again.

## Risks / Trade-offs

Malformed, nested, empty or aliased forms could fabricate references: cover positive and negative normalization. Unit-only success could leave persisted edges unresolved: exercise actual CLI imports and imported-function callers. Existing unchanged indexes could retain stale edges: prove prior-contract invalidation and a real installed 0.5.0 upgrade. Global semantic versioning can require one-time reconciliation for non-Python indexes; measure/report that upgrade separately from current-contract warm opens rather than disguising it as a zero-work open.

## Migration Plan

Ordinary refresh detects the changed semantic fingerprint and regenerates derived relations using the existing publication owner. Schema, identity and authored purposes remain intact. Rollback uses the same semantic mismatch handling under the previous runtime; no authored data migration.

## Architecture Disposition

N/A: syntax normalization changes inside the existing Python semantic provider and its already-versioned projection contract. Extraction, service resolution, database schema, publication ownership and CLI/MCP routes remain unchanged.

## Dependencies / Cross-Issue Impact

#722 has no implementation blockers. It is a direct child of #715 and blocks integrated RC publication. Coordinate the semantic-upgrade performance case with #708; preserve #721 bounded relation defaults when integrating. No dependency on a sibling's unmerged implementation.

## Open Questions

None.

## Verification

Positive and negative normalization tests, including ordinary and parenthesized combining-mark names/aliases with ASCII siblings and invalid starts/punctuation; real CLI resolved file and imported caller queries; ordinary/relative imports and alias compatibility; incremental/full convergence; cached prior-contract and installed 0.5.0 refresh preserving identity/purposes. Run cargo fmt --check, locked workspace check/Clippy/tests/doctests and warning-clean docs, strict OpenSpec, scoped IssueOps and normal affected native/hosted gates. Independent read-only review reconciles all five acceptance outcomes.
