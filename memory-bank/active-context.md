## Current public #739 checkpoint — PR #745

Public source is 5560e43045d9bdb2b66a6d1043dcce48956500f0 on
`fix/external-excludes-739`, based on main 6927ed0. PR #745 is OPEN and ready,
milestone 37 (`v0.5.1-00`), references only #739, with no auto-merge.
Implementation remains 2/4: 1.1 and 2.1 accepted; 2.2, 3.1 and acceptance open.

Normal push 58835 finished successfully: 1,770 passed, 0 failed, 12 existing
ignored tests across 34 suites, plus required repository/Rust/docs/security/Atlas
gates. `/tmp/projectatlas-739-native-push-session.json` is terminal, not live.

Exact-source CI 41340135380: plan, repository, Rust, Windows diagnostics and Linux
passed; Windows and both macOS jobs were still running at the last read.
CodeQL workflow 41340131537 succeeded. Codex code and security reviews completed
on 5560 (summary 6562604562), no Codex findings posted, bot thumbs-up. Security
summary finding flags are null; do not turn that into a clean-scan claim.
Latest metadata PR-state 41340706944 passed; original missing-milestone failure
is superseded. Do not restart passing jobs for metadata.

All seven new GHAS threads (410, 404–409) were individually statically assessed
and independently accepted by Luna Max review_731, then replied to and resolved
with readback. Six are private nonshipping libtest fixture paths; 409 propagates
aggregate taint to a root actually assigned from the selected plan, with document
prefix admission before ignore classification. No source restrictions, alert
suppression or dismissal. Evidence: `/tmp/projectatlas-739-codeql-triage-result.json`
and `codeql-intake/normalized-seven-sarif.json`. These seven dispositions do not
claim all 11 Rust SARIF results are clean or replace the older #741 baseline.

A platform coverage gap was confirmed: existing jobs compile the two new public
#739 scenarios but do not explicitly execute them. Primary added one bounded
`External source policy and root replacement` step in `.github/workflows/ci.yml`,
under the existing compile contract, including all four full-plan platforms.
It runs FS lib, policy_watch/source_observation unit families and the exact real
external-policy/root-replacement CLI/MCP selectors. YAML parse, diff check and
planner self-test passed. Exact extracted step session55986 finished PASS: FS46,
policy-watch10, source-observation36, and both exact public E2Es. Log:
`/tmp/projectatlas-739-platform-step.log`. Luna Max review_741 approved the bounded
step and all-four compile-matrix selection; final hosted outcomes remain required.
No push of this workflow correction yet. Preserve source clean during push.

Stable050 exact-root MCP refresh index-2 completed in about 42 seconds, without
reset; subsequent selected-root brief reports available. One bounded low-scope
`.agents/plugins` purpose row was approved by purpose_739 (one change, no conflict);
no index or mutation lease remains.
Current candidate artifact 9c7224c6... and all 139 source inputs remain unchanged;
workflow proof must be rerun for the correction, while unchanged cost/upgrade/
causal artifact evidence remains reusable only for its stated source boundary.

Next: accept and normally push the workflow correction, verify the new exact-head
four-platform outcomes and every current review comment, complete task/acceptance
mirrors only after proof, merge #745 normally and verify #739 closed before #738.
Current remaining milestone issues: #739, #738, #708, #715. No RC exists yet.
Stable050 remains Latest; stable publication requires later user authorization.

# Active RC1 context — 2026-11-01

## Goal and ownership

Continue until v0.5.1-rc1 is a non-draft prerelease with Latest=false and its
packages are independently downloaded, installed and verified. Stable 0.5.0 stays
Latest. Stable publication and milestone closure require a later user instruction.
Finish, normally merge and verify closure of one issue before starting the next.
Current order: #739, then #738, #708, and final release acceptance #715.
The primary writes code; independent reviews use Luna Max and remain read-only.

## Current #739 checkout and tracker

Exact checkout `.worktrees/issue-739`, branch `fix/external-excludes-739`.
Base aaef42a0e431c3549ecdacca9c64459c07197384 merges accepted main 6927ed0.
Use git HEAD for the subsequent normal checkpoint; do not infer it from this base.
Earlier root-watch checkpoint 832cdd5 and original backup pointer
`/tmp/projectatlas-739-pre-main-snapshot-path.txt` preserve prior work.
Source/spec/docs through publication freshness are committed in 5560; public PR #745
is open. The header above owns the current workflow correction and process state.

