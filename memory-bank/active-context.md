# Active RC1 checkpoint — 2026-11-02

Goal: normally merge and verify closure ONE issue before next:739→738→708→715,
then non-draft v0.5.1-rc1 prerelease Latest=false and independentdownload/install
verification. Stable050 staysLatest; stablepublication/milestoneclosure require
laterinstruction. Primarycodes; independentLunaMax reviewsreadonly. Preserve
parkedwork/privateDBs; no reset/force/admin/hookbypass.

## Current owner / source / tracker

Exactroot .worktrees/issue-739, branchfix/external-excludes-739.
Public PR745 OPEN/READY, base main6927ed0, headbad7fb3b293a390c19273de8f0f35c0c0b51dee9,
milestone37 v0.5.1-00, soleowner739 Refs/notCloses. Local107ad7e rootgapfix and0fba5b0
observerfixturefix committed/unpushed; finalWindows/nativefixture packet readyfor
normalcommit/push. Readgit for nextactualHEAD. OpenSpec reconcile-external-git-excludes,
spec-driven; only1.1checked (1/4),2.1/2.2/3.1/all5acceptanceopen. LiveWhy/WhatChanges
updated/readback for ALL findings, nativegraphunchanged. StrictOpenSpec/fmt/strings/
plannedIssueOps local4/remote4/checked1PASS. CLI ~/.npm/_npx/21a1280729add0a7/node_modules/.bin/openspec.

No liveLOCALbuild/index/proof lease. SourceRust FROZEN after currentartifact/proof.
Oldnormalpush94433 bad7 TERMINAL0:1773PASS0FAIL12ignored34suites, lockedRust/docs/
repo/Atlasgates. Pending107adpush91354 was INTENTIONALLYTERM143 afterconfirmedMac
failures, nottimeout; all7capturedprocessesgone andremoteheadbad7 readback. Incomplete
receipt /tmp/projectatlas-739-root-gap-push-process.json; do NOTclaimfullpass/restart.

Oldbad7 CI41353486619 now TERMINALFAILURE, watcher98043 TERMINAL1 at22:47:16ZNov1.
No oldhostedjob remainslive. Results individuallyinspected:
- Linux124988531704 wholePASS; ownerstep49FS/10policy/36observer/1external/1replacement.
- Rust124988531739 FAIL55/1 navigation: CLI rootreplacement sees shortrootgap.
- MacARM124988531722 FS49/policy10PASS,observer34/2FAIL delayed/stalereaderinvalidation.
- MacIntel124988531733 FS49/policy10PASS,observer33/3FAIL sametwo plusmidqueryretry.
- Windows124988531890 FS42PASS,policy7/1FAIL rawverbatimparent inputOS123InvalidFilename.
- plan/repository/windowsdiagnosticsPASS; verifyFAIL. Logs/snapshot
/tmp/projectatlas-739-bad7-{linux,macos-arm64,macos-x64,windows}{,-policy-step}.log,
-bad7-rust.log and-bad7-ci-snapshot.json. All failures accounted for below; no restart
or cancellation hides them. Oldf8CI41344940865/watcher50183 alsoTERMINALFAILURE.

## Reviewed root causes / changes / causal tests

FSrootspelling API repair alreadybad7: optionaloriginallexicalroot prefix rebase,
canonical/relative/unrelated targets retained,no targetI/O. Emptyrootreject/'.'valid;
fullcanonicalnodes unchanged. APFS onlydirectEILSEQ fallbackrealUnicodeGit/worktree,
alwaystypedNonUtf8Path. BothMac FS49PASS confirms formerFIFObypass/APFS failuresfixed.
CurrentDER25/semantic23 remains sufficient; no schema/graphfact/rootidentity change.

ActualCLIwatch rootgap fixed107ad: firstaffectingevent discoverspolicy BEFOREdebounce.
A root or child event canarrive between the two replacementrenames. CatchONLY
exactroot RootNotDirectory/rootIoNotFound|NotADirectory, retainfullscan+recursive
rebind, drainoriginaldebounce/errors/disconnect/overflow withoutredundantclassification.
Existingplanreload/rebind/freshreconcile stillvalidates settledroot beforepublication;
persistentinvalidroot/otherpolicyerrorsstillfail. No newretrybudget/pollfallback,
callbackI/O/warmrewatch. Actualhelper old2FAIL/fixed2PASS,regular/nonregularcontrol;
watch25PASS/notify4PASS/CLIClippy/strings/fmt. /tmp/projectatlas-739-root-gap-*.log.
Luna741/731 approved boundedproductionfix, notwholeissueacceptance.

