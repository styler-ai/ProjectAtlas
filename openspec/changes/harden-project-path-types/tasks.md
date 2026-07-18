## 1. Routing Types

- [x] 1.1 Introduce narrow typed wrappers or equivalent typed helper boundaries for selected roots, indexed roots, absolute filesystem paths, and repository-relative keys.
- [x] 1.2 Refactor MCP nearest-project routing helpers to use typed boundaries without changing public behavior.
- [x] 1.3 Keep explicit `project_path` isolation ahead of nearest-project routing.

## 2. Test Coverage

- [x] 2.1 Add property or table tests for repository-relative key validation and absolute-path normalization.
- [x] 2.2 Add filesystem-backed tests for selected-root absolute paths, indexed external roots, nested indexed roots, missing DBs, partial `.projectatlas/` folders, and DB/config root mismatches.
- [x] 2.3 Add Windows-specific and Unix-style cases where host behavior allows deterministic assertions.

## 3. Verification

- [x] 3.1 Run focused MCP routing tests.
- [x] 3.2 Run `cargo fmt --check`, `cargo check --workspace --all-targets --all-features`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, and `cargo test --workspace --all-features`.
