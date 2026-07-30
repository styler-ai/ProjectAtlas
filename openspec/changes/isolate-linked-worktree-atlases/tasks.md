## 1. Real Worktree Reproduction and Routing

- [x] 1.1 Map `isolate-linked-worktree-atlases` to GitHub issue #382 in `openspec/issue-map.json` and keep the issue checklist synchronized with this file.
- [x] 1.2 Add a real `git worktree add` integration fixture without a worktree-container ignore rule and prove the main checkout currently risks admitting branch-only sibling source before the structural exclusion fix.

## 2. Structural Isolation and Automatic First Use

- [x] 2.1 Reuse bounded common-Git/worktree policy discovery to derive same-repository registered worktree roots once, prune every other in-root worktree through existing excluded prefixes, fail typed on boundary uncertainty, and leave unrelated nested repositories/submodules compatible.
- [x] 2.2 Return typed non-mutating `init_required` guidance for read-only use of an uninitialized selected worktree and make the shipped agent workflow automatically invoke `atlas_init` for that exact `project_path`.
- [x] 2.3 Prove CLI `init`, first-write `scan`, `watch --once`, and interleaved MCP per-call `project_path` retain independent worktree-local DB/config/host-cwd/identity/purpose/source/summary/symbol/graph state and typed bare-root guidance.

## 3. Recursive Coverage and Verification

- [x] 3.1 Cover deep eligible folders, ignore-before-descent, repository `/.worktrees/` defense-in-depth policy, branch-only clean/dirty/add/edit/delete/switch behavior, sibling/common-Git exclusion without ignore, and unrelated nested-Git compatibility on Windows and Linux.
- [ ] 3.2 Run focused filesystem/CLI/MCP/E2E tests, `cargo fmt --check`, workspace check/clippy/test/doc gates, OpenSpec validation, IssueOps, and live review-feedback reconciliation.
