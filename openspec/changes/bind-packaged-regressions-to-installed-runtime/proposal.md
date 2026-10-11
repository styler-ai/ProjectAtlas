## Why

The RC3 Windows diagnostic passes its installed public-surface and published-predecessor upgrade contracts, then the standalone document-navigation regression panics because Cargo's binary environment variable is absent. Release-selected regression proof must exercise the explicitly installed candidate outside Cargo.

## What Changes

- Replace the two Cargo-only binary lookups in `bounded_pdf_and_docx_reach_cli_and_mcp_navigation` with the existing installed-runtime selector.
- Preserve every CLI/MCP, document, publication, failure, and resource assertion; retain the selector's ordinary Cargo fallback for source tests.
- Verify the complete release-selected regression set has no other unguarded Cargo-only lookup, and prove the corrected standalone runner against the checksum-verified package with Cargo's binary variable absent.
- Land the reviewed owning fix before #492 freezes a fresh candidate and restarts complete platform acceptance.

## Capabilities

### New Capabilities

- `packaged-regression-execution`: explicit installed-runtime selection by standalone release regression runners, with unchanged behavioral assertions and ordinary source-test compatibility.

### Modified Capabilities

None. This clarifies the executable-admission boundary supporting the existing installed-release acceptance contract; no product behavior or acceptance requirement is relaxed.

## Impact

#668 owns the two calls in `crates/projectatlas-cli/tests/e2e_navigation.rs`, affected frozen-source metadata if required, and causal standalone proof. Existing helper, workflow dispatch, production runtime, database schema, dependencies, and architecture remain unchanged. Planning is candidate-local until published; implementation begins only after exact-main specification admission and RC3 graph reconciliation.

## Non-Goals

No new executable selector or harness, Cargo-environment workaround, product feature, database migration, dependency update, relaxed assertion, release publication, stable promotion, or unrelated state mutation.
