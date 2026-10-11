# Design

## Context

See proposal.md for the observed standalone-runner failure. The release workflow already supplies an explicit installed executable and plugin root. The navigation test contains two Cargo-only lookups; the existing `mcp_contract_executable()` selector supports both installed and ordinary Cargo execution.

## Goals / Non-Goals

Resolve both lookups through the existing selector, preserving all behavioral assertions and normal source-test compatibility. No new selector, environment workaround, production change, or relaxed release gate is needed.

## Decisions

Reuse `mcp_contract_executable()` at both call sites. Setting Cargo's environment variable in the workflow would conceal the test's incorrect executable ownership and leave standalone execution dependent on Cargo naming. A new helper would duplicate the existing boundary.

Update the navigation source hash in the existing frozen-contract owners after editing. Audit every release-selected regression caller, but change only demonstrated unguarded lookups. The audit found no other unguarded caller.

Prove the corrected test executable outside Cargo against the checksum-verified Windows package, with `CARGO_BIN_EXE_projectatlas` absent and the installed executable and plugin root explicitly supplied. Preserve the ordinary Cargo fallback and run required affected checks. Full platform acceptance belongs to the fresh merged candidate under #492.

## Risks / Trade-offs

- A second lookup could retain the defect: replace both calls and inspect the complete selected suite.
- A source build could accidentally substitute for the package: execute the compiled test runner directly with explicit package binding and no Cargo binary variable.
- Frozen source metadata could drift: refresh both existing navigation hash owners and run their causal checks.
- Old candidate results could be mistaken for acceptance: #492 restarts exact-candidate acceptance after this fix lands.

## Migration Plan

Publish this specification before implementation admission. Land the reviewed test-only fix, then freeze a fresh main candidate. No database or installation migration is introduced. Reverting the test change restores the demonstrated harness failure and cannot justify publication.

## Dependencies / Cross-Issue Impact

#668 owns executable selection and its causal proof. It has no implementation prerequisite beyond the current accepted main. #492 depends on the accepted fix before restarting complete release acceptance and closes last. Existing installed-release architecture remains accurate; no diagram change is needed.

## Open Questions

None.
