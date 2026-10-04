## Context

IssueOps and PR-state independently scan PR title/body text for every local-looking issue number. Dependabot embeds upstream HTML changelogs with many such numbers, so the three current bot PRs cannot satisfy the one-owner gate by adding a single ProjectAtlas issue link. The required PR-state workflow uses `pull_request`; GitHub runs that workflow definition from the PR merge commit, so an earlier step is not inherently trusted. PR #636 also fails the Rust/macOS warning gate because RMCP 3.5 deprecates `ServerInfo` and `ClientInfo` aliases.

## Goals / Non-Goals

**Goals:** Make explicit local PR ownership unambiguous in IssueOps and a required base-controlled PR-state check; accept the three existing bot PRs under aggregate #499 and matching milestones; preserve JSONC, pinned-action, MCP, parser-pack, and supported-platform behavior; retain the two owners to the RC3 release graph.

**Non-Goals:** New PR templates or ownership infrastructure, broad dependency updates, a new MCP protocol, weakened warning gates, stable promotion, or treating upstream changelog links as ProjectAtlas owners when an explicit local ownership line exists. The fallback when no explicit ownership line exists remains non-origin-aware.

## Decisions

### Prefer one standalone ownership line

Recognize an unindented standalone `Fixes #N`, `Closes #N`, `Resolves #N`, or non-closing `Refs #N` line naming this repository before the legacy all-reference fallback. Reject multiple distinct explicit local owners, mixed closing/non-closing references even for the same issue, and standalone ownership declarations naming a foreign repository. Ordinary foreign-qualified mentions and changelog links remain valid context alongside one explicit local owner. Allow repeated references of one kind to the same issue and retain the existing single-reference fallback for older PRs that lack an explicit ownership line. Mirror the rule in IssueOps and PR-state with matching positive/negative tests. The compatibility fallback can still misread one bare upstream HTML `#N` without an explicit line; require an explicit line on each new bot PR rather than claiming the legacy fallback is origin-aware.

### Keep the required owner check base-controlled

Use a native job named `pr-state` in the protected-base `pull_request_target` workflow. Check out only the protected base revision and validate live owner/milestone metadata and the exact candidate head with read-only API permissions. GitHub reports the eligible native job result on that candidate head, including Dependabot heads; the validation job executes no PR-head code or dependencies. Issue changes rerun that same eligible source so fresh metadata is checked without changing the head.

GitHub does not evaluate every Actions event as a required PR check. Hosted proof showed that the former `workflow_run` publisher produced a successful exact-head API check while the normal merge API still reported that `pr-state` was expected. The final design therefore uses the eligible native job and removes the obsolete custom publisher rather than adding a second commit-status channel. Verify actual merge readiness, not only API check success.

Branch protection binds `pr-state` to its name and the shared Actions app, so a path-scoped event policy does not reserve that context against another PR-controlled workflow. The accepted boundary is protected-base execution plus independent verification of the exact trusted workflow path, event, revision, PR head, and API result before every merge; a green context name alone is insufficient. Same-repository write access remains trusted. Prove Dependabot publication, invalid metadata refusal, stale-head refusal, and issue-change refresh at the hosted boundary. Keep #639 and RC3 blocked until the required native result and normal merge readiness are verified.

### Keep the existing aggregate dependency owner

Reuse #499 for all three existing bot PRs; do not create one issue per PR. PR #623 and #635 use an explicit non-closing `Refs #499` line so the aggregate remains open until all planned updates pass. The final closeout uses `Closes #499` after all dependency tasks and acceptance are complete. The explicit parser recognizes the non-closing owner without scanning incidental upstream references and rejects conflicting closing/non-closing owners. #623 and #635 remain independent after #639; #636 waits for accepted PR #623 because both alter Cargo.lock. Each refreshed head receives its affected proof and independent review; #499 closes only after all three PRs merge successfully and final aggregate acceptance passes.

### Adapt only the grouped update's actual source break

Use RMCP 3.5's supported server/client configuration type names at existing adapter boundaries. Preserve initialize payloads and exact selected-root behavior with MCP tests. Prove native optional-parser loading at the updated language-pack version on supported Windows/Linux hosts; on macOS x64/arm64 prove typed `unsupported_containment` before worker launch and unchanged built-in parsing. Retain the current bot PR; if its branch cannot accept the minimal source adaptation, stop and revise the issue/PR route rather than force an unrelated merge.

## Risks / Trade-offs

- [An upstream changelog happens to contain a standalone closing line] → Require top-level unindented syntax and test HTML/list/foreign references; ambiguous explicit owners fail closed.
- [A PR changes its own ownership workflow or a privileged job executes candidate code] → Source the required check from protected base code, use API data only, and verify current-head required-check identity and event policy on GitHub.
- [Bot refresh rewrites the body or Cargo.lock] → Re-read the exact PR head/body after refresh and restore only its issue binding before validation.
- [RMCP aliases compile locally but protocol behavior drifts] → Exercise real initialize/tool calls, selected-root refusal, and installed MCP smoke.
- [Action or grammar bundle works on one runner only] → Keep pinned integrity and platform gates; validate parser-pack construction/loading separately.

## Migration Plan

The protected-base wakeup and default-branch custom publisher are already deployed, and the original legacy `pull_request` job/event is removed with the narrow event policy active. Their current-head API results do not satisfy the actual required gate, so migrate to the native protected-base job before accepting #639.

Use a temporary read-only CI bridge for the initial deployment. It may report `pr-state` only after reading the existing trusted publisher result for the exact current head. The bridge has no write permission and checks out no candidate code, but its workflow definition comes from the candidate; its green result is only a mechanical required-context bridge and is not trusted provenance. Independently verify the protected publisher workflow path, event, revision, head, and exact API result before the normal merge. The bridge exists only because the candidate protected-base workflow cannot run before it reaches the base branch; it is not a permanent validation authority.

After the native protected-base job reaches main, prove its real required result, Dependabot behavior, invalid metadata and stale-head refusal, and issue-event refresh. Remove the temporary bridge, obsolete custom publisher, and helpers without active consumers, then narrow the event policy to the final native workflow and its `pull_request_target`/`issues` events. Close #639 only after the final deployed workflow set and normal protected merge behavior pass. Never bypass or relax branch protection to deploy this repair.

Bind/refresh and merge PR #623 and #635 when independently green under #499; refresh and adapt #636 after #623 lands. If an update fails, leave that issue/PR open and keep RC3 blocked; a compatible rollback reverts only the accepted dependency commit, without touching project databases. #492 then freezes the integrated candidate and performs installed release acceptance before publication.

## Dependencies / Cross-Issue Impact

#639 and aggregate #499 are sole direct children/blockers of #492 for this work. #499 is blocked by #639. PR #636 also waits for PR #623's accepted Cargo.lock baseline within #499; PR #635 is independent. #640–#642 are superseded by #499 with their implementation tasks preserved here. #492 closes last and implements no dependency fix.

## Open Questions

None.
