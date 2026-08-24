## 1. Contract and architecture (#497)

- [x] 1.1 Freeze the #497 proposal, design, capability specification, exact impact/fallback and compatibility boundaries, dependency order, task ownership, and focused architecture view before milestone planning.

## 2. Lean affected-contract delivery (#497)

- [ ] 2.1 Check in the closed impact contract and deterministic fixtures, deriving Rust reverse dependencies from one `cargo metadata` result and declaring only the specified non-Cargo cross-contract edges, with unknown, shared, planner-owning, stale, malformed, rename/delete, and metadata-failure inputs selecting complete proof.
- [ ] 2.2 Implement the single Python-standard-library planner and existing-workflow integration for human and Dependabot pull requests, exact base/head/event/contract/workflow/toolchain/platform plan binding, affected execution, stable required-context aggregation, full default-branch/scheduled/release proof, and read-only superseded-PR cancellation without changing any proof boundary.
- [ ] 2.3 Add positive, negative, failure, compatibility, and representative-scale coverage for every classifier/edge, every rename/deletion selecting complete proof even when both paths are classified, ordinary-addition/modification unions, plan/result mismatch, trusted not-applicable state, missing/skipped/canceled proof, human/Dependabot parity, all four platform contexts, full-boundary fallback, #341/#366 reuse, and the repository's complete CI, IssueOps, OpenSpec, dependency, packaging, installer, release, locked Mermaid parser/CLI render, and visual-semantic gates.
- [ ] 2.4 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.
