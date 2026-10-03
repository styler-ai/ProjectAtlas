## Context

IssueOps and PR-state independently scan PR title/body text for every local-looking issue number. Dependabot embeds upstream HTML changelogs with many such numbers, so the three current bot PRs cannot satisfy the one-owner gate by adding a single ProjectAtlas issue link. The required PR-state workflow uses `pull_request`; GitHub runs that workflow definition from the PR merge commit, so an earlier step is not inherently trusted. PR #636 also fails the Rust/macOS warning gate because RMCP 3.5 deprecates `ServerInfo` and `ClientInfo` aliases.

## Goals / Non-Goals

**Goals:** Make explicit local PR ownership unambiguous in IssueOps and a required base-controlled PR-state check; accept the three existing bot PRs with one open issue and matching milestone apiece; preserve JSONC, pinned-action, MCP, parser-pack, and supported-platform behavior; add each owner to the RC3 release graph.

**Non-Goals:** New PR templates or ownership infrastructure, broad dependency updates, a new MCP protocol, weakened warning gates, stable promotion, or treating upstream changelog links as ProjectAtlas owners when an explicit local closing line exists. The legacy no-closing-line fallback remains non-origin-aware.

## Decisions

### Prefer one standalone closing line

Recognize an unindented standalone `Fixes #N`, `Closes #N`, or `Resolves #N` line naming this repository before the legacy all-reference fallback. Reject multiple distinct explicit local owners and foreign qualified owners; retain the existing single-reference fallback for older PRs that lack a closing line. Mirror the rule in IssueOps and PR-state with matching positive/negative tests. The compatibility fallback can still misread one bare upstream HTML `#N` without an explicit line; require an explicit line on each new bot PR rather than claiming the legacy fallback is origin-aware.

### Keep the required owner check base-controlled

Move PR-state owner/milestone validation to a `pull_request_target` workflow sourced from the protected base, using only GitHub API PR/issue data. Do not check out or execute PR-head code or dependencies in that workflow; ordinary `pull_request` CI continues to validate candidate IssueOps files. Retain the required `pr-state` context and prove on a real PR that it attaches to the current head. Confirm a narrowly scoped repository Actions policy allows `pull_request_target` before relying on this route; if approval or current-head status proof is unavailable, leave #639 and RC3 blocked rather than weakening branch protection.

### Keep dependency PR ownership separate

Use #639 for the shared gate, #640 for PR #623, #641 for PR #635, and #642 for PR #636. The three bot PRs each get one standalone closing line, matching issue/PR milestones, and native release membership. #640 and #641 are independent after #639. #642 waits for #640 because both alter Cargo.lock; this avoids a speculative dependency on the action update. Refresh each bot PR against accepted main and rerun affected proof after every baseline change.

### Adapt only the grouped update's actual source break

Use RMCP 3.5's supported server/client configuration type names at existing adapter boundaries. Preserve initialize payloads and exact selected-root behavior with MCP tests. Prove native optional-parser loading at the updated language-pack version on supported Windows/Linux hosts; on macOS x64/arm64 prove typed `unsupported_containment` before worker launch and unchanged built-in parsing. Retain the current bot PR; if its branch cannot accept the minimal source adaptation, stop and revise the issue/PR route rather than force an unrelated merge.

## Risks / Trade-offs

- [An upstream changelog happens to contain a standalone closing line] → Require top-level unindented syntax and test HTML/list/foreign references; ambiguous explicit owners fail closed.
- [A PR changes its own ownership workflow or a privileged job executes candidate code] → Source the required check from protected base code, use API data only, and verify current-head required-check identity and event policy on GitHub.
- [Bot refresh rewrites the body or Cargo.lock] → Re-read the exact PR head/body after refresh and restore only its issue binding before validation.
- [RMCP aliases compile locally but protocol behavior drifts] → Exercise real initialize/tool calls, selected-root refusal, and installed MCP smoke.
- [Action or grammar bundle works on one runner only] → Keep pinned integrity and platform gates; validate parser-pack construction/loading separately.

## Migration Plan

Merge #639 first. Bind/refresh and merge #640 and #641 when independently green; refresh and adapt #636 after #640 lands. If an update fails, leave that issue/PR open and keep RC3 blocked; a compatible rollback reverts only the accepted dependency commit, without touching project databases. #492 then freezes the integrated candidate and performs installed release acceptance before publication.

## Dependencies / Cross-Issue Impact

#639 is a direct child/blocker of #492. #640, #641, and #642 are also direct children/blockers; #640/#641 are blocked by #639, and #642 is blocked by #639 and #640. Release owner #492 closes last and implements no dependency fix.

## Open Questions

None.
