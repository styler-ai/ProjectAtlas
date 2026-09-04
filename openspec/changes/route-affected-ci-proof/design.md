## Context

The ordinary `01-CI` workflow currently serializes an Ubuntu `verify` job and a
four-runner `e2e-smoke` matrix. Seven recent successful pull-request runs took
roughly 29-48 minutes from workflow creation to completion and 53-75 raw
runner-minutes in aggregate; the median Ubuntu verification job alone took
about 11 minutes. A one-line OpenSpec task update received the same five jobs
as a Rust change. Review submissions and review comments also start the entire
workflow, so a single unchanged pull-request head can receive two additional
full runs whose combined samples consumed roughly 109-126 raw runner-minutes.

The broad matrix contains valuable contracts, but not every contract can
detect a defect in every change. Cargo dependencies alone are not enough to
make that distinction: `projectatlas-cli` depends on five workspace crates,
and shared-core changes fan out broadly, while documentation, IssueOps,
installer, process, path, and operating-system contracts cross Cargo package
boundaries. Issue #487 provides responsibility-coherent CLI E2E binaries that
can be selected as domains; it does not itself route CI. Issue #366 permits
input-equivalent release proof reuse and remains the authority for that
behavior. Declined issue #497 is historical context only; this design is a new,
measured and narrower boundary.

The repository currently protects `verify` and all four platform job names.
The migration therefore has to keep those contexts green until branch
protection can safely move to stable aggregate contexts. The complete
installed-product four-platform matrix remains a release-candidate boundary.

## Goals / Non-Goals

**Goals:**

- Finish representative ordinary pull-request required checks in at most ten
  minutes where runner availability permits, with a hard design ceiling of
  fifteen minutes for legitimately broad affected proof.
- Reduce both required-check wall time and raw runner-minutes by omitting only
  proof contracts that cannot detect a defect in the exact change.
- Preserve every causal unit, integration, E2E, failure, compatibility,
  installer, packaging, security, and platform contract.
- Make every selection and omission reviewable in the Actions job summary and
  fail closed whenever the planner cannot prove a narrower selection.
- Keep full installed-product proof on Linux, Windows, macOS Intel, and macOS
  ARM at one integrated release-candidate boundary.

**Non-Goals:**

- Adding mutation, coverage, or nextest campaigns to normal pull-request CI.
- Adding build caching without a separate representative measurement that
  meets the repository's materiality threshold.
- Creating a new Rust crate, database, third-party change-detection action,
  generalized build graph, or durable proof ledger.
- Publishing commit hashes, task receipts, or per-task evidence records.
- Broadening the accepted v0.5 release scope beyond this CI acceleration work.

## Decisions

### 1. Separate live pull-request state from source verification

A small `pr-state` workflow SHALL own the live issue-reference and milestone
gates. GitHub's native required-conversation-resolution branch rule SHALL own
live review-thread state because Actions has no thread-resolution trigger and a
workflow result could therefore become stale. Review and review-comment events
SHALL NOT start `pr-state`, Rust, or platform jobs for an unchanged source tree.
The `pr-state` concurrency namespace is separate from source CI and automatic
cancellation is disabled, so metadata activity cannot cancel useful
compilation, tests, another state check, or an IssueOps run.

Title and body edits rerun only the lightweight workflow. A base-branch edit
also reruns source verification because it changes the exact base-to-head diff
without changing the pull-request head. The metadata-only source-workflow event
uses a unique non-cancelling namespace, skips every source job, and does not emit
a check named `verify`, so GitHub cannot treat a skipped aggregate as fresh
source proof. Owning issue close, reopen, milestone-assignment, and
milestone-removal events SHALL enter a trusted default-branch job in the same
`pr-state` workflow. That job reuses the existing issue-reference parser,
inspects only affected open pull requests, locates the existing workflow run for
each exact current PR head, and reruns it through the Actions API. The rerun
fetches live issue state through the existing validator and runs no source build
or test. This preserves one Actions-generated `pr-state` authority instead of
creating a synthetic check or expanding IssueOps permissions.

