## Context

Hosted pull-request validation already has the required authority split: the PR owner slice is compared with the live issue, and every unrelated slice is compared with the accepted base. The local pre-push hook instead invokes the global mode intended for `main` and release validation. Mutable progress on one independent issue can therefore invalidate another branch even when that branch did not change the unrelated slice.

## Goals / Non-Goals

**Goals:**

- Give local candidate branches the same owner/live and unrelated/base comparison used by hosted PR validation.
- Resolve one owning issue and one accepted base deterministically before the expensive checklist comparison.
- Keep global `main` and release validation unchanged.

**Non-Goals:**

- A new task store, branch registry, workflow framework, or dependency edge.
- Relaxing ownership, issue state, accepted-base, unrelated-slice, Mermaid, or checklist validation.
- Changing Rust product, SQLite, CLI, MCP, or installer behavior.

## Decisions

### Reuse the existing candidate comparison

Add an explicit local candidate CLI route that accepts one issue number and one base revision, then calls the existing owner-scoped accepted-base comparison without requiring a hosted PR payload. The hosted PR route remains responsible for resolving its owner from PR metadata; both routes share the same checklist comparison owner.

This is smaller and safer than a second comparison implementation or a pre-push-only checklist parser.

### Resolve candidate ownership from accepted-base commits

For a non-`main` branch, pre-push resolves the merge base with `origin/main`, extracts issue references in the repository's required `(#NNN)` commit-subject convention from commits after that base, and requires exactly one unique issue. Zero or several owners fail before live comparison. The candidate checker then independently rejects a closed or unmapped issue and unreadable base authority.

Using commit ownership keeps first publication possible before a hosted PR exists. Guessing from a branch name or mutable environment variable would be weaker and is rejected.

### Preserve global validation where it belongs

The hook retains global validation when the checked-out branch is `main`. CI pull requests remain PR-scoped, issue events remain planned-issue scoped, and push-to-main plus release validation remain global. The real self-test remains an independent mandatory command in every existing owner.

## Risks / Trade-offs

- **A candidate contains commits for several issues** -> fail closed and require the work to be split or rebased into one owning issue.
- **The remote base is missing or stale** -> fail closed rather than infer accepted authority; ordinary fetch/rebase resolves it.
- **Shell ownership extraction drifts from the commit convention** -> cover zero, one, duplicate-same, and multiple-owner subjects in the existing workflow-contract tests.
- **A branch changes an unrelated task slice** -> the existing accepted-base comparison rejects it even though unrelated live progress is ignored.

## Migration Plan

Land the scoped candidate mode and hook routing together. Existing hosted PR, issue-event, `main`, and release invocations keep their current flags. Reverting the change restores the former global local check without changing stored data or issue bodies.

## Dependencies / Cross-Issue Impact

#549 has no product prerequisite and is one direct child and blocker of release owner #492. It repairs the local publication gate used by independent issue branches; it does not add product dependency edges between #342, #544, #547, or their consumers.

## Open Questions

None.
