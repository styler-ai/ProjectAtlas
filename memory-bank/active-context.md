# Active RC1 context — 2026-11-01

Finish, normally merge and verify closure of one issue before the next:
#739 → #738 → #708 → release acceptance #715 → v0.5.1-rc1 non-draft prerelease,
Latest=false, independently downloaded/installed/verified. Stable050 stays Latest;
stable publication and milestone closure need later instruction. Primary codes;
independent Luna Max reviews are read-only. Preserve parked work and private DBs.

## Current #739 checkpoint

Exact checkout `.worktrees/issue-739`, branch `fix/external-excludes-739`.
Public PR745 OPEN/READY, soleowner739 (Refs, not Closes), base main6927ed0,
head f8a8fa8ed6d4aa8256c82d114fa9f57f948c2572. Local repaired source, cost docs,
design and memory ready for normal commit/push; read git for next actual HEAD.
No auto-merge, force, admin or hook bypass. Normal pre-push hook is mandatory.

OpenSpec reconcile-external-git-excludes, spec-driven, owns issue739. Only1.1 is
checked (1/4);2.1 and mitigations were reopened after supported-platform failures.
2.2/3.1 and all five acceptance rows remain open. Luna741 final changed-boundary
review found no new blocker and approved next native/hosted gates; keep2.1 open
until repaired current-source native and four-platform proof. Luna731 approved
bounded DER25 and Windows fixture composition, without claiming nativeWindows proof.
Strict OpenSpec, fmt, strict-strings, planned IssueOps local4/remote4/checked1 pass.
CLI: ~/.npm/_npx/21a1280729add0a7/node_modules/.bin/openspec.
Current status/apply JSON: /tmp/projectatlas-739-current-openspec-{status,apply}.json.