No workflow can change PR-head readiness if GitHub refuses the API calls needed
to enumerate that head or request its rerun. The issue-event job therefore
fails visibly on an unreadable inventory, missing run, timeout, or rejected
rerun and prints the exact failing operation. It does not claim that this
default-branch failure invalidates an older PR-head check. Final orchestrator
acceptance already rereads the live owning issue and matching milestone before
merge; it SHALL block merge and require a manual rerun after service recovery
when the refresh failed. This honest bounded recovery uses the existing final
acceptance authority instead of a synthetic check that has the same first-write
failure and creates a second mutable state owner.

This deliberately supersedes #299's Codex-only GraphQL polling gate. Native
conversation resolution applies to every review conversation, including human
threads; that stronger rule matches the repository requirement to disposition
all actionable review feedback and is the smallest gate whose state changes
immediately when a thread is resolved or reopened. The obsolete polling script
and its workflow step are removed rather than retained as duplicate authority.

The existing code workflow remains the source-verification owner and runs for
pull-request source changes, merge-group candidates when enabled, explicit
dispatch, and schedule. A merge to a protected branch does not rerun the same
accepted source tree: strict branch protection applies to administrators and
requires current source proof against the current base before either `main` or
`dev` can merge. `main` now requires `pr-state`, `verify`, and resolved
conversations; `dev` retains `verify` plus the four legacy platform contexts
until the task 4.3 migration is completed. Splitting the workflows is the
smallest safe option because it avoids a skipped `verify` check from a
review-only run being mistaken for source proof. Merely adding job-level
conditions in one workflow leaves that required-context ambiguity.

### 2. Cancel only an older source run for the same pull request

Automatic cancellation SHALL exist in exactly one namespace: source
verification triggered by `pull_request`. A newer source-verification run may
cancel an older source-verification run only when both carry the same
pull-request number. Different pull requests never share that namespace.

Every other event or workflow owner has a separate namespace with automatic
cancellation disabled:

| Owner or event | Namespace ownership | Automatic cancellation |
| --- | --- | --- |
| `pull_request` source verification | Source workflow plus pull-request number | A newer run may cancel only an older run for the same pull-request number |
| `pull_request` title/body edit | Source workflow plus unique run ID; no source jobs or `verify` context | Never |
| `merge_group` | Source-merge-group namespace | Never |
| `workflow_dispatch` and `schedule` | Source event-specific namespaces | Never |
| `pr-state` and IssueOps | Their own workflow namespaces | Never |
| Release, publish, and deploy | Their existing delivery namespaces | Never |

The namespace discriminator is deterministic from the workflow owner, event
class, and pull-request number when applicable. It never uses a shared ref-only
key that could cross these owners. This is narrower than a general superseded-
run policy and directly prevents expensive duplicate PR source work without
risking a release or governance operation.

### 3. Use one closed, fail-closed proof-contract planner

One Python-standard-library script SHALL read a NUL-safe base-to-head
name-status diff, bind the plan internally to the event's exact base and head,
and produce bounded JSON plus a human-readable Actions summary. It invokes
`cargo metadata` once to derive Rust reverse dependencies, then unions those
results with a checked-in, closed mapping from paths to non-Cargo proof
contracts. The planner returns proof-contract identifiers and job booleans,
not arbitrary shell commands.

The local pre-push hook consumes the same plan for the exact clean candidate
head and accepted `origin/main` base before running expensive proof. It maps the
fixed contract IDs to existing commands locally, just as hosted static jobs do,
so a documentation or owning-test change does not pay for unrelated workspace
or platform-neutral proof. Invalid push input or a plan that cannot be bound to
that candidate fails closed; it never guesses a narrower local command set.

The mapping uses the existing owners: repository policy, Rust compile/lint/doc
quality, crate unit and integration tests, the CLI E2E domains established by
#487, and platform-owned process, filesystem, path, installer, packaging, and
runtime behavior. Selection is based on the owning production and test
contract, not package names alone. A test-only change selects its owning test
domain. A production change selects its reverse dependents plus declared
cross-contract edges. Known renames and deletions union both old and new path
ownership.

