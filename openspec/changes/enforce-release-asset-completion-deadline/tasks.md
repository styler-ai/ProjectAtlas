## 1. Contract

- [x] 1.1 Reconcile every completion receive path and freeze the absolute-deadline contract, bounded correction, specification, and release dependency mapping.

## 2. Completion deadline and proof

- [ ] 2.1 Enforce the existing absolute deadline at the shared release-asset completion decision for all receive paths and bound idle waits without changing ownership or timeout values.
- [ ] 2.2 Prove expired and timely complete/incomplete decisions, preserve real lifecycle and observer compatibility, improve causal failure diagnostics, update required frozen source identities, and pass required local and hosted platform gates.

## Verification

Run the focused `cargo test --locked -p projectatlas-cli --test e2e_delivery release_asset_server_lifecycle_is_causal_and_bounded -- --exact --nocapture`, required locked workspace check/Clippy/test/docs and normal pre-push gates, source inventory checks, OpenSpec validation, IssueOps, and affected hosted lifecycle jobs.
