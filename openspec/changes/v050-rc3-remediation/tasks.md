## 1. RC3 contract and release graph planning (#620)

- [x] 1.1 Freeze the three observed RC2 defects, their distinct owners, RC3 scope, privacy boundaries, supported host constraints, and stable-promotion separation in one OpenSpec proposal and design.
- [x] 1.2 Create and validate the RC3 capability specifications, issue-backed implementation tasks, and exact issue-map ownership slices.
- [x] 1.3 Reopen #492, make #620/#624/#625 direct native children and blockers, synchronize their milestone and issue task mirrors, and read back the release graph.

## 2. Plugin and runtime readiness (#620)

- [x] 2.1 Trace supported Codex marketplace, trusted hook, packaged installer, fresh-shell CLI resolution, generated config, and MCP registration paths; choose the smallest truthful readiness/repair route that the host actually supports.
- [x] 2.2 Implement and exercise older-runtime/registry, version-matched repair, wrong-project/newer-schema refusal, disabled-hook manual diagnostics, and stale-parent versus fresh-child states without automatic project mutation or database replacement; reserve live Codex trust/disable and running-host restart proof for #492 installed acceptance.
- [ ] 2.3 Run the owning installer/plugin/host checks and independent review; reconcile documentation and architecture with the supported lifecycle and resolve every material finding.

## 3. DOCX font-specific symbol continuity (#624)

- [ ] 3.1 Reproduce rendered and non-rendered `w:sym` in minimal valid DOCX packages across main body, headers, footers, footnotes, endnotes, referenced comments, frames, and nested text boxes; classify unreferenced glossary and referenced glossary/subdocument stories, trace extractor/relationship/text-index/symbol/coverage/publication callers, and identify verified Unicode mappings versus font/code-only identities.
- [ ] 3.2 Preserve every valid rendered symbol across package-reachable story parts within retained bounds by exact font/code, part, and occurrence identity in durable queryable evidence; decode verified mappings and represent unknown Unicode-text, unexamined-part, or accepted limit coverage without a global scan failure; retain surrounding text, malformed/unsafe-package refusal, and last-valid database state.
- [ ] 3.3 Add direct CLI and MCP full/incremental scan, search/overview/graph, post-reopen query of font/code/part/distinct repeated occurrences, note-marker versus note-story and unevaluated live-field coverage, referenced-unexamined story and parser-limit versus ZIP-safety/cancellation/deadline, external/escaping/missing/wrong-type relationship refusal, repair/retry, and Windows/Linux/macOS packaged regressions using causal DOCX fixtures.
- [ ] 3.4 Run `cargo test -p projectatlas-symbols`, the owning qualified CLI E2E, `cargo fmt --check`, affected workspace Clippy/check/tests, OpenSpec and IssueOps gates; obtain independent review and resolve findings.

## 4. PDF execution-fuel continuity (#625)

- [ ] 4.1 Reproduce a valid PDF that exhausts RC2 Wasmi execution fuel; measure fuel, wall time, memory, and output against adversarial controls, distinguish parser fuel from text-index byte caps, and trace document/publication ownership.
- [ ] 4.2 Fix demonstrated parser inefficiency and raise the finite fuel budget if measured valid-input proof justifies it under retained limits; represent remaining exhaustion with exact path, typed reason, and document-local incomplete coverage.
- [ ] 4.3 Add full/incremental CLI and MCP regressions for a valid PDF above the old ceiling and ordinary source beside a PDF that still exhausts retained fuel, plus malformed/oversized inputs, fault/rollback/retry, and supported Windows/Linux/macOS package behavior.
- [ ] 4.4 Run `cargo test -p projectatlas-symbols`, the owning qualified CLI E2E, `cargo fmt --check`, affected workspace Clippy/check/tests, OpenSpec and IssueOps gates; obtain independent review and resolve findings.
