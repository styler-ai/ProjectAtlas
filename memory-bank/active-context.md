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
HEAD57bad541e70ff5833d25296261521e438815e0c1. Native watch-scope/CI follow-up
is COMMITTED/PUSHED; see latest hosted checkpoint below. Older sections are predecessor evidence.
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


## Latest authoritative checkpoint — public57bad54, 15:19Z onward

Normalpush20909/wrapper4193263/git4193267 TERMINAL0 at2026-11-02T15:19:08.519898Z.
Do NOT repoll/restart it as live.1814PASS/34suites/0FAIL/12ignored; required Rust,
docs,dependency,repository,Atlas scan+lowpurpose lint gates PASS. Public/local
HEAD57bad541e70ff5833d25296261521e438815e0c1 verified; PR745OPEN/unmerged/Refs739.
Only memory edits afterpush; all artifact/cost proofs remain predecessor8a/dd15
as explicitly labelled, not current-source equivalence.

Source41470417383 LIVE, not terminal. Plan125362199356/repository125362377370/
Windowsdiagnostics125362377377 SUCCESS. Rust125362377449/Linux125362377475/
MacIntel125362377534/MacARM125362377643/Windows125362377760 LIVE. Retain same
run/handles; Windows initially toolchainpreflight, Linux/MacARM PHPcoldbuild.
Native policy commands now followPHP, no deadlineorselectorwaiver. Need actual
Windows test result to establish Os5AccessDenied operation/layout/path; not yet
rooted or claimedfixed. MacIntel compiletimeout fix likewise needsnativeproof.
pr-state41470414773 SUCCESS. CodeQL41470411586 LIVE. Both Codex code review and
separate securityreview RUNNING exact57 per6562604562 at15:19:44Z; no quotaerror.
Prior8a code+Security completed; codehadfalse/false, securityretained3advisories.

Current supplied31securitycollection assessment is COMPLETE, superseding older
incomplete claims here: reported27native8a +3resourcebot +RustSec, inline/static
against57; wholecollectionnormalizedBEFOREverdict,31not_actionablecurrentpatched
source. All13Rustfiles in reportednativeflows unchanged8a->57; workflowsteps
explicitlymapped. Correct4758Nodekind/4765Nodepath/4767parent_path (NOTlanguage);
414fs::copy is contentread/write (NOTmetadata-only). Root/database selectors are
independently constructed; derived targets have Normal-component/freshnative
prefixadmission. Privatecfgtest developeroverride is not TempDirconfined byclaim.
Originalresource/advisorydefects genuineinpredecessors, not originalfalsepositives.
Noalertdismissal/suppression, noinputdedup, no newscan/exhaustivecleanclaim. App
RENDERED31rowsSUCCESS. No tests/builds/apps/agents/repoedits initiated during
staticphase; already-runningnormalpushindependent. Staticphase explicitlyLEFT.

Artifacts /tmp/projectatlas-739-native-scope-{normalized-inputs,complete-triage,
rendered-triage,triage-render-receipt,static-location-map}.json plus
*static-triage.py. Appreceipt31. Currentpublicsecuritysummary6563449602 rewritten
for31andexactreadback. NewCodeQLcollectionafter57analysis stillneedsfullintake/
flowreconciliation; do not claim currentnativeanalysisreviewcomplete yet.

CodeQL417thread PRRT_kwDOQyFgJ86ypas- now RESOLVED after staticroot/admission
disposition, exactpublicheadcheck and technicalreply4444055416 exactreadback.
SevenGENUINEfixthreads remainOPEN awaiting requiredacceptance. Fullthread-aware
read /tmp/projectatlas-739-native-scope-review-threads.json (capturedBEFORE417
resolution); do not treat it as currentresolvedstatewithoutchecking.
PRbody updatedapp/readbackEXACT; *native-scope-pr-body.md +*pr-readback.json.
It qualifies predecessorcost/upgrade/artifactproof and unknownWindowscause.

Next: poll SAMEsourceWindows/Macjobs; inspectnativecontext iffails; fixconfirmed
owningcausepreservingcapabilities, causaltests+LunaMax, then normalpublish. Once
source settles finalartifactfreeze/affectedcost+upgrade+realCLI/MCPproof; reconcile
newheadbots/CodeQL, sevenfixthreads, implementation/fiveacceptance and normalmerge/
native739closure BEFORE738->708->715RC1publication+independentinstallverification.
Alltaskstates unchanged, goalACTIVE; RC1unpublished,050Latest.715staysopenuntil
separatelyauthorizedstablepromotion. Parked738/708 andprivate715edits preserved.

