## 1. Contract

- [x] 1.1 Define the native source-observer teardown and partial-start cleanup contract with its release dependency mapping.

## 2. Implementation and proof

- [x] 2.1 Complete Windows source-observer unwatch operations before releasing the owning entry, including partial registration failure.
- [x] 2.2 Prove native directory cleanup after MCP server teardown and partial watcher startup, preserve observation behavior on supported platforms, and pass required local and hosted gates.

## Verification

Run focused source-observation and MCP identity tests with their actual nonzero counts, then the normal pre-push gates: cargo fmt --check; cargo check --workspace --all-targets --all-features; cargo clippy --workspace --all-targets --all-features -- -D warnings; cargo test --workspace --all-features; cargo test --doc --all-features; strict rustdoc. Run OpenSpec validation and scoped IssueOps before transitions. Required hosted proof follows affected-ci-proof.py.
