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

- [x] 6.1 Resolve the existing dependency ranges to patched DOMPurify 3.4.16 and source-map-js 1.2.2 in the Mermaid lockfile for GHSA-p98j-92pf-mc4p and GHSA-68fv-2mgg-jv7q; preserve the manifest and unrelated dependency versions, and verify the exact locked delta under aggregate #499.
- [x] 6.2 Prove lockfile installation and valid and invalid Mermaid parsing, inspect the low-severity dependency audit, remediate compatible findings and document the remaining KaTeX compatibility and parser-only reachability disposition while retaining the repository audit gate; run OpenSpec, IssueOps, required affected local and current-head hosted checks, and resolve independent and automated review findings before acceptance.