Fresh57 review summary6562604562 at15:22:24Z: code_review COMPLETED false/false
(no findings), separateSecurityreview RUNNING. PartialCodeQL6exact57analyses
saved *native-scope-analysis-snapshot.json: Actions2078116821=2(fullSARIFfetched),
JS2078117874/Ccpp2078116547/Python2078116519/Csharp2078116426/Go2078115522=0.
Rust125362184021 stillLIVE, analysispending; do not assignnewcollectionverdict
beforecompleteintake/normalization. Existing31static assessment reported8a stays
validasqualified, not evidence that newexact57collectionwas fullyreconciled.


## Windows lifecycle follow-up — latest implementation checkpoint

Public/local committed HEAD57bad541e70ff5833d25296261521e438815e0c1; PR745OPEN.
Current UNCOMMITTED changes: policy_watch.rs TESTS ONLY, design.md, spec.md and
memory. No product/runtime/backend/dependency change in this follow-up. Source
41470417383 stillLIVE atlastread: Rust125362377449 andLinux125362377475SUCCESS,
MacIntel125362377534 documentresourceproofLIVE, MacARM125362377643 lifecycleLIVE;
Windows125362377760TERMINALFAIL. Preserve same remainingjobs while normalnewpush
gatesrun; a genuine nextsourcefix may supersedeoldrun, never restartforobstimeout.

Windows57 now20policyPASS1FAIL; priorWin32selectionfailure FIXED. Exactfailure
/tmp/projectatlas-739-native-scope-windows-ci-clean.log: DIRECTFIRSTLAYOUTpresent
fs::rename(base/control/current,base/retired) Os5AccessDenied while watchednested
is below it. Not alias/setup/write. Sourceobservertestsnotreached. Nativecause
notyetaccepteduntilnewcausalmatrixpasses. MicrosoftFILE_RENAME_INFORMATION
remarks125describesopen-descendant renameconstraint; shareDELETEaloneisnoproof
ancestorcanmove. OfficialURLrecordeddesign; don'tclaimsupportedautomovefrom
legalnearesttestsalone.

Currenttestfactor: original9actualhigherancestorlayoutsremainnonWindows unchanged
viaassert_native_policy_directory_replacement. Windows7legalpositivecasesreplace
nearestwatcheddirectoryitself(direct/externalalias/sourcealias present/missingleaf)
plusmissingparentcreation, thenrealinstall/rebind/distinctlaterleaf. NoPermission
Denied-as-successinreplacementtest. Sharedtestqueue drainnowpropagatesnotifyErr.
Windows-onlynative_descendant_directory_handles_bound_ancestor_rename: nohandles
roundtrip; targethandleonlyroundtrip; exactnotifyFILE_LIST_DIRECTORY+BACKUP|
OVERLAPPED+READ/WRITE/DELETEsharechildhandle causesexactOs5withoutmutation; drop
childthenroundtrip. RepeatwithrealnotifytargetthenaddnestedONLYONCE. Dropnotify
andbounded5srenameackretryONLYrawOs5;othererror/deadlineFAIL. Freshcoldlifecycle
movesparentwhileunwatched,recreatesrules, NEWselection+watcher realcoldinstall,
distinctlaterleafnotification. No freshqueuederrorsdiscarded; dropfreshwatcher
andawaitteardownusingfreshclosed-parenttarget(retiredoccupied). Nativeproofpending.

Luna741caughtduplicatewatch+asyncDropandfreshwatchercleanup/drainbugs; allfixed.
Latestactualdiff741APPROVEDFORNATIVEPUBLICATIONONLY;731boundedsource/specslice
APPROVED, informedofsubsequentcleanupfixes. Specnowexplicituser-visibleWindows
stop-BEFOREmove/recreate/restartfreshpath; originalWHENactualancestorreplacement
retainedfornative-permittedmoves. NoautomaticwarmclaimforOS-refusedoperation.
No wholeissue/platformacceptanceclaim untilnativecausalmatrix+restartproofpass.
Local58406TERMINAL0:23policytestsPASS0.24s, affectedCLIalltargetallfeatureClippy,
strictstringsPASS. Finalfmt/OpenSpec/scopedIssueOps/diffcheck87202runningatwrite.

