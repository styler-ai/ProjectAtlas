## 1. Root-Cause Repair

- [x] 1.1 Map issue #409 and align its live checklist with this narrow stdin-isolation contract.
- [x] 1.2 Close stdin on the shared effective local Git-config subprocess without changing its existing lifecycle or result semantics.
- [x] 1.3 Add a persistent-open stdio MCP regression covering `atlas_session_brief`, `atlas_root`, `atlas_init`, and immediate session reuse.

## 2. Compatibility and Landing

- [x] 2.1 Run the focused regression plus existing linked-worktree, bare-root, wrong-root, missing-index, included-config, and no-implicit-mutation coverage.
- [x] 2.2 Pass `cargo fmt --all -- --check`, package Clippy with `-D warnings`, strict OpenSpec validation, and `.github/scripts/issue-checklists.py`.
- [x] 2.3 Review the exact current-main diff, inspect all hosted review feedback and checks, then land the verified head for v0.4.2.
