## Context

ProjectAtlas already uses weekly GitHub-hosted Dependabot entries in `.github/dependabot.yml` for Cargo and GitHub Actions. That producer correctly opens pull requests, but release acceptance has no single authoritative inventory of every update considered during a release window. The newly protected `issueops-implementation` and `issueops-merge-authorized` contexts also matter: #494 deliberately rejected all Dependabot merge authorization before a campaign contract existed, so the exception must be narrower than a generic bot bypass.

For v0.5.0, the release window starts at the v0.4.5 publication time, `2026-08-20T16:55:18Z`. Its initial live inventory is open #453, #454, and #455 plus merged-in-window #457 and #468. The current red `verify` results on #453/#454/#455 stop on stale branch-global #314 IssueOps drift and skip later E2E; they are not dependency-regression evidence. Each branch must first refresh or rebase onto accepted current `main`, then rerun the normal affected proof before disposition.

Current official-source facts were refreshed on 2026-08-23:

- [GitHub's Dependabot configuration reference](https://docs.github.com/en/code-security/dependabot/working-with-dependabot/dependabot-options-reference) keeps hosted version-update production driven by `.github/dependabot.yml` on the default branch.
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

The campaign issue contains one delimited machine-owned region inside its existing lean issue body. The region has a pull-request inventory and bounded audit records while the human-authored issue keeps the required section order and one OpenSpec task mirror. Automation replaces only the delimited region using optimistic read/modify/readback; it does not rewrite other sections or keep another mutable copy.

The pull-request inventory is the union of:

- all currently open Dependabot pull requests targeting the release branch; and
- every Dependabot pull request created, updated, closed, or merged from the release-window start through campaign close.

Each PR record includes repository, PR number, author, base, exact head, package ecosystem and dependency/update summary, first-seen and last-seen timestamps, current PR state, milestone, campaign relationship, proof/review summary, and disposition. Records are keyed by PR number within the repository, updated idempotently, and never deleted during the campaign. A closed or merged record remains in history.

Allowed final dispositions are `accepted`, `deferred`, `declined`, and `superseded`. Intake begins `pending`; a retrospective or not-yet-final judgment may be `provisional`. Both non-final states block release. A final disposition records a short repository-technical rationale and, where applicable, the superseding PR or follow-up issue. The v0.5.0 records for #457 and #468 begin provisional accepted and must be finalized before release.

### Intake and CI planning have different owners

The campaign automation owns discovery, exact campaign/PR relationship, body-region reconciliation, disposition state, audit records, and release readiness. `verify-affected-repository-contracts` owns build/test/quality selection. Campaign automation reads the protected-context result but does not reproduce path classification or contract selection; the CI planner does not mutate the campaign.

This single-writer split prevents two inventory stores or two planners.

### Dependabot gets a narrow exact-author IssueOps path

The implementation context admits the campaign path only when all of these bind to the same current readback:

- the pull-request event actor is exactly `dependabot[bot]` and authenticated `gh pr view` author readback is exactly the source-aware GitHub App identity `app/dependabot`; no other actor or App identity is equivalent;
- the release graph names exactly one active campaign and the campaign is the PR milestone's direct release child;
- the PR body contains exactly one standalone non-closing `Relates to #<campaign>` relationship and no closing campaign reference;
- PR number, base, exact head, body, milestone, and campaign inventory record agree;
- the inventory record is current for that exact head and is not absent or malformed.

The normal implementation-owner closing-reference path remains unchanged for all other authors. The Dependabot extension is not an arbitrary app/bot role and cannot be used by a human or another bot. Keeping both source-specific identities explicit avoids either rejecting the real GitHub App representation or broadening admission to every App-authored pull request.

Merge authorization remains explicit and one-shot. It reads back exact author, campaign mapping, PR number/head/body/milestone, inventory record, review decision, unresolved threads, every protected check, current base, and branch synchronization. Only an explicit Sol dispatch for that exact head may authorize the existing `issueops-merge-authorized` context. There is no automatic merge or gate waiver. Any new commit invalidates the authorization and requires fresh proof/readback/dispatch.

#494's blanket rejection can be narrowed only after these positive invariants and negative tests exist; until then rejection remains the safe behavior.

### One trusted sequential audit complements hosted weekly updates

One manually dispatchable job stays in the existing trusted default-branch workflow and declares least job-level permissions. The CLI harness launches it with `gh workflow run`; it does not attempt a nonexistent hosted-update REST trigger or drive the GitHub UI.

The job parses only the current default-branch `.github/dependabot.yml`, validates its supported entries, and invokes the official Dependabot CLI sequentially once per entry. Later Luna implementation pins:

- `dependabot/cli` to v1.92.0 commit `13f626792737e1a2975e94feb8065874882616c4` or a newer Sol-approved official immutable replacement refreshed at implementation time; and
- every official updater and proxy image used by those entries to an OCI digest recorded in the workflow, never a mutable tag alone.

The proxy remains the secret/network isolation boundary documented by the official CLI. Audit input, logs, and the campaign update must not expose credentials. `clean` means no would-be update operations, `findings` means one or more bounded operations were reported, and `failed` covers invalid configuration, tooling/image/pin failure, API/rate failure, or incomplete execution. Every outcome and exact config/tool identity is ingested into the same campaign body region. `findings` create or refresh pending inventory items; `failed` blocks release; `clean` cannot silently finalize existing pending or provisional records.

One sequential job avoids duplicate runners, rate bursts, and cross-job aggregation. A second `workflow_run` handoff is unnecessary because the trusted default-branch job can hold narrow issues write permission at job scope and update/read back the campaign directly.

### The v0.5.0 campaign delivery order follows compatibility risk

After exact Rust #482 is accepted on `main`, campaign delivery is #453 (`object`, Rust/parser) then #454 (`rusqlite`, Rust/database/SQLite implementation and review) then #455 (`rmcp`, Rust/MCP). Each branch first refreshes or rebases onto accepted current `main`, resolves only its own new findings, and reruns the same affected-contract quality bar. Existing red checks caused by stale shared IssueOps state are recorded as stale-baseline evidence, not package regressions.

The sequence is intentionally local to these high-impact open updates. It does not make #497 a fake blocker of independent product work.

### Rust, storage, and resource pattern fit

Campaign automation introduces no Rust code. The Rust pattern-fit judgment is “no named Rust pattern needed”; the seven-crate product boundary stays unchanged. Database and SQLite are N/A for #498/#499 campaign storage because the issue body and release graph are the only authorities. The separate #454 implementation must load the Rust, database, and SQLite skill stack because it changes `rusqlite`, but this campaign design does not pre-decide that package migration.

Inventory reconciliation is linear in the bounded release-window PR record count. The audit is linear in `.github/dependabot.yml` entries and intentionally sequential, with one updater/proxy pair active at a time. No long-lived worker, queue, lock, local persistent bytes, or additional service exists.

## Risks / Trade-offs

- **A PR could be admitted under the wrong campaign** → bind exact author, milestone release graph, standalone relationship, PR/head/body, and inventory record in both implementation and merge contexts.
- **Automation could overwrite human issue prose** → mutate one delimited machine region with optimistic concurrency and read back the exact body.
- **A closed PR could disappear from release history** → retain every release-window record and update state instead of deleting rows.
- **A clean audit could be mistaken for campaign completion** → clean records the audit result only; every PR record still needs a final disposition.
- **Mutable container tags could drift** → pin the CLI source and every updater/proxy image by immutable commit/digest and fail on missing/mismatched pins.
- **Audit secrets or untrusted branch code could cross the boundary** → run only the trusted default-branch workflow, use the official proxy contract, declare least job-level permissions, and sanitize logs/body output.
- **Current red checks could be misdiagnosed as dependency breakage** → require refresh/rebase and fresh protected proof before technical disposition; label stale shared IssueOps failures as baseline drift.
- **Serial updates take longer** → accept the bounded sequence because object/parser, rusqlite/storage, and rmcp/MCP compatibility need distinct review; unrelated release lanes remain parallel.

## Migration Plan

1. Land #497 so both human and Dependabot pull requests have one affected-proof contract.
2. Implement #498's release-graph campaign declaration, exact-author IssueOps path, body-region reconciler, audit job, tests, and architecture readback while the blanket Dependabot merge rejection remains the default on any mismatch.
3. After #482 and #498 are accepted on `main`, publish/read back #499's native relationships and initial inventory, refresh/rebase #453/#454/#455, and deliver them in order.
4. Reconcile every window record and both pre-RC/pre-stable audits to final state before #499 closes; #492 performs final release readback and closes last.
5. Roll back automation by restoring blanket Dependabot merge rejection and manual campaign-region maintenance. The release remains blocked rather than bypassing the inventory.

## Open Questions

None. OCI digests are intentionally resolved from the official registry at #498 implementation time and checked in beside the immutable CLI pin so the workflow never depends on a mutable tag.
