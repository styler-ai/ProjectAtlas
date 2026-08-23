## Why

Weekly hosted Dependabot correctly produces update pull requests, but ProjectAtlas lacks one release-owned inventory that admits every relevant update, records a final disposition, and blocks release while any record remains pending or provisional. v0.5.0 needs one lean campaign and one bounded on-demand audit, not an issue per pull request or another dependency service.

## What Changes

- Keep weekly GitHub-hosted Dependabot as the only pull-request producer driven by `.github/dependabot.yml`.
- Add exactly one campaign issue per release/version with one machine-owned inventory of the union of all currently open Dependabot pull requests and every Dependabot pull request created, updated, closed, or merged during the release window.
- Require a final `accepted`, `deferred`, `declined`, or `superseded` disposition for every inventory record; `pending` and `provisional` block release.
- Make #498 own intake, exact campaign relationships, inventory/disposition reconciliation, the smallest Dependabot-only IssueOps admission/merge-authorization extension, and one sequential manually dispatchable dependency audit.
- Make #499 own the v0.5.0 campaign, including live refresh of open #453/#454/#455 and retrospective provisional records for merged #457/#468, with no dependency-regression claim from the currently stale branch-global IssueOps failures.
- Require every Dependabot pull request to carry exactly one explicit non-closing relationship to the active campaign issue, the release milestone, normal affected-contract CI, review/thread/check readback, exact author/PR/head/body/inventory identity, and explicit Sol merge dispatch. No update auto-merges or bypasses the protected contexts.
- Drive the bounded pre-RC and pre-stable audit solely from `.github/dependabot.yml` through one trusted default-branch `workflow_dispatch`, launchable with `gh workflow run`. Pin the official Dependabot CLI and its updater/proxy container contracts; emit only `clean`, `findings`, or `failed` into the same campaign inventory.
- Keep CI selection owned by `verify-affected-repository-contracts`; campaign automation consumes its result and does not duplicate its planner.

Non-goals:

- Per-Dependabot-PR issues, a second issue graph, database, service, dependency model, updater, or inventory store.
- A nonexistent REST trigger for GitHub-hosted version updates, browser/UI automation, or direct reuse of `github/dependabot-action` in a repository workflow.
- Automatic merge, a broad bot exemption, weaker review/check gates, or release with pending/provisional records.
- Parallel audit fan-out, arbitrary configuration beyond `.github/dependabot.yml`, or another `workflow_run` handoff framework.

This specification remains candidate-only backlog planning until the planning pull request lands and its exact published evidence is read back. #498 becomes implementable after #497 is accepted on `main`; #499 waits for accepted #498 and exact Rust #482 on `main`.

## Capabilities

### New Capabilities

- `dependabot-release-campaigns`: one release-scoped Dependabot inventory, exact IssueOps ownership, bounded on-demand audits, final dispositions, and release reconciliation.

### Modified Capabilities

None.

## Impact

- Future Luna implementation is limited to existing GitHub workflows, IssueOps/repository policy scripts and tests, `.github/dependabot.yml`-driven audit translation, and campaign issue reconciliation.
- The dependency manifests and lockfile change only in their normal Dependabot pull requests; this capability adds no product runtime, Rust crate, SQLite state, package manager, or service.
- The v0.5.0 release graph gains #498 and #499, with #498 blocked by #497 and #499 blocked by #498 and #482; #492 remains the feature-free release root and closes last.
