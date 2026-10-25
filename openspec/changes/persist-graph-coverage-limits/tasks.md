## 1. Closed Limit Domain

- [x] 1.1 Define one closed stable-spelling inventory for every `GraphLimitKind` and make serde/storage round-trip drift fail focused tests.
- [x] 1.2 Generate the current SQLite admission constraint from that inventory while retaining exact historical graph-schema shapes.

## 2. Schema and Persistence

- [x] 2.1 Append the schema-18 to schema-19 migration through the existing disposable graph rebuild, preserving project identity and authored state while invalidating derived publication.
- [x] 2.2 Prove fresh and migrated databases persist/read all nine limit kinds, reject an unknown kind, roll back failures, and reopen under the exact current contract.

## 3. Publication Compatibility

- [x] 3.1 Add a real non-language-specific partial Markdown graph-publication regression for a formerly rejected limit and prove last-complete/atomic scan behavior.
- [x] 3.2 Keep source and installed-candidate CI/release contracts exercising schema migration and graph publication on the affected supported platform matrix.

## 4. Release Gates

- [x] 4.1 Run focused core, database, CLI publication, migration, negative, rollback, and compatibility tests plus `cargo fmt --check`, warnings-denied Clippy, and diff checks.
- [x] 4.2 Run the complete workspace all-target/all-feature compile, test, doc, lint, OpenSpec, IssueOps, architecture-link, and release-candidate gates without relaxing existing limits.
- [x] 4.3 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.
