# Active context — #739, v0.5.1-rc1

## Goal, ownership and checkout

Finish/normal-merge/native-close ONE issue at a time: #739 -> #738 -> #708,
then #715 RC acceptance/publication and independent downloaded/installed
v0.5.1-rc1 verification. Non-draft prerelease Latest=false; stable0.5.0 stays
Latest. Stable publication/milestone closure require later instruction; #715
stays open through separately authorized stable verification. Goal ACTIVE.
Primary implements; existing Luna Max reviewers read only. No force/admin/
noverify/selfapproval, DB reset or silent confirmed self-hosting-bug deferral.

Selected .worktrees/issue-739, fix/external-excludes-739, PR745 OPEN/unmerged,
Refs739/base6927ed0ca64788db6947b4d43eafce29aac5ed61. Public/local committed
HEAD8a276a885086fd4a0afb386d053d517898f4785e. Native watch-scope/CI follow-up
is UNCOMMITTED; see current checkpoint below. Older sections are predecessor evidence.
Live731/PR737 and741/PR743 merged/CLOSED;739/738/708/715 OPEN; RC1 absent.
Only OpenSpec1.1 checked;2.1/2.2/3.1 and all five acceptance rows remain open.

## Current implemented review follow-up

Code P2 4441758314 confirmed ordinary higher external ancestor replacement
stranded the descendant watch. Register every existing external ordinary ancestor
nonrecursively, with selected immediate-child filtering and BEFORE-insertion256
unique-directory bound. Preserve ordinary in-root recursive authority and native
alias/Win32 parent semantics. Native test covers present/missing leaves, multiple
ancestor depths, missing parents and external/source-contained aliases, then a
new leaf after rebind. Capacity test retains exactly256 on refusal.

Security P3 4441784180 confirmed >1024 uncertain Unicode siblings overflowed
source ingress. Keep arbitrary Unicode/non-UTF-8 equivalence at controlled native
owners; no blanket Unix byte equality or callback filesystem I/O. One packed
atomic publishes recheck pending/structural/generation together; controlled drains
consume before inspection, preserve transient no-epoch uncertainty and restore
pending on failure. CLI fences real initial and later refreshes against ingress,
including pre-entry pending selection, under original work control and cold
reconciliation. Structural Unicode siblings remain conservatively full/rebind;
no universal hostile-local-filesystem availability claim.

Real CLI/MCP testing found the fix initially turned SQLite's own sibling WAL/SHM
writes into false refresh_required for a Unicode source root. SourceBinding exact
captured selected/resolved namespace routing now belongs to RuntimeEventIngress,
with ONE separate pending runtime wake, not the policy marker or an unbounded
collection. Main structural events bypass pending and retain rebind even in an
ASCII parent; mixed/unknown/rescan and distinct source aliases retain handling.
MCP native filter revalidates captured target; a retained runtime candidate forces
full/rebind. All callbacks remain filesystem-I/O-free. Shared drain_source_events
clears runtime pending under receiver fence for ALL three callers; exact queue
reset also clears it. CLI keeps the runtime binding of its SAME open AtlasStore,
never adopts a retargeted SQLite database. Luna731 withdrew the static stale-target
concern after tracing that ownership.

Causal artifacts: /tmp/projectatlas-739-unicode-runtime-{before,after}.json;
initial real Unicode MCP false source_changed guidance versus ASCII positive.
/tmp/projectatlas-739-runtime-burst-before.log confirms ordinary per-event runtime
queueing overflows; coalescing fixes it. /tmp/projectatlas-739-runtime-rearm-before.log
confirms missing reset on direct epoch drains; shared owner fixes it. Expanded
existing callback fence covers root rename/error/Unicode recheck/runtime WAL/main
rename. Explicit previous-target sidecar after selected alias retarget remains
observable and requires reconciliation. No remaining source blockers per Luna731.

