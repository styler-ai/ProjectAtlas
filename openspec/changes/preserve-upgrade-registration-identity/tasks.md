## 1. Contract and planning

- [x] 1.1 Specify the observed registration type and fixture-placement failures, native identity and canonical-root contract, unchanged migration proof, and synchronized issue/OpenSpec/release ownership.

## 2. Repair and causal proof

- [x] 2.1 Correct the existing migration-repair helper to read and bind the native integer registration identity and exercise its disposable schema-19 state at the selected test root's canonical database path before candidate initialization; prove both regressions through the existing upgrade contract while preserving failure injection, rollback, repair, retry, and authored-state assertions.
- [ ] 2.2 Pass affected local checks, real Windows installed-predecessor upgrade proof, and required hosted source gates; reconcile specification and architecture with the final fix and return fresh four-platform candidate acceptance to release owner #492.

Verification uses the smallest qualified causal Rust test with a nonzero count, `cargo fmt --all -- --check`, required affected checks selected by the repository pre-push hook, `openspec validate preserve-upgrade-registration-identity --strict`, planned/current-PR IssueOps gates, and the corrected `packaged_cli_upgrades_published_predecessor_without_losing_state` installed Windows contract. Full four-platform publication acceptance remains #492 work.