OpenSpec `reconcile-external-git-excludes`, schema spec-driven, owns #739.
Implementation tasks 1.1 and 2.1 are checked locally and remotely (2/4); both 2.1
mitigations are checked. Tasks 2.2, 3.1 and all five acceptance rows remain open.
Fresh strict OpenSpec and planned IssueOps passed: local4/remote4/checked2.
Use existing cached CLI `~/.npm/_npx/21a1280729add0a7/node_modules/.bin/openspec`;
bare lookup/npx-no-install failed. Status/instructions receipts are
`/tmp/projectatlas-739-openspec-{status,apply}.json`; read all four returned paths.

## Implementation and confirmed root causes

Canonical gix-config 0.62.2 (defaults off, sha1) and ignore 0.4.34 share bounded
external system/global config selection, configured paths, shared Git excludes,
inherited local policy, ignored-parent pruning and full/targeted behavior. Reads
are bounded to 1 MiB/file and 4 MiB aggregate; implicit Git includes are unsupported.
Preserve explicit operator root/config/DB paths and ordinary no-Git workflows.
Static nonregular ignore/config inputs fail promptly; regular symlinks and pruned
subtrees retain compatibility. Do not claim universal hostile-filesystem races.

Both CLI and MCP observe bounded selected external parents and retargets. Cold
reconciliation rebinds replaced parents and source roots; warm reads do not rewatch
unconditionally. Callbacks have no policy I/O, preserve bounded queue/continuity,
and filter only proven unrelated external siblings. Root rebind uncertainty is
published under the same ingress receiver fence as the event and sequence.

The policy-only document probe pages minimal unresolved inputs (256), stops on the
first mismatch, and invokes existing atomic projection while reusing source parses.
It does not invent a complete affected closure or retain a cross-page target cache.
The shared uncached document inspector now applies #738's accepted native-prefix
admission before per-target ignore reads, covering cached projection, staged
revalidation and this new probe. All carry the original operation control. The map
budget check remains before allocation. Publication now rediscovers policy from
the original plan options before checking retained target states; it never reuses
the staged matcher. The obsolete staged-policy field/clones were removed. #738
still requires its own complete
acceptance and closure; its parked checkout was not edited.

Current extraction derivation is 25; semantic contract 23 is retained. Refreshed
`/tmp/projectatlas-739-private-contract-inventory.json` shows every other checkout
at derivation <=24. Preserve all approved purposes and identities; never reset DBs.

## Verified current evidence

- Root/public and ingress causal controls: disabling the fix fails as intended;
  fixed positives pass and source is restored byte-exactly. Receipts:
  `/tmp/projectatlas-739-{root-public,ingress}-causal-control.json`.
  MCP child-event proof uses relation reads only after the edit; repeated explicit
  watch_once formerly masked broken coverage and was removed.
- Shared document guard: old direct probe fails Ignored vs OutsideRoot; current
  document suite passes 33 tests, including cached/uncached/revalidation cancellation,
  escape/reentry and persisted-probe outside-policy invariance/no-op publication.
  Logs `/tmp/projectatlas-739-document-probes-{negative,positive}.log`.
- Current FS46, policy-watch10, source-observation36, inventory3 (223 selectors),
  all-target/all-feature CLI Clippy, formatting, strict strings and cargo-deny pass.
  Strict strings and inventory were rerun successfully in73927 after fixture
  constants/pins and portable memory corrections. No exemptions or bypasses.
- Real external-policy and root-replacement CLI/MCP tests pass against the frozen
  optimized candidate (1.33s/8.08s), logs
  `/tmp/projectatlas-739-optimized-{external-policy,root-replacement}-e2e.log`.
- `/tmp/projectatlas-739-upgrade-proof.py` and JSON pass all four stable050/main24
  x ordinary/linked-worktree cases: causal repair, every approved purpose including
  dormant excluded files, identity/revision, no-op, malformed failure, early deadline,
  policy-only/full convergence, rollback to the predecessor view and re-upgrade.
  Early deadline is not mid-publication cancellation; owning controlled units cover
  cancellation. MCP public proof is separate and must not be overstated.
- Publication race: deterministic old-owner interleaving FAIL101, fresh-policy
  fix PASS0.22s, last-valid publication/identity, retry and no-op. Logs
  `/tmp/projectatlas-739-policy-publication-{negative,positive}.log`.