NewSecurityreport4444215885 fs/lib199 (ancestor.ignoreFIFO/directory aboveGit):
inlineSTATICsingle-reportcollectionnormalizedBEFOREverdict, not_actionableunder
acceptedinheritance/unsupportedinputcontract. Canonicalignore0.4.34loadsandmatches
.ignoreALLparents independentlyGitcutoff; validator, singlepath,batchseed and
sourceobservationselectsameinputs. AcceptedSpecinherited/noGit+nonregularancestor
refusalexplicit; droppingvalidationwouldletengineopenFIFO, stoppingancestrywould
changesupportedpolicy. No generalsharedancestoravailabilityguarantee. AppRENDERED
1rowSUCCESS; *ancestor-policy-{normalized-input,static-triage}.json. Staticphase
explicitlyLEFT before agents/tests/edits. Independent741agreesdisposition. Real
CLIproof4casesPASS /tmp/projectatlas-739-ancestor-policy-compatibility.{py,json}:
stable050+immutable57bothhonorregularaboveGit.ignore/noGit; currentdir/FIFOprompt
refusalpreservespublication/identity/approvedpurposes/authoredrevision, regular
policyfilesymlinkpositive. TwoHARNESSerrorspreserved *initial-harness-failure.*
(oldstablealreadyknowndocumentreasondivergence) and *root-selection-harness-failure.*
(CLIinitdiscoversGitroot, notnestedatlas). No newproductdefectclaimed. Existing
Rustsubrootinheritanceunitremainsproof. Reply4444477324EXACTreadback; newthread
PRRT_kwDOQyFgJ86yu_LD RESOLVED afterheadcheck. StillSEVENgenuinefixthreadsopen;
CodeQL417FPthreadalreadyresolved. Public31summarynotyetupdatedfornewseparatereport.

OptimizedCURRENT57artifactbuild92805TERMINAL0(41s), frozen ~/.cache/projectatlas-
benchmarks/external-policy-739-native-scope-release: CLI64ad0bd87e2ca04c2240a0a6
d4145473c9deeff70bbb1afce5fea8d01dacad1e,69,998,592bytes; FShelper544eb8349e6803
c427b11382b32b4297e071336f66b547c465ddc8a339d83fc9 byteidentical.140inputmanifest
c082f60c9419200af526544d98d38cc00e9aafb3ea3c36ee9ec9de4db04b07cd; allcommitted57
ANDsavedbytesmatched /tmp/projectatlas-739-native-scope-commit-source-binding.json.
Currentlatercfgtest-onlyeditsdifferfromthatmanifest; don'tclaimequivalencewithout
rebuild/byteidentityandnewsourcebinding. Fresh18CLIcost+4stable/mainupgradecases+
2DER25MarkdownrepaircasesPASS89968: ordinaryinit311.27->312.21ms/noop48.37->49.47ms;
10krulesnoop71.36->95.51ms, scopednotSLO. *native-scope-{cli-cost-proof,upgrade-
proof,markdown-contract-upgrade-proof}.{json,log}, scriptscost/upgradecloned.
Four64adoptimizedCLI/MCPadapters81398TERMINAL0 external1.892s/doc2.341s/Unicode-root
ancestor+replacement16.770s/automaticstate2.354s. *native-scope-optimized-adapters
.{py,json}, optimized-*.log. Bothruntimebinaryenvsoverridden; compiledharnesses
fromnormalpushlog. No newprefixcomplete/coldpolicycostmeasurementfor64adyet.
Resourcefixturesstill8a/dd15predecessorevidence; trackedcostdocsalsostill8a,
truthfullymarkedpredecessor, notfresh57/finalsource. Nextfinalfreeze/reproofor
exactbinaryidentityreusemustdocumentthatdistinction.

57CodeQL41470411586Rust125362184021stillLIVE atlastread;6othersSUCCESSpartial
analysissnapshotandActions2078116821SARIFsaved. No freshcomplete57nativecollection
intake/verdictyet. 31reported8acollection/current57staticrenderremainsqualified;
newseparateancestorreportaddsoneassessedinputbutdon'tclaimnewnativeanalysisdone.
BothCodexreviews57COMPLETED: codefalse/false,securitytrue/truefromnewancestor
report(nowdispositioned/resolved). Getfreshsummaryafternewhead, notquotablocked.

NEXT: finish87202cheapgates; normalcommitofWindowsTEST/spec/design/memoryslice,
normalcleanpushwithrequiredhooks(noeditsduringpush), thenactualWindowscausal/
legal/coldrestartproof. Holdalltask/fiveacceptancecompletion/mergeuntilactual
proof. Finishfrozenartifact/cost/upgrade/resourceCLI/MCPbindings; reconcilefresh
fullCodeQL+code/securityfeedback; reply/resolve7genuinefixthreadswithproof; then
normalmerge/native739CLOSED before738->708->715RC1publication+independentinstall.
GoalACTIVE,4issuesOPEN,RC1unpublished,stable050Latest;715privateeditsandparked
738/708preserved. Stablepromotionrequireslaterinstruction.

Final87202cheapgatesTERMINAL0:fmt/strictOpenSpec/scopedPRIssueOps/diffcheckPASS. LatestactualLuna741publicationapprovalincludesallcleanup/errorpropagationanduser-visiblecontractfixes. Normalcommit/cleanpushnext; nochangeselsewhere.


## Current authoritative checkpoint — native Windows proof and hosted disk fix