Old CI41344940865 TERMINALFAILURE; watcher50183 TERMINAL1; never restart oldsource.
Linux124962832387 passed fulljob/FS46/policy10/observer36/public2. BothMacjobs
124962832480/124962832454 failed FS44PASS2FAIL (root alias/FIFO bypass and APFS
invalid-byte fixture EILSEQ). Windows124962832471 FS40PASS then policy7PASS1FAIL
(verbatim PathBuf::join normalized away the fixture's intermediate/.. component).
Logs: /tmp/projectatlas-739-f8a8-{linux,macos-arm64,macos-x64,windows}{,-policy-step}.log.
Previous normalpush93555 passed1770/0fail/12ignored/34suites, but attests f8 only.

## Repairs and owning boundaries

RootScanPolicy stores an optional selected absolute lexical root when distinct
from canonical. Relative selection captures CWD once; drive-relative native fallback
preserved; empty root rejects and '.' remains valid. Component-wise rebase matching
absolute targets before both Atlas/native ignores, without target canonicalization
or target I/O. Canonical/relative/unrelated paths retain handling. Full Node vectors
and canonical plan/persisted root are unchanged, DER25/semantic23 remain sufficient.
Causal oldsource0PASS2FAIL; fixedFS49PASS. Root spelling test always3 selections under
capturedCWD, distinct Windowsalias assertion, existing/absent targets/fullrow equality.
Unix symlinkalias test proves FIFO refusal, pruning and outside prefix-sibling isolation.

APFS fixture falls back ONLY on direct EILSEQ to real Unicode Git/worktree lifecycle;
raw-byte roundtrip retained where representable and typed NonUtf8Path always tested.
Windows changes are TEST-ONLY: raw OsString retains ParentDir; separately assert
verbatim-join normalized control accepts actualrules/rejects unrelatedintermediate.
Isolated actual gix config producer test retains absolute parent components. No new
callback I/O or production filter change. HostedWindows remains native authority.

Existing repair: canonical gix-config0.62.2 defaults-off/sha1 and ignore0.4.34,
1MiB/file4MiBaggregate direct config reads, declared precedence/no implicit includes;
no-Git/inherited rules/pruning/operator paths retained. Bounded external coverage,
alias retarget, cold parent/source-root rebinding, ingress fence, no warmrewatch;
queue1024/registration256/convergence3. Policy-only unresolved probe pages256 and
reuses unchanged parses in atomic projection. Shared #738 native prefix guard at
uncached document inspector covers cached/probe/staging consumers under original
control; #738 still owns fullacceptance. Publication rediscovers policy from original
plan options before transaction; no staged matcher reuse/schema/authored-state change.

## Current artifact and affected proof

Frozen candidate ~/.cache/projectatlas-benchmarks/external-policy-739-native-spelling-release:
CLI10c3f448a999b47e22153979379b8f9511712c9f93f7f6a16e0c8fc982fff9cf,
69,805,624bytes;139inputmanifest884c65c1a1986dfb7075fc917fc3903a6a9ee7d3993462a80cea58599c17f8bc.
Current exact saved139source hashes and frozenbytes independently match.
Helper68663dbeaf98788375eb33213c0b472b4e0fcc3c3bd5da176089e79113aebb72
relinked byte-identically to newly measured root-alias helper. OnlyFS18measurements
reused; CLI18samples/upgrades4/public2 reran on10c. Micro sourcebe8b7ab61c0230a7...
Receipts copied some historical provenance: correction at
/tmp/projectatlas-739-native-spelling-provenance.json explicitly records current
source/artifacts/buildlogmtime; do not claim original built_at/dirty_paths are current.
Normal locked default release build, policy10/0, CLI alltarget/allfeatureClippy PASS.
Final public2PASS (1.27s/8.08s), upgrade4/4 and CLIcost18passedtrue, outputs each
inspected: /tmp/projectatlas-739-native-spelling-{external-policy-e2e,
root-replacement-e2e,upgrade-proof,cli-cost-proof}.*. No proof/build process live now.

Durable docs/benchmarks/external-policy-cost.{md,json} regenerated and Luna741 checked:
18FS +18CLI samples,4upgrades, identities and limitations. Baseline acceptedmain
adfe95f1ae05c4a3bbbf1776a460e754d6fc016468d51238558611218b0e2b3c;
stable050aa6188e11a05c54f4a69680e74055a02bdae4db7503aa0144d5e7157316ab485.
Binary+348664bytes/+0.502%. Ordinary CLIinit293.0→292.1ms/noop48.6→48.9ms;
10krulesnoop70.3→97.9ms (+27.6ms). Componenttarget18.03→2.52ms is no general
speedup/#708SLOwaiver. Warmcanonicalroot fixtures do not quantify aliasbranch costs.
Upgradeearlydeadline is not midpublication cancellation; controlledtests cover that.
Historical9c/bd86 artifacts immutable, not current CLI evidence.

Unchanged causal mechanisms: /tmp/projectatlas-739-{root-public,ingress}-causal-control.json;
publication /tmp/projectatlas-739-policy-publication-{negative,positive}.log;
shared document /tmp/projectatlas-739-document-probes-{negative,positive}.log (33units).
Old fixdisabled controls fail; source restored byte-exactly. Root child-event proof
uses relation reads after required childedit (no explicitwatch masking coverage).
Inventory223selectors; current normalnative hook reruns affected fullscope.

## Current security/review and next steps

Pushedf8 CodeQL41344938393 SUCCESS: Rust2072779166 11results, Actions2072728208
alert411, other5languages0. /tmp/projectatlas-739-f8a8-rust-reconciliation.json
preserves full facts/codeflows/related multisets, normalizes only numericrefs and
relatedlocation presentationorder/ids; Luna731 independently confirmed. All7inline
threads individually dispositioned/resolved/readback. Actions411 primary+Luna741
not_actionable eventref/cacheboundary triage posted/readbackcomment6563449602.
Codex code/securityf8 completed, codeflagsfalse/securityflagsnull, no newfindings.
Not a cleanscan/suppression/dismissal. Reinspect next-source scans and ALL reviews;
do not transfer counts/approvals blindly across code changes.

Next: normalcommit/push with hooks; capture livehandle/PID and update INTEGRATION
memory only while gate runs to keep candidate clean. Inspect actualfourplatform
proof/reviews then sync2.1/2.2/3.1 and acceptance as proven; update Closes739 only
when ready. Normallymerge745 and verify native739CLOSEDbefore738.
StableAtlas050 index-4 COMPLETE (log /tmp/projectatlas-739-index-4.log), available
exactroot brief, privateDB preserved. Low.cargo/.githooks purposes approved.
Known dependency_closure_limit guidance authorizes fullrefresh; no reset.

## Accepted closures and parked work

#731/PR737 closedmerged. #741/PR743 closedmergedmain6927ed0 Nov1 16:32:45Z,
reviewed5dda tree equals49893e53b41eb98bd3e641a200dab26d9981e941.
/tmp/projectatlas-741-closure.json. Do not reopen or start more741source work.
#738 parked8814d6d+memorydirty preserved; reconcile shared739guard without losing
accepted spec/full platform/upgrade acceptance or duplicating contract increments.
#708/PR728 publicc65dfed/local587ecbe+memorydirty preserved. Warm1201.127FAIL,
focused1.031526partial/no SLOwaiver. Acceptedmain globalIssueOps known708task2.3
livechecked/mainunchecked drift; reconcile when708accepted, do not uncheck private
progress. Candidate739 scope excludes it;715globalgate must pass.
Integration .worktrees/v0.5.1 release/0.5.1 retains private715spec changes.
Queued CodexCI gapreview /tmp/projectatlas-codex-ci-followup.json belongs laterowner,
not permission for parallelissueimplementation. PR744 Dependabot outsideRCmilestone;
reconcile current dependency/security at715. Four openRCissues739/738/708/715.
No RC published. Preserve originalbackup /tmp/projectatlas-739-pre-main-snapshot-path.txt.