Unknown paths, malformed diffs, metadata errors, missing ownership, stale
base/head bindings, and changes to shared support, CI workflows, the planner or
mapping, the Rust toolchain, the lockfile, workspace manifests, or schemas
select the complete normal-pull-request proof. This fixed seven-crate graph and
closed contract map keep planning complexity bounded by changed paths plus the
small workspace graph.

Alternatives rejected:

- Cargo reverse dependencies alone miss platform and cross-contract behavior.
- Hand-maintained path globs alone duplicate the Cargo graph and drift faster.
- A third-party planner or generalized graph engine adds supply-chain and
  maintenance cost without evidence that the bounded repository needs it.
- Mutation testing adds quality work and latency; it is not a speed mechanism.

### 4. Select proof by causal contract and platform ownership

The initial ownership rules are deliberately conservative:

| Change or contract class | Minimum ordinary pull-request proof |
| --- | --- |
| Documentation/OpenSpec with no executable-policy effect | Owning Markdown/OpenSpec/IssueOps/Mermaid checks; no Rust or platform build |
| Independent lint crate | Lint-crate compile, lint, and owning tests; no product platform E2E |
| CLI E2E test/domain | Owning post-#487 E2E binary/domain and its compile prerequisites |
| Production crate | Owning unit/integration targets, Cargo reverse dependents, and declared CLI/MCP/platform contract edges |
| Target-gated Rust in a platform-neutral path | Inspect bounded exact base/head blobs and compile the affected package closure on all matching supported targets; conjunctive, negated, unknown, or over-budget predicates select complete proof |
| OS-sensitive process, path, filesystem, watcher, installer, or packaging behavior | Owning Rust proof plus every operating system named by that contract |
| Architecture-sensitive macOS runtime or package behavior | Both macOS Intel and ARM owners |
| Shared core/support, workflow, toolchain, lockfile, manifest, schema, planner/map, or unknown input | Complete normal-pull-request proof |

"Complete normal-pull-request proof" means all repository quality and test
contracts that currently protect source changes, including every owning
platform contract. It does not mean repeating platform-neutral tests on four
runners when their ownership audit proves one runner is equivalent. The
implementation must inventory every current workflow step before moving it;
anything without a proven owner remains in the fail-closed set.

The complete installed-product command/MCP inventory and platform matrix is a
separate release-candidate contract. It always runs on Linux, Windows, macOS
Intel, and macOS ARM for the integrated candidate. If that boundary discovers
a defect, the fix returns to its owning issue and the complete boundary runs
again.

### 5. Run selected code proof concurrently behind one stable aggregate

After planning, selected quality, Rust test-domain, and platform jobs SHALL run
concurrently unless one consumes an actual output of another. The current
platform jobs consume no Ubuntu build artifact, so serializing all of them
behind an approximately eleven-minute Ubuntu job has no defect-detection
benefit.

Static job definitions receive planner booleans and fixed commands. A final
required `verify` job runs with `if: always()` and succeeds only when the plan
is valid and fresh, every selected job succeeded, and every omitted job was
explicitly marked not applicable by that plan. A selected job that is missing,
skipped, cancelled, or failed makes the aggregate fail. The independent
`pr-state` is the second stable required context. Native required conversation
resolution is the independent live review-thread condition.

Pull-request readiness is an explicit logical AND: the current `pr-state` and
the current `verify` for the same pull-request source input must both exist and
succeed, and GitHub must report every review conversation resolved. Either
context being absent, stale, skipped, cancelled, or failed, or any unresolved
conversation, keeps the pull request blocked. None can satisfy or replace
another.

That logical AND is the branch-protection boundary. Final merge acceptance also
rereads the live owning issue and milestone. A failed issue-event refresh is a
visible operational failure requiring recovery; it is never represented as a
new green PR-head result or treated as permission to merge on the older check.

This stable aggregate avoids a required-context name per possible plan. The
existing four `e2e-smoke` job names remain present during migration, and the
change's own pull request selects the full fallback so current branch
protection is satisfied before it changes.

### 6. Backstop classifier mistakes without repeating release work on every PR

The accepted pull-request or merge-group source proof remains authoritative as
that tree reaches the protected branch; the merge does not launch duplicate
source CI. Scheduled and manual drift checks run the complete normal-pull-request
proof. Merge-group events use an exact merge-group diff if supported; any
missing or ambiguous input selects the complete fallback. Human and
dependency-bot pull requests use the same planner and gates.

