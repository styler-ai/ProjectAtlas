## 1. Contract and planning

- [x] 1.1 Specify the observed registration type, fixture-placement, and predecessor plugin-asset failures, native identity, canonical-root and release-specific asset contracts, unchanged migration proof, and synchronized issue/OpenSpec/release ownership.

## 2. Repair and causal proof

- [x] 2.1 Preserve the corrected native integer registration identity and canonical schema-19 fixture placement; restrict predecessor plugin comparisons to assets shipped by that release and retain all current-candidate asset comparisons in the existing version-specific branch; prove compatibility and fail-closed missing or mismatched current assets while preserving failure injection, rollback, repair, retry, and authored-state assertions.
- [ ] 2.2 Pass affected local checks, real Windows installed-predecessor upgrade proof with native Codex plugin acquisition enabled, and required hosted source gates; reconcile specification and architecture with the final fix and hand off fresh four-platform candidate acceptance to release owner #492 for execution after this fix merges.

Verification uses the smallest qualified causal Rust test with a nonzero count, `cargo fmt --all -- --check`, required affected checks selected by the repository pre-push hook, `openspec validate preserve-upgrade-registration-identity --strict`, planned/current-PR IssueOps gates, and the corrected `packaged_cli_upgrades_published_predecessor_without_losing_state` installed Windows contract. Full four-platform publication acceptance remains #492 work.