Required Rust CI found reachable RUSTSEC-2026-0433: Options::all() enables vulnerable
nested-image wikilinks, and event checkpoints do not bound internal parser work.
Lockfile ONLY pulldown-cmark0.13.5->0.13.6; preserve wiki/ordinary refs. Upstream
fix changes heading facts, so final DER26/semantic23 (previous DER25) repairs them.
Causal Markdown test fails before/passes after; all10Markdown tests passed.
Mac-only two let_underscore_must_use lint failures fixed with drop(writeln!(...));
original timeout, sampler bounds and primary error unchanged. Luna741 approved
parser/contract/Mac changes. Frozen navigation/lifecycle identity pairs updated,
no inventory selector/count/gate changes. Navigation now uses Unicode root for
permanent CLI/MCP ancestor+replacement proof; fixture paths unchanged by local
EXTERNAL_POLICY_DIRECTORY constant. Luna741 approves narrow fixture/inventory.

## Current source and immutable optimized proof

Default locked release/freeze54945 TERMINAL0. Folder
~/.cache/projectatlas-benchmarks/external-policy-739-runtime-recheck-release:
CLI dd15f5772c6f2a280ca4a1f190632c7a3289ae12fdd1b4f6d0afc2e07b34051b,
69,995,872 bytes; FShelper544eb8349e6803c427b11382b32b4297e071336f66b547c465ddc8a339d83fc9.
140-input manifest9764dd5a16f55257740f4ff74268ad37936b067859c91a0e4c0e75a8308f629a.
All140 saved current build inputs verified equal; receipt explicitly records dirty
source/old HEAD. MUST bind all140 to new committed Git tree before normal push.
No product/source edits after freeze without affected rebuild/reproof.

18freshCLI samples and4 predecessor upgrades/rollback PASS via
/tmp/projectatlas-739-runtime-recheck-{cli-cost-proof,upgrade-proof}.{json,log,py}.
Additional2 native DER25->26 ordinary/linked Markdown fact/state/noop/deadline/
rollback/reupgrade cases PASS /tmp/projectatlas-739-markdown-contract-upgrade-proof.*.
Old heading dog.png becomes ![[cat.png|dog.png]], semantic23 unchanged, all approved
purposes/identity/reference outcomes preserved. Zero deadline is entry refusal,
not a mid-publication cancellation claim; owning controlled tests cover latter.

Exact frozen optimized public adapters ALLPASS (58735 TERMINAL0): external1.55s,
documents2.31s, Unicode ancestor+source-root replacement16.83s, automaticstate2.16s.
/tmp/projectatlas-739-runtime-recheck-optimized-adapters.json and optimized-*.log.
Current compiled harnesses use both CARGO_BIN_EXE_projectatlas and
PROJECTATLAS_MCP_CONTRACT_EXECUTABLE overrides to exact immutable CLI (assert_cmd
runtime lookup verified). Real narrow ordinary ancestor e89 BEFOREFAIL / dd15
AFTERPASS with finite notify mode+exit0: /tmp/projectatlas-739-ordinary-ancestor-active-*
plus active-source-cli.py. Earlier cli-after/continuous-after failure artifacts
were HARNESS errors: their node query included retained exists_now=0 tombstones.
Actual active blocked.rs was already correct; corrected active-node query reran
both predecessors/candidate. Do not invent a product defect from those artifacts.

Fresh1000links/depth200 complete fixture init7.127615s/cold3.429214s; all1000 missing
and per-document500/500 complete0omitted, publication unchanged. Historical1dd
before191.476237/95.708869s retained. /tmp/projectatlas-739-runtime-recheck-prefix-complete.*.
Fresh1024links/2000rules cold3readmedian.068774041s, publication unchanged and
1024 persisted missing rows verified: runtime-recheck-cold-policy-cost.*.
Historical512link/64dir e89 and strace counts remain explicitly HISTORICAL;
no final-CLI equivalence claim. Original1doc1000/depth200 extraction correctly
partial at614 intermediate_bytes; accepted complete fixture uses2docs, no cap waiver.