Public/local HEAD 1b7880c46105d18b7b93043dea5e5e71c1589c0c; PR745 OPEN,
unmerged. Previous normal push65057 TERMINAL0 at16:17:33Z,1814 tests PASS across
34 suites,0 failures/12ignored. Do not repoll that push or treat it as live.
Source41480033316: Rust/repository/diagnostics SUCCESS; Windows native policy
and source-observer units SUCCESS (including causal handle matrix/legal
replacement/cold restart), MacARM native units SUCCESS. Windows now installed
host-reader proof; MacIntel document resource proof; MacARM concurrent rollback
proof at latest read. These are still running, not whole-platform acceptance.
Linux125396656789 FAILED at independent installed runtime /bin/cp with ENOSPC,
not a product assertion. Preserve copy semantics (Rust executable-fd workaround).

Current bounded follow-up sets the e2e-smoke job dev/test DEBUG=1 to match the
already accepted Rust job, from the first build across all native platforms.
No optimization/assertion/feature/test selector/deadline/release-profile change.
Luna731 actual-diff review APPROVED, conditional on fresh hosted Linux copy proof.
Existing workflow quality-gate E2E PASS; final fmt/OpenSpec/scoped IssueOps next,
then normal commit/clean push with required hooks. No edits during normal push.

Fresh immutable default-release CLI dff7764ae6114458fada5e218d1dd7c29c59ba7187aa83da85e2b26c72316dfd
(69,998,192B), frozen source manifest e990ab1e4ff21be2ea4777223ab3dea9fd945081eb32bc099025d2027f1f3d4e
all140 inputs match committed1b and saved bytes. New proof batch19289 TERMINAL0:
18 CLI cost samples,4stable/main upgrade/rollback cases,2 DER25->26 Markdown
repair cases,4 optimized CLI/MCP adapters all PASS. 1000links/depth200 init7.103s
and cold3.454s;1024links/2000rules cold median67.790ms. Publication unchanged;
complete500/500 document relation rows each checked, not whole-AST coverage.
Updated cost MD/JSON reviewed APPROVED by Luna741; +0.78% binary vs acceptedmain.
Current metadata/CI changes outside140 buildinputs; rebind after commit rather
than repeat proved artifact. No general SLO or persistent-MCP timing claim.

Task2.1 now CHECKED locally and live739;2.2/3.1 and all five acceptance rows
remain UNCHECKED. Full platform/installed acceptance still required. Both Codex
reviews1b COMPLETED; code false/false, security retained old advisory threads.
CodeQL1b workflow41480027200 all7 SUCCESS, but native findings need fresh full
intake/reconciliation (CodeQL aggregate remains failed). Prior full32 static
assessment reported57/current1b complete and publicly read back; no alerts
suppressed/dismissed. Seven genuine fix threads still open pending acceptance;
417/ancestor dispositions resolved with exact public readback.

Next: normal publish bounded CI/cost/task update; preserve native run handles,
verify fresh Linux installed-reader and every native platform; reconcile exact
head bots/CodeQL, complete2.2/3.1/acceptance and genuine feedback, normal merge
and native closure739 BEFORE738->708->715 RC1 publication/independent install.
GoalACTIVE; RC1 unpublished;050Latest;715 stable promotion needs later instruction.
Preserve parked738/708 and three private715 specification edits. No fixed ETA
while a required hosted gate is failing.


## Current public checkpoint — c431fdb source verification live

Normalpush66804/wrapper407506/git407510 TERMINAL0 at2026-11-02T17:07:43.332120Z.
All1814tests/34suites PASS,0FAIL/12ignored; required Rust/docs/dependency/
OpenSpec/IssueOps/Atlas scan+lowpurpose lint PASS. Do NOT repoll/restart push.
Public/local HEAD c431fdb39bcf75b7c95ec788a6ca146f832a750b, verified app exacthead;
PR745OPEN/unmerged/Refs739, base/main6927ed0ca64788db6947b4d43eafce29aac5ed61.
Selected clean immediately after push; only later memory updates. Commit adds
CIresource parity+freshcostdocs+2.1completion/proposal/memory; NO Rust/buildinput
changes. /tmp/projectatlas-739-platform-debug-commit-source-binding.json verifies
all140 frozen committed AND saved inputs match c431 and exact dff CLI artifact.
No redundant optimized rebuild/proof required for this metadata/CI follow-up.

Fresh SOURCE41488165395 LIVE on exactc431; plan125424735486SUCCESS. Currentjobs:
repository125425270332, Rust125425270378, Windows125425270659,
Windowsdiagnostics125425270703, MacARM125425270707, Linux125425270745 LIVE;
MacIntel125425271172 queued atlastread. Preserve thesehandles; no restartmerely
forobstimeout. Sourcepolicy/native/installed/fullplatform gates stillpending.
PRstate41488161702/41488315311SUCCESS. Metadata41488314690 LIVE followsSAMEsource
proof; bodyedit cannotcancel/rerun it. CodeQLdynamic41488159691LIVE; needall7
exactc431analyses before finalcompleteintake. Codexsummary6562604562 at17:08:19Z
bothcode+SecurityRUNNINGexactc431, no quotaerror. Fourretainedadvisorythreads are
not newverdicts; 7genuine fixthreads remainOPEN.

