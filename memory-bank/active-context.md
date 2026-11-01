# Active RC1 checkpoint — 2026-11-01

Goal: normally merge/verify closure of one RC1 issue at a time, then non-draft
v0.5.1-rc1 prerelease Latest=false and independent download/install verification.
Order739→738→708→715. Stable050 staysLatest; stablepublication/milestoneclosure
requirelaterinstruction. Primary codes; independentread-onlyLunaMax reviews.
Preserve parkedwork and everyprivateAtlasDB; no reset/force/admin/hookbypass.

## Current owner and live hosted work

Exact checkout .worktrees/issue-739, branchfix/external-excludes-739.
Public PR745 OPEN/READY at bad7fb3b293a390c19273de8f0f35c0c0b51dee9,
base main6927ed0, milestone37 v0.5.1-00, soleowner739 (Refs, not Closes).
New primary-owned runtime root-gap fix/design/costdocs/memory await normalcommit/push.
Read git for the next actual HEAD. OpenSpec reconcile-external-git-excludes,
schema spec-driven; task1.1only checked (1/4),2.1/2.2/3.1/all5acceptanceopen.
LiveissueWhy/WhatChanges updated for newgap rootcause and readbackmatched.
Strict OpenSpec/fmt/strings/plannedIssueOps local4/remote4/checked1 PASS.
CLI ~/.npm/_npx/21a1280729add0a7/node_modules/.bin/openspec.

Normalpush94433 forbad7 TERMINAL0:1773PASS0FAIL12ignored34suites,lockedRust/docs,
repository/localAtlasgates, no bypass. /tmp/projectatlas-739-native-spelling-push-summary.json.
Oldsource CI41353486619 STILL LIVE at lastpoll; watcher execsession98043,
log /tmp/projectatlas-739-bad7-ci-watch.log; neverrestart/cancel on timeout.
Rustjob124988531739 TERMINALFAILURE: public rootreplacement test failed during
short rename gap (55PASS1FAIL navigation suite); exactlog /tmp/projectatlas-739-bad7-rust.log.
Linux124988531704 wholePASS incl external-policy step. MacARM124988531722,
Macx64124988531733, Windows124988531890 stillrunning, ownedstep pending at lastpoll.
Repository124988531707 andWindowsdiagnostics124988531771 PASS.
Snapshot /tmp/projectatlas-739-bad7-ci-snapshot.json. Preserve nativejob proof
rather than restarting; next normalpush eventually supersedes oldsource automatically.
Earlier f8 CI41344940865/watcher50183 TERMINALFAILURE, notlive/restartable.

## New rename-gap root cause and fix

wait_for_index_event_with_continuity discovers rootpolicy for the first affecting
event BEFOREdebounce. A prepared rootreplacement still has two rename syscalls;
an event can arrive in that namespacegap. A child event can precede rootnotification.
Early RootNotDirectory was fatal even though replacement settles before refresh.
Do not weaken the realCLI/MCP scenario or restore testhelper fallback masking.

At the batch owner, catch ONLY exactcapturedroot RootNotDirectory, or exactroot
FsErrorIo NotFound/NotADirectory (is_dir→canonicalize race). Retainfullscan and
PolicyWatchSelection recursive-rebind uncertainty. Drain originaldebounce/native
results, preserving nativeerrors/disconnect/overflow; stop redundantclassification
oncefullscan required. Existing outerplanreload→rebind→freshreconcile validates
settled root before publication. Initial/one-shot/persistentlyinvalid roots still
fail, otherpolicy/parser/IOfailures propagate. No newretrybudget/pollfallback,
callbackI/O/unconditionalwarmrewatch/schema/rootidentity/graphfact change.
DER25/semantic23 sufficient; bothindependentreviewers confirmed bounded scope.