The release workflow remains the only complete installed-product
four-platform candidate boundary and may continue to reuse input-equivalent
proof under #366. No new cross-run proof ledger or public revision receipt is
introduced.

### 7. Accept only measured material improvement

The implementation SHALL record before/after workflow wall time, including
queue time, and raw runner-minutes for five representative change classes:
documentation-only, eligible independent leaf crate, CLI test/domain, shared
core, and platform-sensitive. If the current ownership graph has no genuinely
independent production leaf, that class is reported as not applicable instead
of manufacturing a source change solely to consume CI. Claimed routing is
retained only when representative measurements show at least a 30 percent and
30 second improvement without removing a causal contract.

Documentation-only, leaf-crate, and ordinary CLI-domain required checks target
ten minutes or less. Shared-core, platform-sensitive, and fail-closed runs have
a fifteen-minute design ceiling. If runner queueing or an indivisible causal
job prevents a target, the implementation records the measured cause and
keeps the proof; it does not omit a test to make the number green. The current
29-48 minute wall and 53-75 raw runner-minute ranges are the before baseline.
Raw runner-minutes below sum non-skipped source-workflow job durations; the
separate required `pr-state` workflow completed in 7-32 seconds and never
launched source work.

| Change class | Required-check wall after routing | Raw runner-minutes after routing | Disposition |
| --- | ---: | ---: | --- |
| Documentation-only | 0:48 | 0.97 | Retained: at least 97% less wall time and 98% fewer runner-minutes than the best baseline. |
| Eligible independent leaf crate | N/A | N/A | No independently owned production leaf exists: the apparent leaf is the repository-wide source-policy owner and correctly selects complete fallback. No synthetic run was created. |
| CLI test/domain | 2:49 | 3.07 | Retained: at least 90% less wall time and 94% fewer runner-minutes than the best baseline while running only its owning Rust, repository, and test-domain proof. |
| Shared core / complete fallback | 12:10 | 45.05 | Retained for safety: the identical complete plan used by shared-core, unknown, and workflow-authority changes is at least 58% faster in wall time and inside the 15-minute ceiling. Raw cost improved only 15% against the best baseline, so no material raw-cost claim is made. |
| Platform-sensitive | 3:16 to fail | 4.25 to fail | The narrow plan selected only Rust plus macOS Intel/ARM and correctly failed on a Bash 3.2 incompatibility. After the fix, the same macOS steps passed in 8:34 and 7:45 inside complete proof. No synthetic rerun was created solely to claim a green narrow duration. |

One earlier complete activation run reached 15:22 because its Windows owner took
15:00, exceeding the ceiling by 22 seconds; the unchanged complete-plan
representative above finished in 12:10. The variance is recorded rather than
removing proof. The measured narrow documentation and CLI routes materially
improve both latency and runner cost; the complete route materially improves
latency while preserving every causal backstop.

Caching is excluded because the available measurement validated a parser-pack
dependency cache on Linux but found an immaterial Windows improvement; it does
not establish a standard-CI cache win. It can be reconsidered independently
only after affected routing exposes a remaining measured hot path.

### 8. Keep the mapped issue task slice literal

For `--planned-issue`, mapped `tasks.md` is the implementation-task authority.
IssueOps SHALL compare the live issue's task text, order, and checkbox state,
not merely its task count or identifiers. Equal counts with changed text or
order fail exactly like checked-state or count drift. The existing parsed task
representation is sufficient; no second task store, hash, or receipt is added.

## Dependencies / Cross-Issue Impact

- #487 must land first because its responsibility-coherent CLI E2E binaries
  are the test-domain selection boundary used here.
- #366 remains the authority for input-equivalent release-proof reuse; this
  change neither duplicates nor weakens that contract.
- #341 supplies the measured-materiality rule, but its optional-parser cache is
  not extended into standard CI without new evidence.
- Declined #497 is historical context only and is not reopened.
- #555 is a direct `v0.5.0-00` child of release owner #492 and is blocked by
  completed #487; no other release dependency is added or changed.

