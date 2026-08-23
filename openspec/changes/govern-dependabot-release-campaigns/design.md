## Context

ProjectAtlas already uses weekly GitHub-hosted Dependabot entries in `.github/dependabot.yml` for Cargo and GitHub Actions. That producer correctly opens pull requests, but release acceptance has no single authoritative inventory of every update considered during a release window. The newly protected `issueops-implementation` and `issueops-merge-authorized` contexts also matter: #494 deliberately rejected all Dependabot merge authorization before a campaign contract existed, so the exception must be narrower than a generic bot bypass.

For v0.5.0, the release window starts at the v0.4.5 publication time, `2026-08-20T16:55:18Z`. Its initial live inventory is open #453, #454, and #455 plus merged-in-window #457 and #468. The current red `verify` results on #453/#454/#455 stop on stale branch-global #314 IssueOps drift and skip later E2E; they are not dependency-regression evidence. Each branch must first refresh or rebase onto accepted current `main`, then rerun the normal affected proof before disposition.

Current official-source facts were refreshed on 2026-08-23:

- [GitHub's Dependabot configuration reference](https://docs.github.com/en/code-security/reference/supply-chain-security/dependabot-options-reference) keeps hosted version-update production driven by `.github/dependabot.yml` on the default branch.
- [GitHub's REST Dependabot API](https://docs.github.com/en/rest/dependabot) exposes alerts, secrets, and repository-access administration, not a public endpoint that triggers hosted version updates. GitHub documents an interactive “Check for updates” path, but this design does not automate the UI.
- [`gh workflow run`](https://cli.github.com/manual/gh_workflow_run) dispatches workflows that declare `workflow_dispatch`.
- The official [`dependabot/cli` v1.92.0](https://github.com/dependabot/cli/tree/v1.92.0) release resolves to commit `13f626792737e1a2975e94feb8065874882616c4`, uses Docker, and reports would-be pull-request operations without creating hosted Dependabot pull requests. Its source owns the official `ghcr.io/dependabot/dependabot-updater-<ecosystem>` and `ghcr.io/dependabot/proxy` image contracts. The official [`dependabot-action`](https://github.com/dependabot/dependabot-action) states that it is used by GitHub.com and is not supported as a directly embedded repository workflow action.

## Goals / Non-Goals

**Goals:**

- Maintain exactly one active Dependabot campaign issue for each release/version and no issue per update pull request.
- Reconcile one body-embedded machine-owned inventory over the full release window.
- Require final, explicit dispositions before release.
- Admit and authorize Dependabot pull requests through the smallest exact-author IssueOps extension that preserves every protected gate.
- Run one bounded on-demand pre-RC and pre-stable audit from the same `.github/dependabot.yml` authority.
- Keep campaign ownership separate from affected-contract CI planning.

**Non-Goals:**

- No second issue graph, database, inventory service, dependency model, update producer, or per-PR issue flood.
- No automatic merge, arbitrary bot exemption, weaker review/check policy, or closing relationship to the campaign.
- No REST or UI automation for GitHub-hosted version updates.
- No direct repository use of `github/dependabot-action`, parallel audit matrix, or second `workflow_run` handoff framework.
- No product runtime, Rust crate, CLI/MCP protocol, package-manager, or SQLite change.

## Decisions

### The release graph declares one campaign issue

Each release graph has one optional `dependabot_campaign_issue` pointing to exactly one ordinary direct child of its release-acceptance issue. A graph with a campaign must reject an absent, extra, out-of-milestone, multiply parented, or non-child campaign issue. The v0.5.0 declaration points to #499. #492 remains the parent and closes last; #499 is blocked by #498 and #482, #498 is blocked by #497, and #497 has no blocker. Existing independent lanes remain parallel.

This reuses `openspec/issue-map.json` and native GitHub parent/blocker relationships. A second campaign graph or service would duplicate authority and was rejected.

### One bounded body region is the inventory store

The campaign issue contains one delimited machine-owned region inside its existing lean issue body. The region has a pull-request inventory, separate pre-PR audit-finding records, and bounded audit-run records while the human-authored issue keeps the required section order and one OpenSpec task mirror. Automation replaces only the delimited region using optimistic read/modify/readback; it does not rewrite other sections or keep another mutable copy.

The pull-request inventory is the union of:

- all currently open Dependabot pull requests targeting the release branch; and
- every Dependabot pull request created, updated, closed, or merged from the release-window start through campaign close.

Each PR record includes repository, PR number, author, base, exact head, package ecosystem and dependency/update summary, first-seen and last-seen timestamps, current PR state, milestone, campaign relationship, proof/review summary, and disposition. Records are keyed by PR number within the repository, updated idempotently, and never deleted during the campaign. A closed or merged record remains in history.

A CLI audit may report a would-be update before hosted Dependabot creates a pull request. That result is a separate pre-PR finding keyed by stable ecosystem, directory, dependency, and update/operation identity. It records first/last observing audit runs and times, current finding state, and any final repository-technical rationale, but has no PR number, author, base, head, milestone, or campaign relationship. When hosted Dependabot later creates a matching PR, automation first reads back the real repository/number/author/base/head/milestone/body relationship, creates or updates the independently complete PR record, and links the finding to that record. It never promotes audit output into synthetic PR identity.

Allowed final dispositions are `accepted`, `deferred`, `declined`, and `superseded`. Intake begins `pending`; a not-yet-final judgment may be `provisional`. Both non-final states block the applicable readiness checkpoint. Provenance is orthogonal: `retrospective_precontract` identifies a PR merged before the campaign contract without becoming a fifth disposition or claiming post-hoc pre-merge authorization. A final disposition records a short repository-technical rationale and, where applicable, the superseding or corrective PR.

An unlinked audit finding stays pending/provisional until an authorized reconciliation marks it `deferred` or `declined` with a repository-technical rationale, marks it `superseded` by another exact finding or PR, or links it to a real PR whose own record then reaches a final disposition. An unlinked finding can never be `accepted`; that finding disposition requires a linked real PR whose own record is finally `accepted`. A link alone is not final, and every other linked finding remains blocking until its real PR reaches an authorized final disposition.

The same machine region records the exact published campaign-contract activation revision and owns one closed campaign stage: `collecting`, `candidate_ready`, or `stable_ready`. A readiness record binds the release/candidate revision, digest over PR, finding, and audit-run records, full-union reconciliation high-water mark, configuration digest, audit run identity and outcome, immutable CLI/updater/proxy identities, and stage timestamp. Any missing, malformed, stale, or mismatched field, any unresolved audit finding, or a newly observed event before the stage-consuming publication readback returns the campaign to `collecting`; this is not a second issue, graph, database, or proof ledger.

### Intake and CI planning have different owners

The campaign automation owns discovery, exact campaign/PR relationship, body-region reconciliation, disposition state, audit records, and release readiness. `verify-affected-repository-contracts` owns build/test/quality selection. Campaign automation reads the protected-context result but does not reproduce path classification or contract selection; the CI planner does not mutate the campaign.

This single-writer split prevents two inventory stores or two planners.

### Future Dependabot pull requests get one narrow exact-author IssueOps path

The implementation context admits the campaign path only when all of these bind to the same current readback:

- the pull-request event actor is exactly `dependabot[bot]` and authenticated `gh pr view` author readback is exactly the source-aware GitHub App identity `app/dependabot`; no other actor or App identity is equivalent;
- the release graph names exactly one active campaign and the campaign is the PR milestone's direct release child;
- the PR body contains exactly one standalone non-closing `Relates to #<campaign>` relationship and no closing campaign reference;
- PR number, base, exact head, body, milestone, and campaign inventory record agree;
- the inventory record is current for that exact head and is not absent or malformed.

The normal implementation-owner closing-reference path remains unchanged for all other authors. The Dependabot extension is not an arbitrary app/bot role and cannot be used by a human or another bot. Keeping both source-specific identities explicit avoids either rejecting the real GitHub App representation or broadening admission to every App-authored pull request.

Merge authorization remains explicit and one-shot. It reads back exact author, campaign mapping, PR number/head/body/milestone, inventory record, review decision, unresolved threads, every protected check, current base, and branch synchronization. Only an explicit Sol dispatch for that exact head may authorize the existing `issueops-merge-authorized` context. There is no automatic merge or gate waiver. Any new commit invalidates the authorization and requires fresh proof/readback/dispatch.

#494's blanket rejection can be narrowed only after these positive invariants and negative tests exist; until then rejection remains the safe behavior. This path applies only to pull requests opened or updated under the published campaign contract. It has no retrospective exception: every post-contract Dependabot pull request must satisfy admission, protected proof, review/thread readback, and explicit pre-merge Sol authorization before merge.

### Pre-contract merges use truthful retrospective reconciliation

#457 and #468 merged before the campaign, `issueops-implementation`, and `issueops-merge-authorized` contracts existed. Their records therefore use provenance `retrospective_precontract` and preserve the explicit historical gate gap. Their original five successful checks are historical evidence only; they do not satisfy later IssueOps, review, campaign, or authorization requirements.

A retrospective record may become finally `accepted` only after all of the following are read back together:

- the exact dependency head, merge commit, changed-file set, merge time, and ancestry/inclusion in current `main`;
- after a metadata-only safety preflight, exactly one post-hoc non-closing `Relates to #499` relationship and the `v0.5.0-00` milestone; a failed or ambiguous metadata update blocks acceptance;
- a fresh Sol review of both the exact merged diff and its behavior integrated into current `main`, with zero unresolved actionable human or automated findings; and
- fresh complete current-`main` proof, including the current protected and all four platform contexts required by the affected-contract contract.

No record claims a historical review or pre-merge Sol dispatch that did not occur. If retrospective review or current proof fails, the record cannot be `accepted`: an actual revert, corrective, or superseding delivery must pass its normal current gates, and the historical record becomes `declined` when fully reverted or `superseded` with the exact accepted successor. This bounded exception is selected only when the recorded merge predates the published campaign-contract activation revision.

### One trusted sequential audit complements hosted weekly updates

One manually dispatchable job stays in the existing trusted default-branch workflow and declares least job-level permissions. The CLI harness launches it with `gh workflow run`; it does not attempt a nonexistent hosted-update REST trigger or drive the GitHub UI.

One standard-library translator parses only the current default-branch `.github/dependabot.yml` and produces official CLI job-description input for `dependabot update -f <input> --output <bounded-output>`; `dependabot test` is not the audit entrypoint because it replays a smoke-test expectation. The translator's closed supported surface is:

- top-level `version: 2` and `updates` only;
- the current `cargo` -> CLI `cargo` and `github-actions` -> CLI `github_actions` ecosystems, exactly one `directory`, `target-branch`, and `schedule.interval` (validated as hosted-schedule policy but not invented as an update-job field);
- group names with supported `patterns`, `exclude-patterns`, `dependency-type`, and group `update-types`, including the current Cargo `minor-and-patch` group and the current ungrouped GitHub Actions entry;
- `allow` rules with dependency name/type and allow-style semantic update types, plus `ignore` rules with dependency name, versions expanded losslessly into conditions, and ignore-style semantic update types; and
- `versioning-strategy` and `exclude-paths` only where the pinned CLI job model has an exact owning field.

Unknown keys, `directories`, unsupported ecosystems or group rules, lossy policy combinations, ambiguous values, or a field that cannot be represented exactly fail the audit before container startup. Fixtures freeze both current repository entries, allow/ignore precedence, group/update-type translation, and unsupported/lossy rejection. Later Luna implementation pins:

- `dependabot/cli` to v1.92.0 commit `13f626792737e1a2975e94feb8065874882616c4` or a newer Sol-approved official immutable replacement refreshed at implementation time; and
- every official updater and proxy image used by those entries to an OCI digest recorded in the workflow, never a mutable tag alone.

Each translated entry runs sequentially with a checked-in positive per-entry timeout passed through the CLI's `--timeout`; the workflow has a larger positive total `timeout-minutes`. The contract also pins positive per-entry and total output-byte and recorded-operation ceilings. Timeout, overflow, unknown output operation, cancellation, signal failure, nonzero exit, or uncertain completion/cleanup makes the hosted workflow run fail. The wrapper forwards cancellation/termination to the one active CLI process and waits within a bounded cleanup interval. Failure or uncertainty stops before credentialed campaign reconciliation; no body outcome is required from untrusted or incompletely cleaned-up state.

Job-level permission alone is not credential isolation. Checkout uses `persist-credentials: false`. The issues-write token is absent from updater/proxy command input, environment, mounts, logs, credentials, generated job YAML, and persisted Git configuration. Because ProjectAtlas is public, the updater uses no repository token; if rate limits later require one, it must be a distinct read-only credential supplied only to the official proxy and never the updater. Only after all updater/proxy containers exit and their output passes bounded schema validation plus credential/log sanitization may a later reconciliation step receive an issues-write token scoped to campaign mutation/readback. A failed, canceled, or uncertain run never receives that token. The existing hosted workflow run conclusion, together with absence or staleness of a matching final successful audit record in the campaign region, is the authoritative blocking fact; a later fresh successful audit/reconciliation is the only recovery. Negative tests inspect generated environment, input, mounts, proxy/updater arguments, logs, persisted Git configuration, cancellation, cleanup, hosted-run binding, and stale/missing campaign-record refusal.

`clean` means a successful complete run reported no would-be update operations. `findings` means a successful complete run reported one or more bounded create/update/close operations, each of which creates or refreshes a pending pre-PR audit-finding record without fabricated hosted identity. These are the only outcomes ingested into the campaign body region, and only after credential-safe reconciliation. Invalid/lossy configuration, tooling/image/pin or API/rate failure, timeout, overflow, cancellation, schema/sanitization failure, or incomplete execution/cleanup remains a failed hosted run with no campaign mutation. A current checkpoint requires a matching successful hosted run plus current final `clean|findings` campaign audit record; either fact missing, stale, or mismatched blocks. `clean` cannot silently finalize existing pending or provisional PR or finding records.

One sequential job avoids duplicate runners, rate bursts, and cross-job aggregation. A second `workflow_run` handoff is unnecessary: the same trusted job keeps the token out of the updater phase and injects it only into the later reconciler after verified container cleanup.

### Candidate-ready and stable-ready are distinct checkpoints

`candidate_ready` is permitted only after an exact RC candidate revision is frozen, the pre-RC audit completes successfully as `clean` or `findings`, the matching hosted run and current final campaign audit record agree, every PR record is final for its exact head/state, every unlinked finding is finally deferred/declined/superseded, every linked finding points to a finally dispositioned real PR, and `accepted` findings point only to finally accepted real PRs. The publication preflight rereads the same inventory/config/audit/hosted-run identities with no intervening event. This state permits `v0.5.0-rc1` publication while #499 and #492 intentionally remain open so weekly intake continues through the stable window.

After RC1 is independently accepted, the campaign returns to `collecting` for newly created or updated records. `stable_ready` requires a later pre-stable audit on a current accepted `main` revision, matching successful hosted-run and final `clean|findings` campaign-record readback, the same unlinked/linked/accepted finding rules, every PR record newly observed since `candidate_ready` final, and exact final readback of the full window union. Only then may #499 close; that native blocker removal allows #492 to begin stable publication acceptance. Stable acceptance still requires every release child closed, and #492 closes last after stable hosted readback.

All implementation-bearing feature, bug, and maintenance children—including #497 and #498—must be accepted and closed before the RC candidate checkpoint. For v0.5.0 the only intentional open governance issues at RC publication are #499 and unparented release owner #492.

### The v0.5.0 campaign delivery order follows compatibility risk

After exact Rust #482 is accepted on `main`, campaign delivery is #453 (`object`, Rust/parser) then #454 (`rusqlite`, Rust/database/SQLite implementation and review) then #455 (`rmcp`, Rust/MCP). Each branch first refreshes or rebases onto accepted current `main`, resolves only its own new findings, and reruns the same affected-contract quality bar. Existing red checks caused by stale shared IssueOps state are recorded as stale-baseline evidence, not package regressions.

The sequence is intentionally local to these high-impact open updates. It does not make #497 a fake blocker of independent product work. #457 (dependency head `7942d8e…`, merge `e142f95…`) and #468 (dependency head `bad62e3…`, merge `409a28d…`) follow the retrospective contract above; their original five green checks, absent milestone, and absent review are retained as history, never promoted into later-gate proof.

### Rust, storage, and resource pattern fit

Campaign automation introduces no Rust code. The Rust pattern-fit judgment is “no named Rust pattern needed”; the seven-crate product boundary stays unchanged. Database and SQLite are N/A for #498/#499 campaign storage because the issue body and release graph are the only authorities. The separate #454 implementation must load the Rust, database, and SQLite skill stack because it changes `rusqlite`, but this campaign design does not pre-decide that package migration.

Inventory reconciliation is linear in the bounded release-window PR record count. The audit is linear in `.github/dependabot.yml` entries and intentionally sequential, with one updater/proxy pair active at a time. No long-lived worker, queue, lock, local persistent bytes, or additional service exists.

## Risks / Trade-offs

- **A PR could be admitted under the wrong campaign** → bind exact author, milestone release graph, standalone relationship, PR/head/body, and inventory record in both implementation and merge contexts.
- **Automation could overwrite human issue prose** → mutate one delimited machine region with optimistic concurrency and read back the exact body.
- **A closed PR could disappear from release history** → retain every release-window record and update state instead of deleting rows.
- **A successful audit could be mistaken for campaign completion or forced into fabricated PR identity** → keep a separate pre-PR finding record, link only after exact hosted PR readback, and require every PR/finding record to be final in the stage-bound digest.
- **Mutable container tags could drift** → pin the CLI source and every updater/proxy image by immutable commit/digest and fail on missing/mismatched pins.
- **The issues-write token could reach updater-controlled code or an uncertain run could require an impossible failure mutation** → use checkout without persisted credentials, no updater token (or a distinct proxy-only read credential), issue-write only after certain exit and validated output, no campaign mutation on failure/uncertainty, and hosted-run plus missing/stale-record blocking until a fresh successful reconciliation.
- **A lossy configuration translation could audit different updates than hosted Dependabot** → support one closed field mapping with current Cargo-group and GitHub Actions fixtures and fail before containers on every unknown, unsupported, or lossy field.
- **RC and stable readiness could form a closure cycle** → bind `candidate_ready` while #499/#492 stay open, then require post-RC `stable_ready` before #499 closes and #492 stable acceptance begins.
- **A pre-contract merge could be granted fictional later authorization** → retain `retrospective_precontract` provenance, original-check history, current-main proof, fresh Sol review, explicit historical gap, and require a real successor when proof fails.
- **Current red checks could be misdiagnosed as dependency breakage** → require refresh/rebase and fresh protected proof before technical disposition; label stale shared IssueOps failures as baseline drift.
- **Serial updates take longer** → accept the bounded sequence because object/parser, rusqlite/storage, and rmcp/MCP compatibility need distinct review; unrelated release lanes remain parallel.

## Migration Plan

1. Land #497 so both human and Dependabot pull requests have one affected-proof contract.
2. Implement #498's release-graph campaign declaration, exact-author future-PR IssueOps path, retrospective contract, body-region/stage reconciler, isolated bounded audit job, tests, and architecture readback while the blanket Dependabot merge rejection remains the default on any mismatch.
3. After #482 and #498 are accepted on `main`, publish/read back #499's native relationships and initial inventory, refresh/rebase #453/#454/#455, deliver them in order, and reconcile #457/#468 truthfully.
4. Reach exact `candidate_ready`, publish/read back RC1 while #499/#492 remain open, then continue intake, run the post-accepted-RC pre-stable audit, reconcile every new record, reach `stable_ready`, and close #499.
5. #492 verifies every child is closed, performs stable publication/readback, and closes last.
6. Roll back automation by restoring blanket Dependabot merge rejection and manual campaign-region maintenance. The release remains blocked rather than bypassing the inventory.

## Open Questions

None. OCI digests are intentionally resolved from the official registry at #498 implementation time and checked in beside the immutable CLI pin so the workflow never depends on a mutable tag.