Causalactualhelper old2FAIL /fixed2PASS:
/tmp/projectatlas-739-root-gap-{negative,positive}.log. Root/child-first events,
fullscan+rebind, overflow, queuednativeerror/disconnect preserved. Thirdcontrol
nonregular.ignore directory remainsInvalidInput/no rebind; regularfile restores
ordinarysourceevent/no fullscan. Allwatch25PASS,notify4PASS, CLIalltarget/allfeature
ClippyPASS,strictstringsPASS,fmtPASS; terminalgate1350.
/tmp/projectatlas-739-root-gap-{watch-tests,notify-tests,clippy,strict-strings}.log.
Luna741 approved source/DER25 and finalartifact/proof/doc binding; Luna731 found
no boundedsource/securitydisposition blocker. Wholeissueacceptance remainsconditional.

## Current frozen artifact and proof

~/.cache/projectatlas-benchmarks/external-policy-739-root-gap-release:
CLI985d7b123e07ffbe40995611edb46c9f15c913309aa221c7f5e935157b9a0ccc,
69,806,664bytes. Current139inputmanifest
7e32535a26580f2da683b1be374cf97a9fecf8b944c466f0a3c239360a817e7f.
All current savedsource hashes independently match; Rustsource FROZEN after proof.
Fresh receipt records actualdirtybad7 source/buildlogmtime/freezetime/uniquerlibs;
no historicalprovenance copy. Normal locked default releasebuild35544 TERMINAL0.
FS helper68663dbeaf98788375eb33213c0b472b4e0fcc3c3bd5da176089e79113aebb72
relinked against actualdefaultrelease libs and BYTEIDENTICAL to measuredhelper;
reuse ONLY18FSmeasurements from /tmp/projectatlas-739-root-alias-policy-cost-proof.json.

Failfastchain76995 TERMINAL0; everyoutputchecked: optimized external-policy1PASS
1.31s, rootreplacement1PASS7.98s, upgrade4/4PASS, CLI18samplespassedtrue.
/tmp/projectatlas-739-root-gap-{external-policy-e2e,root-replacement-e2e,
upgrade-proof,cli-cost-proof}.*. Allidentity/approvedpurposes/noop/failure/rollback
controls preserved; earlyzero deadline not midpublication cancellation claim.
Durable docs/benchmarks/external-policy-cost.{md,json} now985/current139manifest,
18FS+18CLI/4upgrades verified byLuna741. Binary+349704bytes/+0.50348%; ordinary
CLIinit289.6→294.6ms/noop49.5→48.1ms;10krulesnoop69.6→97.4ms (+27.8ms).
No general speedup/#708SLOwaiver. Canonicalroot costfixtures do not quantify alias
or transientgap branches. Old10c/bd86/9c receipts immutableHISTORICAL, notcurrentCLI.
Acceptedmain baselineadfe95f1ae05c4a3bbbf1776a460e754d6fc016468d51238558611218b0e2b3c,
stable050aa6188e11a05c54f4a69680e74055a02bdae4db7503aa0144d5e7157316ab485.

## Security at committed bad7 (not yet the dirty repair)

CodeQL41353483020 TERMINALSUCCESS all7languages. Rust2073052996 has13results,
Actions2073011002 has1, other5languages0. Codexcode/security BOTHCOMPLETED bad7;
codeflagsfalse/securityflagsnull; no newpostedfindings. Sevenexistinginline threads
resolved; new412/413 have noinline at lastread. Never claim cleanscan/suppress/dismiss.
Primary staticallytriaged all14 inputs and rendered results; Luna731 independently
reviewed dispositions. /tmp/projectatlas-739-bad7-complete-triage-result.json.
10existingRust fullfacts/flows equal after exactunchangedGitline mapping/numeric
SARIFreferences/relatedpresentation normalization; duplicates retained. 409overlaps
changedaliasbranch and was SEPARATELYREASSESSED, not falsely declaredidentical.
Immutableplanroot/DBaggregate counterevidence still holds. 412/413sourceONLY private
CWD-created TempDir rootspelling test; fixedchild targets reach metadataexists/is_dir,
not targetcontentread/write. Does not claim everymetadataread followscontainment.
Actions411 fullresult+2flows identical and workflow/producerbytes unchanged; eventSHA
and documentedcache scope counterevidence retained, not readonlytokenalone.
/tmp/projectatlas-739-bad7-{rust-reconciliation,new-rust-triage-result,
actions-triage-result,alerts,codeql-analyses}.json; fullSARIFbad7-{rust,actions}.sarif.json.
Public dispositioncomment6563449602 updated/readbackexact; PRbody candidly records
hostedrootgapfailure/localrepair. Newsource needs freshscan/review reconciliation.

