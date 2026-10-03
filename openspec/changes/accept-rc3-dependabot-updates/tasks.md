## 1. Specification and release graph (#639)

- [x] 1.1 Validate the OpenSpec contract; map #639 and aggregate #499 to these task slices, milestone v0.5.0-00, #492's sole direct-child/blocker graph, and the declared #639 prerequisite and #623-before-#636 baseline order without changing the completed RC3 issues.

## 2. Unambiguous PR ownership (#639)

- [ ] 2.1 Resolve one standalone local ownership reference (closing or explicit non-closing Refs) before incidental upstream changelog links in IssueOps and a protected-base PR-state owner gate; preserve unique-reference compatibility, foreign-reference exclusion, multiple-owner refusal, and milestone checks without executing PR-head code in the required gate.
- [ ] 2.2 Add causal Dependabot-style positive/negative parser and workflow-provenance tests; run IssueOps self-test, affected workflow proof, formatting, and hosted current-head PR-state gates; verify the required event policy and resolve review findings.

## 3. JSONC parser dependency (#499)

- [ ] 3.1 Refresh existing Dependabot PR #623 onto accepted main after #639, bind only #499 with a non-closing reference and matching milestone, and verify the exact locked dependency delta.
- [ ] 3.2 Exercise valid/invalid JSONC configuration behavior and required locked Rust, IssueOps, platform, and current-head hosted checks; resolve review findings before acceptance.

## 4. Pinned install action (#499)

- [ ] 4.1 Refresh existing Dependabot PR #635 onto accepted main after #639, bind only #499 with a non-closing reference and matching milestone, and verify the exact action pin delta.
- [ ] 4.2 Prove pinned toolchain installation in affected CI/release workflows on supported runners, run IssueOps and current-head hosted gates, and resolve review findings before acceptance.

## 5. Grouped Cargo dependencies (#499)

- [ ] 5.1 After PR #623 lands, refresh existing Dependabot PR #636 onto current main, bind only #499 with a matching milestone, and verify the five-dependency Cargo.lock delta; retain a non-closing reference until PR #635 is also merged or otherwise dispositioned and aggregate acceptance is complete before final closing-reference merge.
- [ ] 5.2 Replace deprecated RMCP server/client info aliases with supported configuration types at existing adapters; preserve initialize, tool, selected-root, and missing-index behavior with positive and failure MCP tests.
- [ ] 5.3 Build and exercise the updated optional parser pack's native grammar lifecycle on supported Windows and Linux hosts, including contained failure and active-index preservation; on macOS x64 and arm64, prove typed unsupported containment before worker launch and unchanged built-in parsing.
- [ ] 5.4 Run cargo fmt --check, locked workspace check/clippy/test/doc, dependency/security policy, OpenSpec, IssueOps, and required current-head four-platform/installed gates; resolve review findings before acceptance.
