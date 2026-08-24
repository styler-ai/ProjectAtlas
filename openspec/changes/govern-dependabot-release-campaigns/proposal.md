## Why

Weekly hosted Dependabot correctly produces update pull requests, but ProjectAtlas lacks one release-owned inventory that admits every relevant update, records a final disposition, and blocks release while any record remains pending or provisional. v0.5.0 needs one lean campaign and one bounded on-demand audit, not an issue per pull request or another dependency service.

## What Changes

- Keep weekly GitHub-hosted Dependabot as the only pull-request producer driven by `.github/dependabot.yml`.
- Add exactly one campaign issue per release/version with one machine-owned region containing the union of all currently open Dependabot pull requests and every Dependabot pull request created, updated, closed, or merged during the release window, plus separate pre-PR audit-finding records for reported would-be updates that have no hosted pull request yet.
- Require a final `accepted`, `deferred`, `declined`, or `superseded` disposition for every PR record; `pending` and `provisional` block the applicable stage. A post-contract PR becomes finally `accepted` only after authenticated readback proves `MERGED`, binds the reviewed source revision and resulting merge identity, and proves that merge is included in accepted current `main`; deferred, declined, and superseded remain valid non-merge outcomes. An unlinked pre-PR finding may be deferred, declined, or superseded but never accepted. `accepted` requires exact hosted linkage to a real PR whose record is finally accepted; other linked findings remain blocking until their real PR record reaches another authorized final disposition. Provenance such as `retrospective_precontract` is recorded separately and never substitutes for a disposition.
- Make #498 own intake, exact campaign relationships, inventory/disposition reconciliation, the smallest Dependabot-only IssueOps admission/merge-authorization extension, and one sequential manually dispatchable dependency audit.
- Make #499 own the v0.5.0 campaign, including live refresh of open #453/#454/#455 and `retrospective_precontract` provisional records for merged #457/#468, with no dependency-regression claim from the currently stale branch-global IssueOps failures and no claim that later gates ran before those historical merges.
- Require every Dependabot pull request to carry exactly one explicit non-closing relationship to the active campaign issue, the release milestone, normal affected-contract CI, review/thread/check readback, exact author/PR/head/body/inventory identity, and explicit Sol merge dispatch. Authorization is not acceptance: post-merge readback must prove exact merged state and accepted-`main` inclusion before the PR record becomes finally accepted. No update auto-merges or bypasses the protected contexts.
- Drive the bounded pre-RC and post-accepted-RC pre-stable audits solely from `.github/dependabot.yml` through one trusted default-branch `workflow_dispatch`, launchable with `gh workflow run`. Translate the supported hosted configuration into pinned official Dependabot CLI job input without dropping a field and bound per-entry/total time, output, and operation counts. Only a successful, certainly cleaned-up run may reconcile `clean` or `findings` into the campaign region. A failed, canceled, or uncertain hosted run receives no issues-write credential and performs no campaign mutation; its hosted conclusion plus absence or staleness of a matching current final campaign audit record blocks the checkpoint until a fresh successful audit/reconciliation. Findings create or refresh pre-PR records using audit/update identity only; a real PR number, author, head, milestone, and campaign relationship are recorded only after exact hosted PR readback and linkage.
- Keep the issues-write credential out of checkout persistence and every updater/proxy input, environment, mount, log, or container. Updater execution uses no credential for this public repository or a distinct read-only credential; all containers exit and bounded sanitized output validates before a later reconciliation step receives the issues-write token.
- Record two exact states in the same campaign region: `candidate_ready` binds the pre-RC audit, final inventory snapshot, and every accepted PR merge to the exact RC candidate input while #499 and #492 stay open, and `stable_ready` binds a later pre-stable audit plus every newly discovered record and accepted merge to the exact stable input after the accepted RC, after which #499 closes and stable #492 acceptance may proceed.
- Keep CI selection owned by `verify-affected-repository-contracts`; campaign automation consumes its result and does not duplicate its planner.
- Keep only the accepted shared proposal/design/specification/trust/dependency/task/architecture contract in checked section 1; keep #498 automation in unchecked section 2 and #499 campaign execution in unchecked section 3 so milestone planning admits executable delivery without claiming it complete.

Non-goals:

- Per-Dependabot-PR issues, a second issue graph, database, service, dependency model, updater, or inventory store.
- A nonexistent REST trigger for GitHub-hosted version updates, browser/UI automation, or direct reuse of `github/dependabot-action` in a repository workflow.
- Automatic merge, a broad bot exemption, weaker review/check gates, release from a non-final stage, or use of the retrospective path for any post-contract pull request.
- Parallel audit fan-out, arbitrary configuration beyond `.github/dependabot.yml`, or another `workflow_run` handoff framework.

The reviewed exact #498/#499 bodies may be published while the planning pull request remains open solely so normal unfiltered IssueOps/CI can validate the real packets. That temporary body-to-`main` architecture-link gap authorizes no readiness, native relationship, or implementation. This specification remains candidate-only backlog planning until the planning artifacts and later objective repository mechanism are accepted on `main`, exact evidence is read back, and the separately promoted authoritative graph agrees with hosted bootstrap. #498 becomes implementable after #497 is accepted on `main`; #499 waits for accepted #498 and exact Rust #482 on `main`.

## Capabilities

### New Capabilities

- `dependabot-release-campaigns`: one release-scoped Dependabot inventory, exact IssueOps ownership, bounded on-demand audits, final dispositions, and release reconciliation.

### Modified Capabilities

None.

## Impact

- Future Luna implementation is limited to existing GitHub workflows, IssueOps/repository policy scripts and tests, `.github/dependabot.yml`-driven audit translation, and campaign issue reconciliation.
- The dependency manifests and lockfile change only in their normal Dependabot pull requests; this capability adds no product runtime, Rust crate, SQLite state, package manager, or service.
- The v0.5.0 release graph gains #498 and #499, with #498 blocked by #497 and #499 blocked by #498 and #482; #492 remains the feature-free release root and closes last.