Previous1bSOURCE41480033316 nowTERMINALCANCELLED by configurednewsourcecommit
concurrency, NOTobservation-timeoutrestart. ARM125396656378 fullSUCCESS.
Windows native causal/legal/coldrestartunits andrealhostreaderSUCCESS; later
pluginproof interrupted. MacIntel125396656272 PHP SUCCESS16:21–16:34:45, native
policyunits SUCCESS16:34:45–16:40:04. Resourceproof CANCELLED17:08:23 while release
service rustc wasstillcompiling17:07:10, not atestassertionfailure/stepdeadline.
Logs /tmp/projectatlas-739-native-rename-macos-x64-ci{,-clean}.log. Linuxfailed
ENOSPC atcopy boundary; exactnewDEBUG1nativejobpositive remainsrequired.

Luna741 finalbounded acceptance audit: CONDITIONALAPPROVAL, no new source/spec
blocker; reviewed source/cost/proof supports firstfourdimensions subjecttofresh
hostedproof. Notfinalreadiness. Luna731 boundedCIactualdiff APPROVED pending
freshLinuxcopypositive. Same reviewers reusable; primary owns allwrites.
Issue739 bodyprogress wording nowagreeschecked2.1/pending2.2/3.1/fiveacceptance,
appmutationEXACTreadback. Two unansweredgenuine threads nowtechnicalreplies:
ancestor4445255796 andUnicode4445256259, bothexactreadback; stillOPEN. Freshapp
inventory18threads/7unresolved,no newfinding. PRbody updatedcurrentdffcost/
140sourcebinding/nativeproof/CIresourcefix andEXACTappreadback atpublicc431;
/tmp/projectatlas-739-platform-debug-pr-body.md. Prior32staticassessment
reported57/current1b remainsqualified; freshc431nativeflowreconciliationpending.
Prepared NOTRUN /tmp/projectatlas-739-platform-debug-exact-intake.py targets
c431/PRheadref, normalizesWHOLEcollectionbeforeverdict. No currentheadscanclaim.

Next: monitorSAMEfreshsource, establishLinuxinstalledreaderpositive and allnative
platforms, finalexactheadCodeQL/bots/threadreview, thencomplete2.2/3.1/fiveacceptance
withreviewproof andnormalmerge/native739CLOSED BEFORE738->708->715RC1. Finaltask
metadata stillneedsnormalpublish/requiredchecks, no bypass. GoalACTIVE;
RC1unpublished/050Latest; stablepromotionrequireslaterinstruction. Preserveparked
738/708 andthreeprivate715specedits. No fixedETA while requiredproofpending.


## Policy-fence security follow-up — implementation verified locally

Public/local committedHEAD c431fdb39bcf75b7c95ec788a6ca146f832a750b, PR745OPEN
unmerged. CurrentDIRTY follow-up source/spec/tests/inventory/memory is primary's
authorized #739 work; sourcecommit next, NOTyetpublic. Newsecurityreport4445386019
at c431 policy_watch855 is confirmed bounded LOCALresourceamplification: each
uncertain callback advancesgen, so initial/latercontinuousCLI fences repeatfull
reconciliation untiloriginal30mdeadline. Notinfinite/budgetrenewal/remoteexecution.
One-reportcollection normalizedBEFOREverdict, staticappRENDERED1confirmed;
/tmp/projectatlas-739-policy-fence-{review-report,normalized-input,static-triage}.json.
Staticphase explicitlyLEFT before fixes/tests/agents. Userauthorizedallsecurity
fixes; no permissionpending. Luna741independentPLAN APPROVED, primarywroteall.

2.1 REOPENED locally+liveissue739 withbothmitigationsunchecked,1.1checkedonly;
2.2/3.1/fiveacceptance allunchecked. WhatChanges/spec/design/proposal boundthree
nativeattempts perinitial/laterrefresh thenexistingpolling, ACTIVEcontrolfirst
pollreload/refresh, laternormalpercyclecontrols/totalfinitecycles. Generation
fencepreserved; pollinghasordinarybetween-samplestransientlimit. No publicenum,
schema or derivationchange (still26/23). No database/queryproductchange.

Implementation runtime/policy_watch.rs MAX_POLICY_FENCE_ATTEMPTS3, retainsflags
andchecksoriginalcontrolbeforeWatcherfallback. Bothrun_notifywatchfencecallers
dropnativewatcher anddirectlyusecontrolledfirstpoll; latercaller subtracts
acceptedcyclesandaddsreturnedcount. Existingpollwrappercreatesnormalfirstcontrol;
privatecontrolledhelperusesplan.reload_controlled+refresh_index_controlled
firstsample, existingfreshpercyclebehaviorlater. Typedwork/quota/verification
errorsnotconvertedtoWatcher. SharedMCPobservernotchanged.

