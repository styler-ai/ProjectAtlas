## Why

The local pre-push hook compares every open issue's mutable live checklist with one candidate branch's OpenSpec files. Two independently completed branches can therefore invalidate each other's local publication gate even when each branch preserves unrelated task slices exactly as they existed on accepted `main`.

## What Changes

- Add a candidate-branch IssueOps mode that validates one owning issue against live state and compares every unrelated task slice with an accepted base.
- Route pre-push validation from its pushed remote refs: any `refs/heads/main` update remains global, while exactly one non-main `refs/heads/*` update whose non-zero local object is the checked-out `HEAD` uses candidate mode; multiple, deleted, or mismatched candidate updates fail closed and release checks remain global. Candidate mode also requires a clean worktree with no tracked `assume-unchanged` or `skip-worktree` index entries so local issue-map, task, and documentation reads come from that exact `HEAD`, and every post-base commit subject, including merge commits, carries exactly one same-owner `(#NNN)` reference.
- Fail closed when candidate ownership or accepted-base authority is missing or ambiguous.
- Cover concurrent task drift and the failure boundaries without adding a task store, branch registry, or workflow framework.

## Capabilities

### New Capabilities

- `release-issueops`: Defines branch-scoped checklist authority for local publication alongside the existing hosted PR, planned-issue, and global release scopes.

### Modified Capabilities

None.

## Impact

- Affects `.github/scripts/issue-checklists.py`, `.githooks/pre-push`, their existing workflow-contract tests, and the focused IssueOps workflow/architecture documentation.
- Does not change product Rust behavior, SQLite data, public CLI/MCP contracts, hosted PR ownership, issue-event validation, or global `main` and release gates.

## Non-Goals

- A new workflow framework, branch registry, task store, or model-routing rule.
- Weakening unrelated-slice validation or global validation on `main` and at release time.

This change is ready for implementation as release-blocking Issue #549 in milestone `v0.5.0-00`.