- Root fixture uses budget8 and sequential acknowledged completion edits after
  required child proof, not a three-logical-phase assumption about native batches.
  Current causal77496 terminalPASS: CLI missed edited_target, MCP missed child,
  exact source restoration and fixed positive0. No mutation lease remains.
- Luna Max review_741 approves root/callback/public-proof and shared-document slices,
  and the final fresh-policy publication correction after its whole-source finding.
  review_731 accepts task2.1 dependency/resource evidence and upgrade proof limits.
  Whole-issue implementation/acceptance review and platform proof remain pending.

## Frozen costs and proof reuse

Candidate normal default release build: CLI SHA
9c7224c6ba9219325ad3c023bfc153259cc30100a556c1e4dd148a6b933d6335 at
`~/.cache/projectatlas-benchmarks/external-policy-739-publication-release/projectatlas`.
Its 139-input source manifest SHA is
44f2c964c790070a55d3ad1a9f48d6a02fa8499566606b92559cb1a4538d9730.
All current source hashes match. The accepted-main optimized baseline is in
`~/.cache/projectatlas-benchmarks/external-policy-739-main-6927ed0-release`;
its CLI SHA is adfe95f1ae05c4a3bbbf1776a460e754d6fc016468d51238558611218b0e2b3c.
Both use matching default release settings. Baseline micro helper was relinked
from its original unique libraries and identical source; SHA matches measured
bytes exactly. Both receipts bind the micro source and binaries.

Durable `docs/benchmarks/external-policy-cost.{md,json}` records 18 alternating
samples, construction/full/targeted/RSS, binary +345488 bytes (+0.497%), limitations
and upgrade summaries, plus18 completeCLI init/unchanged-refresh samples with one
absentdocumenttarget. CLI large-rule unchangedrefresh rises71.6->97.9ms; preserve
that26ms cost for708. Currentnativeupgrade4cases reran on9c; earlierff57 is
historical and not claimedcurrent. UnchangedFS microhelpera03a01b9... was relinked
byte-identically, so only those component samples are reused. Driver `/tmp/projectatlas-739-policy-cost-proof.py` and JSON
are terminal PASS. Workload is 513 source files, nonmatching rules, warm caches,
three trials. Ordinary targeted overhead rises; 10k-rule targeted work falls from
17.87 to2.50ms. This is no general speedup, #708 SLO waiver or #715 platform proof.

## Next authorized steps

Finish the current workflow proof and review, then push normally with hooks.
Complete whole-issue Luna Max review, exact-head four-platform proof and all current
bot/security/review feedback. Finish 2.2/3.1 and acceptance only after proof, normally
merge #745 and verify native issue closure before #738. No force/admin/bypass flow.

Installed stable Atlas0.5.0 direct runtime and exact-root MCP work. Same private
DB was refreshed incrementally twice without reset; last completed task index-1
restored an available brief. New source edits may require another typed refresh.
Follow exact-root navigation and freshness guidance; known closure-limit recovery
is not automatically a new defect. Preserve approved purpose maintenance.

## Accepted closures and parked work

#731/PR737 are closed/merged. #741/PR743 closed normally at accepted commit
6927ed0ca64788db6947b4d43eafce29aac5ed61 (2026-11-01T16:32:45Z). Its reviewed
5dda tree equals merged tree49893e53b41eb98bd3e641a200dab26d9981e941.
Closure receipt `/tmp/projectatlas-741-closure.json`; final CI41321777233 all9jobs
passed, all45 review threads resolved, current code/security reviews succeeded.
CodeQL27Rust+4Actions match the dispositioned baseline exactly; the advisory red
31 is not a clean-scan claim. Do not reopen741 or make new741 source commits.

Parked #738 source8814d6d plus memory dirt is preserved; integrate739's shared guard
without losing its accepted spec, positive platform/upgrade proof or final contract.
#708 PR728 publicc65dfed/local587ecbe plus memory dirt is preserved. Warm1201.127
failed; focused1.031526 is partial, no SLO waiver. Accepted-main global IssueOps has
known #708 task2.3 live checked/main unchecked drift. Reconcile when708 is accepted;
do not falsely uncheck private progress. Candidate739 scoped gate excludes it;
final715 global gate must pass. Integration release/0.5.1 retains private715 spec
changes; do not overwrite them. Read its own source/spec before final integration.
Queued Codex CI workflow gap review remains at
`/tmp/projectatlas-codex-ci-followup.json`; assess within the appropriate owner,
not as permission to start a parallel implementation issue. No RC exists yet.