Causalprefixtest --all-features --bin projectatlas fails oldloopafter4safely
capped realrefreshattempts; /tmp/projectatlas-739-policy-fence-before-fix-bin.log.
Twoinitialwrong --libselectorsran0tests, NOTcoverage. Correctedteststargetbinary.
4cli_policy_fence_*PASS:bounds3,latecancel,originalABA/preentry.
1polling_handoff_preserves_active_control_and_authored_statePASS:cancel/expired
firstpollnopublication/statechange+positiveidentity/purposes/source. Existing
watch_work_failure...PASS. Logs policy-fence-{focused,control-proof,existing-
budget-proof}.log. Unitcompileinitialnodefieldharnesserrorfixed to node.node.

RealLinuxCLI proof37215TERMINAL0 /tmp/projectatlas-739-policy-fence-native-cli-
proof.{py,json,log}: predecessorimmutabledfftimesout8s with6837unrelatedwrites;
changeddefaultdebugCLI initial3.37s/later4.04s bothportable-polling4cycles plus
selectedpolicy/sourceedits,unchangedallapprovedincldormant/identity/revision.
Firstscriptmodeexpectedpolling notexistingportable-polling; preservedmodeharness
failure,correctedwithoutproductchange. No generalSLO. Currentdebugbinarymaynow
beallfeaturetestbuild; JSONrecordsactualearlierdefaultbinarySHA.

PERMANENT continuous_cli_policy_flood_preserves_polling_and_later_edits added
toe2e_navigation; Linuxinotifyhighfrequencycase, bothinitial/laterhandoffs, bounded
noiseactorstop/join+childkill/wait,error/deadlinecleanup,4cycles,realdocument
ignored/resolvedoutcomes+authoredstate. Firstpurposeexpectationusedactive-only
helper; productpurposesremainDORMANTnotlost. CorrectednewcasewithEXISTING
mcp_database_snapshot.authored_purposes, helperdocclarifiedactivewithoutchanging
itsbehavior. No rawnewSQL/productDBchange. Otherplatformssharerefenceunits;
newcontrolledpollunitexplicitinall4nativeCIjobs,original10mincapunchanged.

Luna741source/E2Ereview foundno furthercodeblocker; caughtpairedinventorygap,now
CLOSED byreview. Inventory225tests,newLinuxcfgrow,normalizednavigationhash
e8a3ffc5634e65da75f7c7bc0214573142c70fd5d79ffd464785e156303e75c4 inJSON+
source_contract. HISTORICAL440symbols/frozenfacetsunchanged. Repeatedrules/xdg
fixturepathliteralscentralizedatfileowner,samepathbytes.
FINAL79213TERMINAL0:3inventoryenforcement/tampertestsPASS,permanentE2E1PASS10.99s,
CLIalltarget/allfeatureClippyPASS,strictstringsPASS,fmt/strictOpenSpec/diffPASS.
Correctstringgatecargo projectatlas-lints strict-strings (alias); initialwrong
packagecargo-projectatlas-lintsnotfoundNOTcoverage. ReviewpublicationAPPROVED
forboundedsource+E2E+inventory, NOTwholeissueacceptance.

C431hostedSOURCE41488165395 stillLIVE: Rust/repository/diag/Linux/ARMfullSUCCESS.
Linuxhostreader125425270745PASS17:28:59–17:30:35 verifies ENOSPCfix. Windows
125425270659 lateobsoleteMCPhandoff; MacIntel125425271172 lateforwarderlifecycle,
bothpastresource/hostreaderproof,notterminalyet. Preservehandles. All7CodeQL
c43141488159691SUCCESS; Rust2079171492=25,Actions2079021320=2,fiveothers0.
CompletePRrefREST27native+33wholeinputintake saved platform-debug-exact-*;
WHOLE33normalizedBEFOREanynewaggregateverdict. All13previousreportedRustfiles
byteunchanged57->c431 (beforethisnewdirtyfix); Actionsfullfacts/flowsidentical
afterEXACT5lineenvmapping. Newwhole33finalsourceassessmentNOTcompleted/rendered.
Prior32reported57/current1bassessmentstaysqualified; new1fenceconfirmed.
Botcodec431COMPLETEDfalse/false;SecurityCOMPLETEDtrue/true(newfence).

19threads/8unresolved:previous7genuine+newfence. Newreply4445535052 exactreadback,
threadOPEN. Liveissue739reopenmirrorEXACT. PRbodyandpublicsecuritysummary6563449602
updatedEXACTreadbackqualifynew1outsideprior32; no currentclean-scanclaim.
Atlas050exact739briefreturnedpolicy_drift/fullrefresh aftercandidate26scan;
preserveprivateDB,purposes; no orientationdowngrade/reset/scan. Exactbounded
sourceevidenceused, notstalegraph.