## Existing accepted mechanisms and continuity

Canonicalgix-config0.62.2 defaults-off/sha1, ignore0.4.34;1MiB/file4MiBtotal,
directconfigprecedence/no implicitincludes. No-Git/hierarchy/pruning/operator paths
preserved. FSrootalias API repair captures optionallexicalroot/rebases matching
prefix beforeAtlas/native matching withouttargetI/O; fullcanonicalrows unchanged.
APFSfixture onlydirectEILSEQ falls backrealUnicodeGit/worktree; typedNonUtf8Path
alwayschecked. Windows rawOsString fixture preservesParentDir and normalizedcontrol;
productioncallbackfilterunchanged. FS49/policy10/observer36 previouslypassed Linux;
currentbad7 Linuxhostedpassed; newsource fullnative/4platformproof stillrequired.
Queue1024/registrations256/convergence3; cold selectedparent/rootcoverage, ingress
fence; unresolveddocument pages256/freshpublicationpolicy/originaloptions; no warmrewatch.
Shared #738 nativeprefix guard covers cached/probe/staging underoriginalcontrol;
#738 retains ownfullacceptance. Sourcefacts DER25,semantic23; no DBreset.
Causal priorroot/ingress/publication/document controls unchanged and preserved:
/tmp/projectatlas-739-{root-public,ingress}-causal-control.json,
policy-publication-{negative,positive}.log, document-probes-{negative,positive}.log.
Inventory223selectors. Stable050 index5 COMPLETE after mandatorypolicy_drift refresh
following candidate nativeindex; preserveprivateDB. No localbuild/index/proof lease
remains now. Low.cargo/.githooks purposes approved.

## Next and parked work

Normalcommit with sole(#739) owner, normalpushwithhooks (captureactualhandle/PID).
Duringgate keep selectedcheckoutCLEAN and updateintegrationmemory only. Verifynew
artifact/commit-source binding, allcurrentCI/jobs/botthreads, then task/acceptance
sync onlyasproven, Closes739 onlywhenready, normalmerge745, native739CLOSEDreadback
BEFORE738. No newissuecode in parallel, stable publication/milestone closure excluded.
#731/PR737 closedmerged. #741/PR743 closedmergedmain6927ed0 Nov1 16:32:45Z;
reviewed5dda tree49893e53b41eb98bd3e641a200dab26d9981e941 equalsmerge,
/tmp/projectatlas-741-closure.json. Do not reopen/rework741.
#738 parked8814d6d+memorydirty preserved; reconcile sharedguard/spec/contract without
blindduplicatechanges. #708/PR728 publicc65dfed/local587ecbe+memorydirty preserved;
warm1201.127FAIL/focused1.031526partial, noSLOwaiver. Known708task2.3livechecked/main
unchecked drift reconcileswhen708accepted, not byuncheckingprivateprogress;715global
IssueOpsmustpass. Integration release/0.5.1 retains private715spec edits.
Queued CodexCIgapreview /tmp/projectatlas-codex-ci-followup.json belongslaterowner;
PR744 Dependabot outsideRCmilestone, final715 evaluatescurrentdeps/security.
FouropenRCissues739/738/708/715. NoRCpublished. Originalbackup pointer
/tmp/projectatlas-739-pre-main-snapshot-path.txt preserved.
