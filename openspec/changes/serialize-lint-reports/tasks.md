## 1. Shared Typed Report

- [x] 1.1 Return deterministic typed non-source and untracked-file facts from map lint while preserving current classifications and compatibility text.
- [x] 1.2 Return typed bounded SQLite purpose-health facts and compose one shared `LintReport` with `ok`, `exit_code`, map/index sections, and compatibility text.

## 2. Adapter Integration

- [x] 2.1 Route CLI lint through the existing JSON/TOON stdout serializer and flush before preserving clean/failing exit codes; keep stderr for execution diagnostics.
- [x] 2.2 Make `atlas_lint` use the same report owner and preserve explicit-root, wrong-root, missing-index, transport, and no-implicit-mutation behavior.

## 3. Contract Proof

- [x] 3.1 Add focused clean/failing CLI subprocess tests for parseable JSON, TOON, distinct formats, stdout/stderr ownership, exit-code preservation, and rejected stdout writes.
- [x] 3.2 Add CLI/MCP parity tests covering typed map/index facts, explicit project routing, missing index, wrong root, and no mutation.
- [x] 3.3 Wire one exact installed-candidate lint-format contract into mandatory Windows, Linux, and macOS release verification and its static workflow gate.

## 4. Release Gates

- [x] 4.1 Run focused CLI/MCP unit and E2E tests plus `cargo fmt --check`, warnings-denied Clippy, and diff checks.
- [x] 4.2 Run the complete workspace all-target/all-feature compile, test, doc, lint, OpenSpec, IssueOps, architecture-link, and release-candidate gates.
- [x] 4.3 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.