NEXT: finalcheapIssueOps,sourcecommitONLYintended739files; defaultreleasebuild/
freezecurrentcommittedsourceBEFOREnormalpush, freshaffectedcost/upgrade+adapters
(includenewflood) andfinalcostdocs+sourcebinding; metadatacommitwith2.1checked
ONLYafterproportionateproofthenNORMALcleansinglepushrequiredhooks. Thisavoids
extraCIcyclejustforartifactdocs. Newproductionbytesinvalidate dffascurrentproof:
keepdff/c431historical,neverassumebinaryidentity. Thenfreshnative/securityfull
headreconcileand8genuinefeedback/tasks/acceptance;normal739merge/nativeclosure
BEFORE738->708->715RC1publish/independentinstall. GoalACTIVE,RC1absent/050Latest;
preserveparkedwork/private715edits,stablepromotionrequireslaterinstruction.


## Sourcecommit2d16 and newly exposed Windows E2E mismatch

Local sourcecommit2d16fd407da6f8973a7676ecc3ee0c2ef98bc7f2 creatednormally after
scopedIssueOps candidate4/live4/checked1,fmt,diff PASS. Containsreviewedbounded
fence/controlhandoff +3newunits+LinuxnativeE2E+nativeCIcontrolselector+inventory
225/hash e8a3ffc5...e75c4 +spec/task/memory. LOCALONLY, NOTPUSHED. PublicPR745
stillc431fdb. No live localbuild/test/push now.58578TERMINAL0;79213TERMINAL0.
Do not repolloldcompletedhandles. 2.1staysreopened(allacceptanceunchecked).

Beforeplannedoptimizedfreeze, c431Windows125425270659 TERMINALFAIL atlate
External source policy and root replacement: externalpolicytest anddoc-transition
precedingtestsPASS; source_root_replacement_preserves_cli_mcp_external_bindings
FAIL16.95s with'CLI replacement failed: Access is denied. (os error 5);stdout=
stderr='. Logs /tmp/projectatlas-739-platform-debug-windows-ci{,-clean}.log.
CompletejoblogsavailablewhileoverallrunLIVE. Do NOT assumewholeWindowsaccepted.

Exacthelpercurrente2e_navigation:14017 replace_policy unconditionallyrenames
directory/policy/current -> directory/retired-policy whilecurrent/nested policy
iswatched. It isfirstcalledafterCLIwatcher_ready(14150); MCPshareshelperbutwas
notreached. ThisistheSAMEoperationclassalreadyshownOS-refused bynative1bWindows
descendant-handle matrix. Genericcatchdoesn'tidentifywhichoperationreturnedOs5,
so don'tclaimdirectstageproofyet. No productionWindowsbackendbugestablished.
Currentroot/source rename also lacksstagecontext.

PLAN NEXT(primaryownsallwrites, Luna741read-onlyplanreviewRUNNING): alignactual
warmpublicCLI/MCPfixturewithalreadyacceptedWindowslimit—Windowsreplace nearest
selectedconfigparent nested(legal target-only case); nonWindowsretainhigher
policy/current ancestor. Addinspect_err stage/pathdiagnostics aroundpolicy/source
rename,recreation,writes preservingrawioerror. AddseparatepermanentWindows
realCLI+MCP stop-before-higher-move/recreate/restart positive, smallfreshfixture
soearlierwarmphaseisnotmasked:actualprocessstop/reapbeforemutation,newdiscovery
andsubsequentdistinctpolicy/sourceedits,allauthored/identitypreservation and
normalMCPread/explicitrefreshsemantics. AddWindowsCIexactselector,inventorynew
testrow/count226 andpairednormalizedsourcehashafterformatting. No countingOs5as
success, platformskip, backendrelaxation, orclaimnearesttestproveshigherautomatic
recovery. ExistingacceptedWindowsSpecalreadyrequiresstop/move/restart; strengthen
explicitrealCLI/MCPproofifneeded. 2.2remainsopenfornativeinstalledacceptance.

HOLDoptimizedfreeze untilthisboundedfixture/public-lifecycle slice settles.
Thencommitslice,build/freezeexactcommitteddefaultrelease+newcost/upgrade/adapters
(includingnewflood),cost/taskmetadata+rebinding BEFORE single NORMALcleanpush,
requiredhooks/freshnative/securitygates. No currentartifactclaimfromhistoricaldff.
Freezehelperavailable /tmp/projectatlas-739-freeze-native-rename.py; CLONEtofresh
policy-fenceorfinal namespace, don'toverwriteimmutableoldfolder/receipt. Itneeds
CargoJSONartifactlog andbuildlog withmatchingprefix; includes140trackedcrate
inputs+Rustcosthelperlinks/frozenCLI. No newbuildstartedyet.