Macobserver failures are TESTFIXTURE mismatch: indexed_project returnedrawroot/source
whilebindingregisterscanonicalroot. Pinnednotify8.2FSEvents canonicalizesregistration
andprefixfilters deliveredpaths; raw/var injection cannotreachconsumer under/private/var
registration. Apple does NOTpromise universalcanonicalcallbackspelling. Returnsource
fromplan.root, retainexplicitDBspelling/operatoraliases. Existingfixtureconsumersaudited,
intentionalWindowscase/displaytests unchanged. NewUnixselectedsymlink mutationfixture
old0PASS1FAIL/fixedPASS (qualifiedselector; -empty-selector.log explicitlyNOTproof).
NewREALnativepositive writesviaselectedalias andcanonicalroot to DISTINCTleaves,
receivesexactnewleaf, assertsactualclassifier sourcepath; no syntheticrepublish or
fullscanfallback. All38observerPASS incl3Macfailurecases; Clippy/strings/fmt.
/tmp/projectatlas-739-observer-fixture-{negative,positive,clippy,strict-strings}.log.
Luna741 approvedfixture/spec. No productionnormalization added for impossibleinput.

Windowsfailure TESTFIXTURE: rawOsString retained.. butbasewasverbatim\\?\;
Windowsrefuses thatsyntax beforefiltering. TEST-ONLY fix uses ordinaryabsoluteDisk/UNC
base forrawparent spelling; retainnormalizedverbatimpositive andgenericunsupported
verbatim..refusal (nohardcodederrornumber). gixproducer nowreads parser-selectedrules
andexactblocked.rsbytes, notmerelystring-preservation. DisplayconversionWindows-only;
UnixnativePathBuf retained. ExistingFSrootspellingtest undernonUTF8CWD old1FAIL,
fixed1PASS; /tmp/projectatlas-739-nonutf-cwd-{negative,positive}.log. Windowsaliasfixture
uses separatetempbase whilecanonical/relativecasesalwaysunderCWD (long/otherdrive
cannoterasecase); comparefullnodevectors against eachphysicalcanonicalroot.
FS49PASS/policy10PASS,CLI+FSalltarget/allfeatureClippy/strings/fmtPASS. Redundantclone
caught/fixedbyFSClippy; no suppression. /tmp/projectatlas-739-win32-parent-{policy-tests,
clippy,fs-clippy,strict-strings}.log, native-spelling-fs-tests.jsonl.
Luna731 approvedall3testfiles; realupdatedWindowsproof stillrequired.

## FINAL frozen artifact and complete affected proof

~/.cache/projectatlas-benchmarks/external-policy-739-win32-parent-release:
CLI7eaa11bab7d95b8310f0e70de0f016e757fe83df2f15ead1e0ecd7b151ce965e,
69,806,784bytes;139inputmanifest8b177fe561e8c40e8bb3fc1066f0146521a388cc1d5644a6bf838a800197b24b.
All current139savedsourcehashes match; freshactualreceipt. Normaldefaultreleasebuild
54822TERMINAL0. FShelper68663dbeaf98788375eb33213c0b472b4e0fcc3c3bd5da176089e79113aebb72
relinkedactualrelease RLIBs/BYTEIDENTICAL; reuseONLY18FSmeasurements from
/tmp/projectatlas-739-root-alias-policy-cost-proof.json. Microdriverbe8b7ab61c0230a7...

Failfast5310TERMINAL0; eachoutputchecked: optimizedexternal1PASS1.27s,
rootreplacement1PASS8.03s, upgrade4/4PASS, CLI18samplespassedtrue.
/tmp/projectatlas-739-win32-parent-{external-policy-e2e,root-replacement-e2e,
upgrade-proof,cli-cost-proof}.*. Allidentity/approvedpurpose/noop/failure/rollback
controls preserved; earlyzero timeoutnotmidpublicationcancellationclaim.
Durablecostdocs.{md,json} now7eaa/8b177: binary+349824bytes/+0.50366%, ordinaryCLI
init291.5→289.1ms/noop49.5→50.2ms;10krulesnoop72.4→99.1ms (+26.7ms).
No general speedup/#708SLOwaiver; canonicalrootfixtures notalias/gapbranch timings.
e71/985/10c/bd86/9c immutableHISTORICAL, notcurrentCLI. Baselineacceptedmainadfe95f...
andstable050aa6188... unchanged. Fullfreshnormalnative/4platformgates stillrequired.

