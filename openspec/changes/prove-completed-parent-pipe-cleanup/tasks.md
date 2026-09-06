## 1. Contract and specification

- [x] 1.1 Reconcile the observed parent, descendant, and inherited-output lifecycle; select the smallest causal fixture correction and complete the issue, specification, and release dependency mapping.

## 2. Fixture and verification

- [x] 2.1 Correct the completed-parent fixture in the existing delivery-test owner, proving successful parent exit with a retained descendant pipe and prompt owned cleanup without changing operation or workflow deadlines.
- [x] 2.2 Preserve and verify successful status, late-observation classification, live-parent timeout, output draining, cleanup failure, and supported-platform behavior through the existing lifecycle and observer tests; update only required frozen source identities and pass required local and hosted gates.

## Verification

Use the existing bounded command runner or normal pre-push gate for long checks:

- `cargo test --locked -p projectatlas-cli --test e2e_delivery release_asset_server_lifecycle_is_causal_and_bounded -- --exact --nocapture`
- `cargo test --locked -p projectatlas-cli --test e2e_delivery e2e_process_observers_ -- --nocapture`
- `cargo fmt --check`
- `cargo check --workspace --all-targets --all-features`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-features` and `cargo test --doc --all-features`
- `cargo doc --workspace --no-deps --all-features` with `RUSTDOCFLAGS=-D warnings`
- `python .github/scripts/issue-checklists.py --repo styler-ai/ProjectAtlas --planned-issue 561`
- The affected hosted lifecycle and installer jobs selected by the current CI planner.