C431SOURCE41488165395stillLIVE: Rust/repository/diagnostics/Linux/ARMfullSUCCESS;
WindowsFAILasabove;MacIntel125425271172latepluginupdateproof(stillLIVEat18:09Z).
LinuxinstalledreaderPASSprovespriorENOSPCfix; WindowsandMacIntelbothpassed
resource/installedreaderbeforelatersteps. PreserveSAMEremainingrun,don'trestart
forobstimeout. Current publicnewcode/securityreviewscomplete; Codecleanfalse/
false,securitynewfenceconfirmed;freshfull33intakeavailablebutnoaggregateverdict
claim. 8genuinefeedbackthreadsOPEN; alltaskstate/issuebodycurrent. Source2d16
fixalreadyapprovedindependentlybounded; no overallacceptance.

PublicPRbody+securitysummary6563449602 updatedappandexactreadbackfornewfence
followup, prior32assessmentexplicitlydoesnotcovernew1 orclean-headclaim.
GoalACTIVE739onlybefore738->708->715RC1;RC1absent050Latest;parked/privatework
preserved, stablepromotionrequireslaterinstruction.


## Reviewed public policy lifecycle coverage — ready for source commit

LocalHEAD2d16(publicc431), currentdirtycoverage slice is primary's #739work.
Luna741PLAN+actualcode/E2E/diagnostics/inventory APPROVED for nextnativepublication;
no sourceblocker. ActualWindowslegalwarm+stoprestartproofstillrequired.
Oldc431SOURCE41488165395 TERMINALFAIL: Windows125425270659lateknownE2Efixture
Os5; MacIntel125425271172+Linux+ARM/Rust/repository/diagALLSUCCESS. Do not keep
pollingoldrun. Mainremains6927ed0ca64788db6947b4d43eafce29aac5ed61.

Warmreplace_policy nowcfgWindowsnearestgit_config.parent(), otherplatformshigher
policy/current unchanged. NO product/runtimechangeinthisslice. NewPORTABLE
external_policy_ancestor_restart_preserves_cli_mcp_observation (notWindowscfg)
fullyreapsCLImaxcycle1orMCPshutdownbeforeactualhigherancestorrename/recreate;
freshCLI/MCPprocessesdiscovercurrentpolicyanddistinctlaterpolicy/sourceedit;
CLI8cycles/notify,correctMCPreadguidance+explicitrefresh,allauthoredpurposes/
identity/revision/externalDB preserved. ExistingMCP leg provesfreshprocess/session
reset,notassertedreleaseofaspecificMCP-heldnativehandle; nativehandleunitmatrix
remainsWindowsOSboundaryproof. Exacttestselectoraddedall4nativeCIjobs,same5mincap.

Testsper86098: restartPASS9.40s +existingwarmCLI/MCPsource/rootreplacementPASS
39.92s onLinux. Initialinventory83selectorappendorderwaswrong, correctedto
workfloworder; historical440symbols/facetsunchanged. Initialeprintln diagnostics
failedClippyprint-stderr; correctedwithoutallowtoPRIVATEthiserrorfixtureerror
recordsretainingactualioError/sourcechain+operation/path. Botholdreplacementand
newrestartCLIerrorbranchesretainoriginalcause/stdout/stderr. No dependencychange.
Finalsourcehash cee7788173fc1ff460b0f234c1330beca53e02da46438e6d1b190c4a2cd02847
pairedinJSON/source_contract;226tests/83selectors. Sharedfixturepathsconstants
keepidenticalbytesandstrictstrings; active/dormantpurposecontractsunchanged.

FINAL37064TERMINAL0:CLIalltargets/allfeaturesClippy,strictstrings,3inventory
contract/tampertests,strictOpenSpec,fmt,diffPASS. Diagnostic-onlylastchangesdon't
alterpassingpositivecases; optimizedexactartifactadapterproofwillrerunboth.
No localbuild/testprocesslive. Preparedfreshfreezehelper /tmp/projectatlas-739-
freeze-policy-fence.py;immutablefolder external-policy-739-policy-fence-release
ABSENTbeforebuild (mkdirrefusesoverwrite). Sourcecommitnextthenlockeddefault
releaseCargoJSONbuild ->freeze ->affectedcost/upgrade+CLI/MCPinclflood/restart ->
finalcost/taskmetadata+140inputrebinding ->SINGLEnormalcleanpushrequiredhooks.
No freezesnapshotbuilt yet. Historicaldff/c431sourceproofnotcurrent2d16+coverage.
2.1open/1.1checkedonly;2.2/3.1/fiveacceptanceunchecked,8genuinefeedbackOPEN.
GoalACTIVE739onlybefore738/708/715RC1;stable050Latest andparked/privateworkpreserved.
