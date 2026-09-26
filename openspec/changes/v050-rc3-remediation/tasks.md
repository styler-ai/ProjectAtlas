## 1. RC3 contract and release graph planning (#620)

- [x] 1.1 Freeze the three observed RC2 defects, their distinct owners, RC3 scope, privacy boundaries, supported host constraints, and stable-promotion separation in one OpenSpec proposal and design.
- [x] 1.2 Create and validate the RC3 capability specifications, issue-backed implementation tasks, and exact issue-map ownership slices.
- [x] 1.3 Reopen #492, make #620/#624/#625 direct native children and blockers, synchronize their milestone and issue task mirrors, and read back the release graph.

## 2. Plugin and runtime readiness (#620)

- [x] 2.1 Trace supported Codex marketplace, trusted hook, packaged installer, fresh-shell CLI resolution, generated config, and MCP registration paths; choose the smallest truthful readiness/repair route that the host actually supports.
- [ ] 2.2 Implement and exercise older-runtime/registry, version-matched repair, wrong-project/newer-schema refusal, disabled-hook, fresh-child, and host-restart cases without automatic project mutation or database replacement.
- [ ] 2.3 Run the owning installer/plugin/host checks and independent review; reconcile documentation and architecture with the supported lifecycle and resolve every material finding.

## 3. DOCX font-specific symbol continuity (#624)

- [ ] 3.1 Reproduce rendered and non-rendered `w:sym` in minimal valid DOCX packages; trace the shared extractor, text-index, symbol, coverage, and atomic-publication callers and confirm the narrowest truthful outcome.
- [ ] 3.2 Correct the DOCX extraction/admission path and, only if required, shared durable incomplete-coverage representation; retain exact surrounding text, malformed-package refusal, and last-valid database state.
- [ ] 3.3 Add direct CLI and MCP full/incremental scan, search/overview/graph, fault/cancellation, repair/retry, and Windows/Linux/macOS packaged regressions using the causal DOCX fixture.
- [ ] 3.4 Run `cargo test -p projectatlas-symbols`, the owning qualified CLI E2E, `cargo fmt --check`, affected workspace Clippy/check/tests, OpenSpec and IssueOps gates; obtain independent review and resolve findings.

## 4. PDF execution-fuel continuity (#625)

- [ ] 4.1 Reproduce a PDF that exhausts Wasmi execution fuel; distinguish the parser limit from text-index byte caps and trace text/symbol callers, document identity, coverage, and publication ownership.
- [ ] 4.2 Fix a demonstrated parser inefficiency if present; otherwise reuse the accepted document-local incomplete outcome with exact path/typed fuel and finite cancellation, time, memory, and output limits.
- [ ] 4.3 Add full/incremental CLI and MCP regression for the fuel fixture, malformed/oversized negative inputs, fault/rollback/retry, and supported Windows/Linux/macOS package behavior.
- [ ] 4.4 Run `cargo test -p projectatlas-symbols`, the owning qualified CLI E2E, `cargo fmt --check`, affected workspace Clippy/check/tests, OpenSpec and IssueOps gates; obtain independent review and resolve findings.
