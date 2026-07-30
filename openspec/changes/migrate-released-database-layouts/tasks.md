## 1. Compatibility Fixtures and Routing

- [x] 1.1 Map `migrate-released-database-layouts` to GitHub issue #379 in `openspec/issue-map.json` and keep the issue checklist synchronized with this file.
- [x] 1.2 Add the captured released v0.3.11-to-v0.3.26 schema-8 layout plus focused positive and incompatible-drift tests that reproduce the physical-order rejection before the production fix.

## 2. Semantic Admission and Atomic Migration

- [x] 2.1 Compare schema-8 predecessor columns and required index references by complete named semantic contract while retaining exact validation for current/later schemas and strict rejection of missing, extra, renamed, or changed objects.
- [x] 2.2 Prove successful migration preserves approved purposes, health resolutions, telemetry, project identity, root binding, and durable source rows through schema 16; invalidates predecessor publication trust; accepts a new complete generation; and passes integrity checks, reopen, and `root verify`.
- [x] 2.3 Inject a post-admission migration failure and prove rollback leaves the original schema, durable rows, publication state, root identity, and independent project databases unchanged.

## 3. Public Boundaries and Verification

- [x] 3.1 Cover read-only upgrade-required/no-write behavior and real CLI `init`/`scan` migration behavior for both released schema-8 layouts.
- [ ] 3.2 Run focused database/CLI tests, `cargo fmt --check`, workspace check/clippy/test/doc gates, OpenSpec validation, IssueOps, and live review-feedback reconciliation.