## Risks / Trade-offs

- **A missing cross-contract edge could suppress a detecting test.** -> Keep a
  closed ownership map, self-test positive and negative cases, and make unknown
  or shared inputs select the complete fallback.
- **A skipped job could satisfy branch protection accidentally.** -> Require
  only `pr-state` and the explicit `verify` aggregate after a staged migration;
  the aggregate rejects selected skipped, cancelled, missing, failed, or stale
  jobs.
- **Review activity could cancel source proof.** -> Use separate workflows and
  deterministic namespaces, disable cancellation everywhere except newer
  source verification for the same pull-request number, and cover overlapping
  events deterministically and in hosted Actions.
- **Thread resolution could leave a stale workflow result.** -> Use GitHub's
  native required-conversation-resolution rule instead of polling or an
  Actions event that does not exist; verify both resolve and reopen behavior
  during the controlled branch-protection transition.
- **Owning issue state could change after a green PR check.** -> Let the existing
  trusted `pr-state` issue-event job rerun the existing workflow on each
  affected current PR head, and exercise close, reopen, milestone removal, and
  restoration without launching source proof. If GitHub refuses enumeration or
  rerun, keep the issue-event job red and require final acceptance to reread the
  live issue and milestone before manual recovery and merge.
- **The native rule broadens the former Codex-only gate to human threads.** ->
  Make that compatibility change explicit, remove the superseded custom gate,
  and prove unresolved and resolved human and Codex conversations in hosted
  branch protection.
- **Parallel execution spends runner time when an early quality check fails.**
  -> Prefer lower merge latency for valid changes, cancel superseded source
  runs, and measure runner-minutes; do not add speculative stage barriers.
- **Hosted runner queueing can exceed the target despite shorter jobs.** ->
  Measure creation-to-aggregate wall time separately from raw runner-minutes
  and record unavoidable queue limitations without weakening proof.
- **Branch-protection migration could create a merge gap.** -> Make the
  implementation PR fail closed and emit all current contexts, merge the code,
  then enable required conversation resolution and replace the four platform
  requirements with `pr-state` plus `verify` in one controlled transition and
  immediately exercise both narrow and fallback plans.
- **The planner becomes another build system.** -> Keep fixed contract IDs,
  standard-library parsing, one Cargo metadata graph, no command generation,
  and no plugin interface.
- **Equal-count issue tasks could drift in text or order.** -> Compare the live
  parsed task slice literally with mapped `tasks.md` and self-test equal-count
  text, order, and checkbox-state mismatches.

## Migration Plan

1. Inventory each existing `ci.yml` step and assign it to one closed proof
   contract; unresolved ownership remains full fallback.
2. Add the planner, bounded report, and self-tests before wiring omission.
3. Move live issue/reference checks to the separate `pr-state` workflow, refresh
   exact PR-head state from owning issue close/reopen/milestone events through
   an isolated job in that workflow, and use native required conversation
   resolution for review threads.
4. Give each event and workflow owner its separate cancellation namespace;
   enable cancellation only for an older PR source run superseded by a newer
   source run for the same pull-request number.
5. Refactor source CI into planner, static concurrent jobs, and final `verify`
   aggregate while forcing the implementation pull request to full fallback.
6. Prove positive, negative, failure, compatibility, bot, rename/delete, stale
   binding, three-input readiness, visible issue-refresh failure and manual
   recovery, same-PR cancellation, and cross-owner isolation locally and in
   hosted Actions.
7. Extend planned-issue IssueOps self-tests with equal-count task-text and order
   drift before relying on the issue/task mirror for handoff.
8. After the implementation is merged, change branch protection from the four
   platform contexts to `pr-state` and `verify` while enabling required
   conversation resolution; immediately exercise thread resolve/reopen, a
   owning-issue close/reopen/milestone transitions, a narrow known plan, and an
   unknown-input fallback. Roll back by restoring the former required contexts
   and making every plan select complete proof.
9. Measure all five before/after classes. Remove any narrowing rule that does
   not meet materiality or causal-coverage requirements.
10. Confirm the next integrated release candidate still runs the complete
   installed-product four-platform boundary and restarts it after any fix.

## Open Questions

None.
