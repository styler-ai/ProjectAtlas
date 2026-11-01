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
Current source/spec/docs/memory changes are primary-owned and ready to commit.
Publication freshness and fixture-cycle fixes below are included, not deferred.
No #739 PR yet. No build, index, benchmark or mutation process remains live.

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

Finish cheap gate readback, commit intended files normally, push this branch with
normal hooks, and create a draft PR to main. Expected push log is
`/tmp/projectatlas-739-native-push.log`; record actual returned session/PID externally
and in integration memory after start. Completed47956: upgrades+CLIcost PASS;
completed42582: currentoptimizedpublicE2Es PASS. Do not infer a live process from a planned
path, receipt or old header. Keep source clean during the normal push gate.
Complete whole-issue Luna Max review, native/hosted four-platform proof and every
current bot/security/review finding, then finish 2.2/3.1 and acceptance, normally
merge and verify native issue closure before #738. No force/admin/bypass flow.

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
