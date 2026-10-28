## Context

Document target candidates originate from parser-admitted Markdown facts or hash-verified re-extraction. Existing parser admission rejects backslash selectors; do not invent a second unproved producer bypass. The reachable defect instead concerns native path admission before per-target ignore-policy inspection. Stored target source bytes are not needed to reproduce the ordering problem.

## Goals / Non-Goals

Preserve the selected source boundary, typed results, normal ignore selection and recovery. Trusted operator root/index/config/state choices and global/worktree excludes retain their existing capabilities; this is not a general filesystem jail. No new parser, API, dependency or schema.

## Decisions

Use one private admission helper in the existing document-resolution index before `RootScanPolicy::excludes_path`. Resolve/check existing path prefixes against the captured canonical root, stopping before policy reads when a prefix escapes or is an unsupported link. Check all prefixes so an escape followed by a link back inside cannot authorize outside ignore reads. Native canonicalization/metadata needed for admission is distinct from reading source or ignore-file contents.

Keep in-root links unsupported where the existing contract says so, preserve ordinary missing/ignored/folder/case outcomes and allow operator-configured global/shared Git excludes. Reuse per-target cache and existing cooperative operation/publication controls. No per-query source parse, DB reset or unbounded retained path state.

Derivation invalidation belongs to the current publication fingerprint. Repair unchanged prior source through ordinary refresh, preserving identity and approved purposes and the last valid publication on failure. When combined with #731, retain a counter newer than both independently exercised contracts; the semantic import contract need not change for document-only admission.

The accepted combined resolver already implements this admission boundary, including the original cooperative control, cumulative native-prefix allowance and the shared uncached freshness path. Its derivation contract 26 and semantic contract 23 are newer than the independently exercised repair branches. Preserve that implementation and contract while completing the dedicated adapter, compatibility and native-platform proof; do not add a duplicate admission helper or advance the counter solely for test and acceptance metadata.

## Risks / Trade-offs

Rejecting every external filesystem path would break intentional operator configuration. Guard derived source targets before policy instead. A final-only canonical check can miss an escaping intermediate prefix; check each admitted prefix. Platform links include Unix links and supported Windows reparse/junction behavior; real platform proof must distinguish unavailable privileges from a passed link case. Capture and retain causal pre-fix evidence, normal positive workflows and reader/write compatibility.

## Migration Plan

No authored-state migration. One compatible derivation repair; current-contract no-ops remain unchanged. Preserve compatible rollback and coordinate exact-source #708/#715 proof after accepted integration.

## Open Questions

None.

## Dependencies / Cross-Issue Impact

#738 is an independent direct child and blocker of release owner #715. #731 owns the separate static-binding repair and #708 owns indexing performance. Combine their accepted contracts without losing either invalidation before #715 freezes and measures the exact RC1 package. The source boundary and trusted operator/state distinction are existing capabilities, not a new product-wide filesystem sandbox.
