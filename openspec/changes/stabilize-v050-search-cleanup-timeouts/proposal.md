## Why

RC3 has three confirmed defects: Unicode case-insensitive search silently misses matches, failed parser-profile cleanup can repeat during destruction and misclassify verified archives, and the indexing timeout schema describes a narrower budget than the runtime enforces. Correct these before the final planned v0.5.0-rc4 candidate.

## What Changes

- Fix shared literal/fuzzy matching and conservative persisted-text candidate selection (#697).
- Make temporary artifact cleanup attempt ownership explicit and report operational cleanup failures truthfully (#698).
- Align MCP schema, applicable help and guidance with the existing cooperative whole-indexing budget (#699).
- Reopen existing release owner #492 and milestone v0.5.0-00 for exact RC4 acceptance after the three fixes merge. Preserve completed RC1-RC3 tasks and keep stable promotion in #602.

## Capabilities

### New Capabilities

- `search-matching`: Unicode-aware literal/fuzzy matching with conservative bounded candidate selection.
- `parser-profile-cleanup`: explicit bounded cleanup ownership and operational failure reporting.
- `indexing-work-budget`: truthful caller-visible cooperative deadline semantics.

### Modified Capabilities

None. The repository currently has no relevant populated main-spec capability; these contracts extend its existing change specifications without changing architectural ownership.

## Impact

Storage and service search, CLI parser lifecycle, MCP descriptions and documentation, causal regressions, release metadata and packaging. No newly resolved package or database migration; search may directly use the already locked regex-syntax Unicode folding implementation. Each bug has a separate implementation PR and independent review. This plan becomes implementation-ready after IssueOps and OpenSpec validation; #492 remains open through RC4 publication.

## Non-Goals

No locale-specific collation or Unicode canonical normalization, new timeout abstraction, blanket timeout increase, containment bypass, unrelated dependency updates, npm delivery, stable publication or v0.6 feature work. Further independent non-blocking findings belong to v0.5.1.
