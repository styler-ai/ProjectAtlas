# Active context

## Objective and working rules

Continue autonomously until `v0.5.1-rc1` is published and independently verified.
Keep the preceding stable release as GitHub Latest; stable promotion needs a later
user instruction. Use `.worktrees/`, private checkout indexes, the installed stable
0.5.0 Codex plugin (CLI preferred), and incremental Atlas refresh. Never initialize
existing state or index home/config directories to orient a task.

The primary agent implements. Independent GPT-6 Luna max reviews explicitly load
our custom Rust and applicable database/SQLite skills and accepted specification.
Keep calls bounded, reuse passing evidence, and communicate progress. Index only
required/invalidated work; retain publication race safety. Codex idle-sleep prevention
is configured globally; a running app must load that setting before protection is claimed.

## Release scope and integration

- Baseline accepted `main`: `7cd5a79`, stable product version 0.5.0.
- Integration checkout `.worktrees/v0.5.1`, branch `release/0.5.1`; its entrypoint
  memory points here. It has no product implementation yet.
- #708: complete indexing/refresh performance and adaptive cooperative budgets,
  OpenSpec `accelerate-incremental-refresh`, ready/high, milestone `v0.5.1-00`.
- #721: bounded relation navigation, OpenSpec `bound-relation-navigation`,
  ready/high in the same milestone; reviewed local plan in `.worktrees/issue-721`.
  Live parent is #715 and #715 blockers are #708/#718/#721; #721 has no blocker.
- #718 is live-verified closed. #715 owns release acceptance/publication and
  closes last after stable verification. RC publication admits that owner open.
- #721's local issue-map includes the new release membership. Merge that plan
  before global/release IssueOps; main still has its preceding mapping.

## #708 checkpoint

Checkout `.worktrees/issue-708`, branch `perf/indexing-708`. PR #723 targets main,
head `7f2d407` before the current fallback review fix. It references #708 without
closing it and is ready for review, milestone assigned. Hosted repository and
PR-state checks pass; Rust and four-platform smoke checks are still running.

Published slices implement ordered source-only comparison, exact sparse updates,
operation-owned source handoff fenced by root/contract/database instance/generation,
borrowed hash-verified raw UTF-8 text for built-in parsing, and atomic full graph
publication with existing derived-index deferral. Final exact source validation
remains. MCP warm no-op reuses verified observation without repair and transfers a
real delta to the shared mutable owner. Watch refuses absent/empty database state;
storage-owned ExistingSchema admission still permits supported migrations.

Luna's holistic review found captured full-fallback discovery was retained while
another full inventory was scanned. The current fix moves that inventory into the
full staging owner, rechecks identity/generation, and retains final validation.
Its causal regression passes: a source added after capture must cause refusal;
a fresh retry includes all source while identity, purpose and publication survive.
The final assertion specifically requires the late path's typed refresh report.
Workspace Clippy and independent Luna fix review pass. The normal push hook must
verify this updated candidate before merge; do not merge the older PR head.

Tasks 1.1/2.1/2.2 are checked and synchronized locally/live. Tasks 2.3 onward and all
five acceptance tasks remain unchecked. No claim of complete #708 acceptance.

## Adaptive-budget follow-up

Local branch `perf/indexing-budgets-708` exists at `7f2d407`. Its owned edits are
safely stashed as `owned adaptive-budget follow-up while fixing PR723`. Restore
only after the fallback fix is committed and incorporated into that branch.

The private runtime policy uses existing source inventories/counters, no filesystem
walk or core/schema change. Unknown initial work keeps 1800 seconds; known work
gets the documented source/verification estimate; verified warm no-op gets five
seconds. Explicit limits bypass selection. All bounds derive from the original
operation start, preserve parent limits/cancellation, and continue through final
validation/publication via a duration in the existing publication batch. Wider
fallback uses the original same-clock parent, never a fresh clock or smaller delta
bound. Coefficients are in the draft indexing benchmark document; Luna policy
review is pending. Two estimator/explicit-limit tests and the publication-bound
regression passed before stashing. The fallback fix will require a small conflict
resolution when restoring the draft runtime edits. Task 2.5 is not complete.

## Proof and measurements

The native pre-push hook at head `7f2d407` completed exit 0, including locked Rust
checks/Clippy/tests/docs, PDF parser source validation, scoped live IssueOps and
source-policy lint. The final nonsource list is advisory: lint reports ok true.
Earlier full workspace runs and affected recoveries passed all native targets,
including 238 DB tests, 351 CLI main tests, strict strings and warning-clean rustdoc.
Conditional artifact/platform ignored cases remain release-owned installed proof.
Logs are `/tmp/projectatlas-issue708-*.log`. Reuse unchanged passing proof; the
normal hook remains mandatory for changed pushes.

Preregistered Linux reference: baseline initial/full/rebuild medians
5.257/6.928/6.589 seconds; first optimized slice 3.770/5.373/5.039 seconds.
Sparse edit 6.606 -> 0.144 seconds, cold no-op 46.9 -> 43.8 ms, RSS targets met.
These are partial reference results, not the final larger-fixture/four-platform
matrix. Current source-reuse changes still need fresh optimized measurements.
Protocol/targets: `docs/benchmarks/v0.5.1-indexing.md`; local results in
`/tmp/projectatlas-*reference.json`, fixture under the dedicated benchmark cache.

## #721 checkpoint

`.worktrees/issue-721`, `perf/relations-721`: no product code yet. OpenSpec plan and
local map changed; strict OpenSpec and scoped IssueOps pass (5 tasks, 1 checked).
Luna accepted the specification after clarifying legacy omitted rows 50 and
unchanged analysis defaults 50 rows/256 KiB. Default relation view becomes detailed,
exact-file anchored, 10 rows/32 KiB. Explicit legacy permits complete small arrays
only, 1..200 rows, bounded output/SQLite VM time/intermediate bytes, with typed
narrowing guidance instead of partial arrays. Reuse existing detailed traversal.
Read its full proposal/design/spec/tasks before coding. Its memory still needs the
latest live-ready/parent/blocker update. Rebase off accepted main before publishing:
its old memory-only #715 base must not contaminate a #721 candidate's sole ownership.

## Tooling and next steps

Pinned Rust 1.98.0, standard rustup proxies, wasm32-wasip1 and CI-pinned cargo-deny
0.20.2 are installed. gh 2.103.2 is selected and authenticated as styler-ai through
the OS keyring; older distro gh lacks IssueOps `--slurp`. Local core.hooksPath is
`.githooks`. Stable Atlas runtime/plugin/registry repair preserved the issue DB.

Use filesystem-backed `$HOME/.cache/projectatlas-test-tmp` as TMPDIR (plain /tmp
is unsupported tmpfs for production WAL). Shared CARGO_TARGET_DIR is the integration
checkout's target. Installer fixtures require a clean PATH with rustup proxies,
the task gh-only helper directory and existing Python venv; keep real Atlas out.
Do not compile sibling worktrees into that shared target during CLI E2E tests.

Next: finish fallback review/gates and update PR #723; restore/complete adaptive
budgets; implement #721; measure/review/integrate complete work; complete #708's
installed four-platform acceptance; prepare #715 RC version/pins and publication
workflow; verify tag/assets/checksums/install behavior/prerelease/Latest independently.
No RC exists yet. Keep the goal active.