Tracked docs/benchmarks/external-policy-cost.{md,json} now current DER26/dd15,
+538,912bytes/+0.78% vs main. Ordinary init312.84->310.10ms/noop48.29->49.91ms;
10krules noop69.56->97.27ms. All18CLI samples fresh. ONLY18FSsamples reused after
exact rebuilt helper identity, with historical overlapping-build limitation kept.
No general speedup/#708SLO/persistentMCP latency claim. Luna741 final cost/artifact
review APPROVED for saved140-input publication, with post-commit Git binding required. Newcost document update excludes private paths.

## Checks and review/public transition

Full CLI bin478PASS/2ignored39.35s before final shared-drain reset; current affected
46observer PASS2.54s and22policy PASS. Final cli+symbols alltarget/allfeature
Clippy PASS, strictstrings/fmt/diffcheck PASS; inventory3PASS2.73s. Cargo deny
advisories/bans/licenses/sources PASS. Strict OpenSpec and planned IssueOps PASS.
Moved an existing document-prefix subsection out of Open Questions (prose unchanged)
so its authoritative final None is recognized; no gate bypass. Live739 WhatChanges
expanded for ordinary ancestors, Unicode/runtime coalescing and patched parser/DER26;
app update + readback EXACT, implementation and acceptance states unchanged.

