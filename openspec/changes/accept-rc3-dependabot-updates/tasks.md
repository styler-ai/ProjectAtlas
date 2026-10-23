## 1. Specification and release graph (#639)

- [x] 1.1 Validate the OpenSpec contract; map #639 and aggregate #499 to these task slices, milestone v0.5.0-00, #492's sole direct-child/blocker graph, and the declared #639 prerequisite and #623-before-#636 baseline order without changing the completed RC3 issues.

## 2. Unambiguous PR ownership (#639)

- [x] 2.1 Resolve one standalone local ownership reference (closing or explicit non-closing Refs) before incidental upstream changelog links in IssueOps and a native protected-base pull_request_target PR-state job for the exact current PR head, including Dependabot; refresh issue changes by rerunning that eligible source; preserve unique-reference compatibility, foreign-reference exclusion, multiple-owner refusal, and milestone checks with read-only metadata validation and no PR-head code execution.
- [x] 2.2 Add causal Dependabot-style positive/negative parser and workflow-provenance tests; prove the native eligible job's current-head result and actual required-gate readiness, invalid metadata and stale-head refusal, and issue-event refresh at the hosted boundary; remove the temporary legacy pull_request job/event and bootstrap bridge, retire the obsolete custom publisher, and enforce the final narrow event policy; run IssueOps self-test, affected workflow proof, formatting, and required hosted gates, and resolve review findings.

## 3. JSONC parser dependency (#499)

- [x] 3.1 Refresh existing Dependabot PR #623 onto accepted main after #639, bind only #499 with a non-closing reference and matching milestone, and verify the exact locked dependency delta.
- [x] 3.2 Exercise valid/invalid JSONC configuration behavior and required locked Rust, IssueOps, platform, and current-head hosted checks; resolve review findings before acceptance.

## 4. Pinned install action (#499)

- [x] 4.1 Refresh existing Dependabot PR #635 onto accepted main after #639, bind only #499 with a non-closing reference and matching milestone, and verify the exact action pin delta.
- [x] 4.2 Prove pinned toolchain installation in affected CI/release workflows on supported runners, run IssueOps and current-head hosted gates, and resolve review findings before acceptance.

## 5. Grouped Cargo dependencies (#499)

- [x] 5.1 After PR #623 lands, refresh the grouped update from Dependabot PR #636 onto current main; preserve the bot branch and deliver its exact dependency delta and adapter repair through one issue-owned successor PR with only #499 and a matching milestone; verify the five-dependency Cargo.lock delta and retain a non-closing reference until PR #635 has also merged successfully and aggregate acceptance is complete before final closing-reference merge.
- [x] 5.2 Replace deprecated RMCP server/client info aliases with supported configuration types at existing adapters; preserve initialize, tool, selected-root, and missing-index behavior with positive and failure MCP tests.
- [x] 5.3 Build and exercise the updated optional parser pack's native grammar lifecycle on supported Windows and Linux hosts, including contained failure and active-index preservation; on macOS x64 and arm64, prove typed unsupported containment before worker launch and unchanged built-in parsing.
- [x] 5.4 Run cargo fmt --check, locked workspace check/clippy/test/doc, dependency/security policy, OpenSpec, IssueOps, and required current-head four-platform/installed gates; resolve review findings before acceptance.

## 6. Mermaid sanitizer security patch (#499)

- [x] 6.1 Retain accepted source-map-js 1.2.2 and resolve only DOMPurify 3.4.16 to compatible 3.4.17 in the Mermaid lockfile for GHSA-cg2g-xh64-v4q7, GHSA-mrv2-3pqr-j5jr and GHSA-c6gr-qvv4-5v3f; preserve the manifest and all unrelated locked entries, and verify the exact delta under aggregate #499.
- [x] 6.2 Prove lockfile installation and valid and invalid Mermaid parsing, inspect the low-severity dependency audit, remediate compatible findings and document the remaining KaTeX compatibility and parser-only reachability disposition while retaining the repository audit gate; run OpenSpec, IssueOps, required affected local and current-head hosted checks, and resolve independent and automated review findings before acceptance.

## 7. Rust dependency security patches (#499)

- [x] 7.1 Resolve only rustls 0.23.45 to 0.23.46 and ruzstd 0.9.0 to 0.9.1 in Cargo.lock for RUSTSEC-2026-0346 through RUSTSEC-2026-0349 and RUSTSEC-2026-0344; preserve existing dependency ranges, unrelated packages, completed aggregate tasks, and security policy.
- [x] 7.2 Prove the patched locked graph passes the existing dependency policy, affected token/parser consumers, mandatory local and current-head hosted platform gates, and independent review; require #593 to refresh from accepted patched main before restarting clean parser and installed-package acceptance.

## 8. Yanked ZIP provider replacement (#499)

- [x] 8.1 Replace yanked pinned zip 0.6.6 with non-yanked 9.0.3 and its required compression dependencies; adapt the existing DOCX reader and ZIP fixture/predecessor consumers while preserving exact part identity, duplicate handling, supported compression, bounded expansion, and typed failures without unrelated upgrades or weaker policy.
- [x] 8.2 Prove the unchanged locked dependency policy and causal valid, malformed, duplicate, and limit archive cases, mandatory local and current-head hosted affected gates, and independent review; accept the replacement on main before #593 refreshes and obtains a new clean parser handoff and installed-package proof.

## 9. Rust trust-store and I/O safety patches (#499)

- [x] 9.1 Resolve only rustls-webpki 0.103.15 to 0.103.16, webpki-roots 1.0.9 to 1.0.10 and tokio 1.53.1 to 1.53.3 for RUSTSEC-2026-0382, RUSTSEC-2026-0381 and RUSTSEC-2026-0386; verify the exact lockfile delta and unchanged license before retargeting its existing version-specific exception, preserve manifest ranges, unrelated packages, completed work and the security gate, and inspect affected consumers for required API adaptation.
- [x] 9.2 Pass unchanged dependency policy, affected TLS/build and asynchronous MCP/parser consumers, mandatory local and current-head hosted platform checks and independent review; accept the patched baseline on main before refreshing pending fixes and obtaining fresh RC4 parser-pack and installed-package proof under #492.