## Security dispositions apply to COMMITTED bad7, notyet thisnewpacket

CodeQL41353483020 SUCCESSall7: Rust2073052996 has13,Actions2073011002 has1,other5zero.
Codexcode/securitybothCOMPLETEDbad7/codeflagsfalse/securityflagsnull/no newpostedfindings.
All7existinginlinethreadsresolved; new412/413noinlineatlastread. Primarytriaged/rendered
14inputs; Luna731 independentlyreviewed. /tmp/projectatlas-739-bad7-complete-triage-result.json.
10oldRust fullfacts/flows match exactunchangedGitlinerelocation/numeric/presentation
normalization(duplicatesretained);409changedaliasbranch SEPARATELYreassessedimmutable
planroot/control, notfalselyidentical.412/413sourceONLY privateCWD-createdTempDirfixed
children→metadataexists/is_dir, no shippeduntrustedboundary/contentreadshown. No claim
allmetadatareadsfollowcontainment. Actions411 fullresult/twoflows/workflowunchanged;
eventSHA/cache-scope counterevidence, notreadonlytokenalone. Publiccomment6563449602
updated/readbackexact. No suppression/dismissal/cleanscanclaim. Reinspect EVERYnewhead
result/review beforeacceptance; tests changed reportedlocations/flows, don'tblindcopy.

## Shared scope and next steps

Canonicalgix-config0.62.2 defaults-off/sha1,ignore0.4.34;1MiB/file4MiBtotal directconfig
bounds/precedence/noimplicitincludes. No-Git/hierarchy/pruning/operatorpaths retained.
Queue1024/registrations256/convergence3; coldselectedparent/rootcoverage/ingressfence,
no warmrewatch. Policyonlyunresolvedprobe pages256,reusesparses/atomicprojection;
freshpublicationpolicy/originaloptions. #738sharednativeprefixguard cached/probe/staging
underoriginalcontrol;738stillownsfullacceptance. DER25/semantic23/noDBreset.
Priorcausalcontrols unchanged: /tmp/projectatlas-739-{root-public,ingress}-causal-control.json,
policy-publication-{negative,positive}.log,document-probes-{negative,positive}.log.
Inventory223selectors; stable050index5COMPLETE/privateDBpreserved/.cargo/.githookspurposes
approved. Refreshonlytyped/affected guidance; no concurrentlocalbuild/index.

Next: normalcommitsole(#739), bindcommitted139inputs/currentartifact, normalpushwith
hooks/actualprocessreceipt; keepselectedtreeCLEANduringgate, updateintegrationonly.
Fresh4platformCI/scans/reviews, task/acceptancesync onlywhenproven, Closes739 onlyready,
normalmerge745 andnative739CLOSEDreadbackBEFORE738. No force/admin/hookbypass.

## Accepted closures and parked work

#731/PR737 closedmerged. #741/PR743 closedmergedmain6927ed0 Nov1 16:32:45Z;
reviewed5dda tree49893e53b41eb98bd3e641a200dab26d9981e941 equalsmerge,
/tmp/projectatlas-741-closure.json. Do not reopen/rework741.
#738 parked8814d6d+memorydirty preserved; reconcileguard/spec/contract withoutblindduplicates.
#708/PR728 publicc65dfed/local587ecbe+memorydirty preserved; warm1201.127FAIL/focused1.031526
partial/noSLOwaiver.708task2.3livechecked/mainuncheckeddrift reconcileswhen708accepted,
notuncheckingprivateprogress;715globalgatemustpass. Integrationrelease/0.5.1 private715
complete-v051-release-acceptance design/proposal/spec.md edits untouched.
QueuedCodexCIgapreview /tmp/projectatlas-codex-ci-followup.json belongslaterowner;
PR744Dependabot outsideRCmilestone,final715evaluatecurrentdeps/security.
Four openRCissues739/738/708/715,noRCpublished. Backup
/tmp/projectatlas-739-pre-main-snapshot-path.txt preserved.
