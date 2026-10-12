## Context

See proposal.md for the observed failures. The earlier `exercise_released_upgrade_migration_repair` decoded column zero of its registration query as `String`; its accepted repair uses the SQLite integer identity in parameterized reads and updates on a disposable predecessor backup. The remaining native acquisition failure is the shared plugin assertion requiring newer files from the published predecessor.

## Goals / Non-Goals

Preserve native identity through the existing query and bindings so migration failure/rollback and repair/retry execute. Respect shipped predecessor assets while requiring every current native plugin asset. No schema, production behavior, installed-route inventory, or publication-policy change is required.

## Decisions

Use `i64`, the native SQLite integer representation already used by the database owner. Keep parameterized bindings and existing row/state assertions. SQL text casts would mask the mismatch and add an unnecessary second identity representation.

The root-transition owner selects the root's canonical `.projectatlas/projectatlas.db`; a global `--db` override does not authorize adopting an alternate database. Exercise the schema-19 state restored from the retained compatible backup at that canonical path inside the already isolated test project before candidate initialization. The earlier MCP adoption proof may already migrate the live fixture; restoring only this disposable state keeps both proof paths independent. Preserve the retained compatible backup and all snapshots; never change production admission or a user-owned database. The helper must inject, refuse, repair, and retry the same canonical fixture state.

Use SQLite's own journal-mode transition to finish the disposable fixture's existing WAL state before replacing it from the compatible snapshot, then restore the required WAL operating profile before fault injection. Keep connections scoped and closed around replacement. No manual sidecar deletion, silent lock bypass, or relaxed operating-profile guard is needed.

The injected relative retired root fails inside the migration transaction's native-identity backfill with `InvalidCanonicalProjectRoot`, wrapped by `DbError::ProjectRootIdentity`. Match that stable typed error and its relative-row/absolute-path diagnostic. The previous unavailable-identity/repair-guidance matcher cannot match this path. Keep exact injected snapshot, schema 19, integrity, absence of partial native columns, row preservation, repair, retry to schema 23, and authority checks; matching the correct diagnostic does not replace them.

The helper proves database migration and authority preservation. The outer installed contract owns navigation after candidate initialization and scan. Keep its authored-purpose and source-marker summary assertions there; an earlier duplicate summary can correctly refuse with `refresh_required: policy_drift` before that refresh. Do not weaken freshness admission to force the premature read.

Use the existing real-SQLite installed upgrade contract to exercise the same helper read; retain its unchanged-baseline failure and corrected-run result as causal proof. Reuse existing fixture and migration owners; add no generic registration abstraction or independent upgrade harness. Execute the full corrected Windows published-predecessor contract against the installed runtime and retain its refusal/repair/retry and state-preservation checks.

## Risks / Trade-offs

The shared native plugin assertion must compare only the manifest and skill shipped by v0.4.5. Move `skills/projectatlas/references/language-support.md`, `skills/projectatlas/references/short-cli.md`, and `hooks/agent-instructions.txt` to its existing current-candidate-version branch beside the readiness assets. Keep exact source/cache byte comparisons and mandatory reads for every current asset; do not make file existence optional. Exercise predecessor success and current missing/mismatch refusal, then execute the installed Windows predecessor contract with native Codex acquisition enabled. Prepublish-only mode does not exercise this boundary.

- A fixture checks a duplicate query rather than the helper: exercise the owning read or full helper with an integer registration key.
- A passing focused check hides later upgrade defects: retain real installed Windows proof and mandatory fresh four-platform acceptance under #492.
- Diagnostic failure could be mistaken for product data loss: describe the demonstrated proof defect and claim product behavior only from executed contracts.

## Migration Plan

No product data migration. Land the reviewed test repair through its normal issue PR, then freeze fresh main and restart affected release acceptance. Retain existing candidate artifacts and fixture evidence.

## Dependencies / Cross-Issue Impact

#666 has no open implementation prerequisite and is a direct child/blocker of #492 in v0.5.0-00. It owns the proof-harness defect; #492 owns fresh four-platform candidate acceptance, publication, and closes last. Historical #593 acceptance remains complete; stable promotion #602 stays separate.

## Open Questions

None.