Current P2/P3 fixes have Luna731 source-publication approval, not whole-issue
acceptance. Luna741 parser/fixture/cost approvals likewise bounded. Cheap scoped
PR IssueOps passed. Next commit sole(#739), all140 Git/saved source binding,
normal clean push with mandatory hooks; source checkout MUST stay clean while
push runs. Then fresh exact-head native/bot/CodeQL/security reconciliation, examined
threads, task/five-acceptance sync and normalmerge/native739CLOSED before738.

## Hosted/security state (old published head, terminal)

34d normalpush TERMINAL0 at11:18:02Z,1804PASS/34suites/0FAIL/12ignored; do NOT
poll it as live. Source41435727790 TERMINAL: plan/repository/Linux/Windows/
diagnostics PASS; Rust FAIL only new advisory, both Macs FAIL only two Mac lint
errors now fixed locally. Both Mac volume profiles including APFSX passed, but
prior APFSX timeout cause remains unestablished; no flake/fixed claim. Windows
17policy/43observer and external/document/rootreplacement realE2Es passed.
Current source still requires new native Windows/Mac and exact feedback proof.

34d code+separateSecurity reviews COMPLETED with new ordinaryancestorP2 and
UnicodeP3. Seven genuine old/new review threads remain open, do not bulkresolve.
CodeQL41435718488 all7 complete: Rust2076504701=15, Actions2076386313=2,
other5zero. Alert417 document_prefix_metadata wrapper NOT YET dispositioned.
Full /tmp/projectatlas-739-34d515-{rust,actions}.sarif.json and scoped-alert-pages.json.
Fresh instances and whole final input reconciliation pending (likely21 inputs:
17native+3resourcecomments+newRustSec; derive actual collection, no dedup).
Historical rendered18input assessment/securitysummary6563449602 describes prior
reportedb703 collection, NOT new417/Unicode/RustSec. No dismissal/suppression or
clean-scan claim. New static phase must normalize whole collection before verdict,
remain inline/read-only/no agents/tests/builds/edits and use triage skill+refs.
GitHub findings intake REST explicit ref=refs%2Fpull%2F745%2Fhead only.

## Parked work and tool continuity

#738 parked8814d6d+dirtymemory. #708/PR728 publicc65dfed/local587ecbe+dirtymemory;
warm1201.127ms FAIL/focused1.031526ms partial, noSLO waiver; reconcile live/main
2.3drift only when708 active. Integration .worktrees/v0.5.1 has3private715
proposal/design/spec edits plusmemory; never revert. Later715 evaluates PR744
outside RCmilestone and CodexCIgap. Keep715 open through later stable promotion.

Source /tmp/projectatlas-rc1-test-env.sh for cargo/gh; PATH removes rg, use rg
outside that shell or /usr/bin/rg. Exact installedstable050 Atlas MCP brief returns
refresh_required/policy_drift for issue739 after newer candidate-derived DB state;
preserve DB, no orientation reset/scan. Normal hooks own required refresh/gates.
Read complete version-matched installedAtlas skill after compaction before calls;
custom Rust+cognition/workspace and database/SQLite skills apply. Current build,
proof and check sessions above TERMINAL; no normalpush started for newfollowup yet.


## Current checkpoint — native CI follow-up after 8a276a8

Normal push74848 TERMINAL0 at2026-11-02T13:36:46.975731Z;1813PASS/34suites/
0FAIL/12ignored plus required gates. No push currently live. PR745 remains
OPEN/unmerged/Refs739 at8a276a8. Source41454506360 TERMINALFAIL: plan,repository,
Rust,Linux,MacARM,Windowsdiagnostics PASS; MacIntel125307725189 cold CLI build
timeout before CLI units execute; Windows125307725199 two policy-unit failures.

Uncommitted owning fix: cfgWindows std::path::absolute on each watch-snapshot
selection, preserving raw source-policy opens/verbatim and Unix traversal. Native
canonical nearest parent decides Win root authority; a real outside alias parent
enables higher namespace coverage even when targeting inside root. Additional
Unix root/../external alias->inside analogue confirmed causally: removing only
the external_namespace setter FAILS; fixed generic regression passes. No callback
I/O or extra filesystem probes. Existing native parent-selection fixture must
pass on Windows; local Linux cannot prove its Win32 branch.

Second Windows failure is unresolved Os5AccessDenied in the nine-layout ordinary
ancestor replacement test. Added operation/layout/path context to every fixture
mutation and watch install/rebind without skips, swallowed errors or deadlines
changes. Need actual native rerun to locate failing operation; do NOT invent a
watcher-locking diagnosis from teardown events. Notify shares delete access.

MacIntel fix moves UNCHANGED ten-minute native-policy commands after existing
twenty-minute PHP cold-build owner. Feature-specific compile remains capped and
needs actual hosted proof; no full-warm guarantee or product SLO/deadline change.
Luna731/741 plans approved; actual-diff independent follow-ups requested.

Latest local batch66383 is TERMINAL (handle missing; all saved logs pass):
23policy tests, affected CLI alltarget/allfeature Clippy, strictstrings, quality
gate workflow contract1 and frozen E2E inventory3. Earlier eprintln Clippy failure
was fixed by contextual errors. Source mutant used for causal test restored.

All dd15/140input/cost/sixupgrade/optimized adapter results above bind committed
8a276a8, NOT current changed source. Benchmark docs explicitly label predecessor
evidence. Publish this reviewed diagnostic/native follow-up, then obtain actual
Windows cause and final source before final artifact freeze/reproof; no claim of
final acceptance based on old artifact identity. All issue task states unchanged.

Current exact8a CodeQL41454498482 all7SUCCESS: Rust2077380626=25,Actions2077297197=2,
other5zero. Full SARIF/scoped27instances retained. Whole31input collection normalized
at /tmp/projectatlas-739-8a276-normalized-inputs.json (27native,3resourcebot,RustSec).
Static triage explicitly left before native CI repairs; NO completed current31
verdict/render/clean-scan claim. Prior18publicsummary is historical. Reconcile
all full facts/flows and render complete currentcollection after source settles.
Do not suppress/dismiss alerts or resolve genuine threads before proof.

Next: independent actual-diff review; cheap fmt/OpenSpec/IssueOps; normal commit
and clean push with required hooks; preserve same hosted run and inspect native
results.739only before738->708->715RC1publication/installedverification.
GoalACTIVE. Preserve parked work and three private715specification edits.

Both actual-diff independent Luna Max reviews APPROVE this bounded follow-up
for native rerun. Not whole-issue acceptance, and Os5cause remains unresolved.
Final fmt/strictOpenSpec/scopedPRIssueOps/diffcheck PASS. Normalcommit/push next;
no selected-checkout edits during required hooks.
