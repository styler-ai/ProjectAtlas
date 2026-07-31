## 1. SQL Fixture Portability

- [x] 1.1 Map `pin-sql-fixture-line-endings` to its GitHub issue and synchronize the issue checklist.
- [x] 1.2 Pin repository SQL checkouts to LF without changing captured schema DDL or production migration behavior.
- [x] 1.3 On Windows, run `git ls-files --eol -- crates/projectatlas-db/tests/fixtures/*.sql`, `cargo test --locked -p projectatlas-db schema::tests::evolved_released_schema_drift_is_refused_without_mutation -- --exact`, and `cargo test --locked -p projectatlas-db --all-features`, each with a 20-minute process timeout, to verify LF checkout state, the evolved released-schema drift test, and the complete database suite.
