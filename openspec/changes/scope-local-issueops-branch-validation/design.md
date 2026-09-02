## Context

Hosted pull-request validation already has the required authority split: the PR owner slice is compared with the live issue, and every unrelated slice is compared with the accepted base. The local pre-push hook must select its scope from Git's pushed remote ref-update records rather than the checkout branch. Mutable progress on one independent issue can therefore invalidate another branch even when that branch did not change the unrelated slice.

## Goals / Non-Goals

**Goals:**

- Give local candidate branches the same owner/live and unrelated/base comparison used by hosted PR validation.
- Resolve pushed remote-ref scope, one owning issue, and one accepted base deterministically before the expensive checklist comparison.
- Keep global `main` and release validation unchanged.

**Non-Goals:**

- A new task store, branch registry, workflow framework, or dependency edge.
- Relaxing ownership, issue state, accepted-base, unrelated-slice, Mermaid, or checklist validation.
- Changing Rust product, SQLite, CLI, MCP, or installer behavior.

## Decisions

### Reuse the existing candidate comparison

Add an explicit local candidate CLI route that accepts one issue number and one base revision, then calls the existing owner-scoped accepted-base comparison without requiring a hosted PR payload. The hosted PR route remains responsible for resolving its owner from PR metadata; both routes share the same checklist comparison owner.

This is smaller and safer than a second comparison implementation or a pre-push-only checklist parser.

### Resolve candidate scope and ownership from push records

Pre-push consumes Git's standard four-field ref-update records. Any valid update targeting `refs/heads/main` selects global validation, even from a feature checkout; candidate validation is allowed only when exactly one valid target is a non-main `refs/heads/*` branch whose non-zero local object ID equals the validated checked-out `HEAD`, whose worktree has no tracked, staged, or non-ignored untracked changes, and whose tracked index entries are not marked `assume-unchanged` or `skip-worktree`. Multiple non-main updates, deletions, empty input, malformed records, unsupported targets, dirty candidate state, or hidden tracked index state fail closed before owner or base extraction. The clean-worktree and Git-index guards bind candidate reads of the local issue-map, mapped task files, and linked documentation to the validated `HEAD` without inspecting a temporary tree. For the candidate route, pre-push resolves the merge base with `origin/main`, validates every post-base commit subject individually, and requires each nonblank subject, including merge commits, to contain exactly one well-formed `(#NNN)` reference with the same owner and no unmatched `(#` fragment. The candidate checker verifies every linked architecture document is a tracked Markdown blob in the submitted local object ID before reading its clean worktree copy. Empty, blank, malformed, multiple, or different references fail before any owner is returned or live comparison begins. The candidate checker then independently rejects a closed or unmapped issue and unreadable base authority.

Using push-target scope plus commit ownership keeps first publication possible before a hosted PR exists. Guessing scope or ownership from a checkout branch name or mutable environment variable would be weaker and is rejected.

### Preserve global validation where it belongs

The hook retains global validation for any push targeting `refs/heads/main`, and uses the candidate route only for one valid non-main branch target whose local object is the current `HEAD`, whose worktree is clean, and whose tracked index has no hidden entries. Multiple non-main targets, deletions, local-object mismatches, dirty candidate state, or hidden tracked index state fail closed. CI pull requests remain PR-scoped, issue events remain planned-issue scoped, and release validation remains global. The real self-test remains an independent mandatory command in every existing owner.

## Risks / Trade-offs

- **A candidate contains commits for several issues** -> fail closed and require the work to be split or rebased into one owning issue.
- **The remote base is missing or stale** -> fail closed rather than infer accepted authority; ordinary fetch/rebase resolves it.
- **Push-target or shell ownership extraction drifts from the contract** -> cover main-from-feature, ordinary candidate, mismatched local object, mixed refs, malformed records, zero, one, multiple non-main updates, duplicate-same, and multiple-owner inputs in the existing workflow-contract tests.
- **A branch changes an unrelated task slice** -> the existing accepted-base comparison rejects it even though unrelated live progress is ignored.

## Migration Plan

Land the scoped candidate mode and hook routing together. Existing hosted PR, issue-event, `main`, and release invocations keep their current flags. Reverting the change restores the former global local check without changing stored data or issue bodies.

## Dependencies / Cross-Issue Impact

#549 has no product prerequisite and is one direct child and blocker of release owner #492. It repairs the local publication gate used by independent issue branches; it does not add product dependency edges between #342, #544, #547, or their consumers.

## Open Questions

None.
