# Active RC1 checkpoint

Older detailed checkpoints are in Git a5ac546 and predecessors. Live source,
tracker, checks and named proof artifacts remain authoritative.

## Goal and ownership

Goal ACTIVE: finish/normalmerge/nativeclose739/PR745, THEN738, THEN708/PR728,
THEN715 RC1 acceptance/publication/independent install. v0.5.1-rc1 must be non-draft
prerelease Latest=false; stable0.5.0 stays Latest. Stable promotion,715stabletask3.2
and milestone closure require later instruction. No permission pending. Primary
writes; independent Luna Max reviews read accepted spec and customRust/cognition/
workspace skills (DB/SQLite when applicable). Preserve parked/private work.

## Exact current state

Selected .worktrees/issue-739, branch fix/external-excludes-739.
Local SOURCEa5ac54628aac866549841b5a083ac45526d63f3f; publicPR745 still
6f9f63cc74a677db134ba8e80b28b216a26a3551 OPEN/unmerged. Main6927ed0ca64788db6947b4d43eafce29aac5ed61
last verified. Current owned metadata-only dirt: costMD/JSON,proposal/tasks,memory.
1.1/2.1/2.2 CHECKED local/live;3.1 and ALLfiveacceptance UNCHeCKED. Firsttwo
mitigationschecked; thirddepends2.2+3.1soUNCHECKED. Luna741 code+packetapproved,
including task2.2 after requiredhandoff recorded. No wholeissueacceptance yet.

Public6f9 normalpush66406/wrapper896291/git896457 TERMINAL0 at19:38:42ZNov2:
1819PASS/0FAIL/12ignored across34targets; requiredworkspace check/Clippy/fmt/docs/
dependency/security/strings/OpenSpec/IssueOps/repositoryscan/lint PASS. Source
CI41511865530 TERMINALSUCCESS ALL4native/Rust/repository/diagnostics/verify.
Nativejobs Linux125505570151/ARM125505570270/Intel125505570278/Windows125505570297.
HostedRust125505570248 logs confirm1819PASS inclflood; metadata41511995173SUCCESS.
Othermetadata runsfromPRbodyedits mayexist; usecurrentPRcheckstate afternewpush.
Alloldpublicsource jobs areTERMINAL; do notrestart/poll aslive.

Exactnative5selectors x4 all20PASS asserted in
/tmp/projectatlas-739-6f9-native-owner-proof.json; native-{linux,arm,intel,windows}.log.
Cases: external_git_exclusions_agree_across_cli_mcp_and_refresh;
observed_mcp_reconciles_document_only_target_changes;
source_root_replacement_preserves_cli_mcp_external_bindings;
external_policy_ancestor_restart_preserves_cli_mcp_observation;
runtime::tests::polling_handoff_preserves_active_control_and_authored_state.
NativeWindows legalnearestwarm+stop/reap/higherancestor/restart BOTHpassed.
Do not claim an unperformed higherwarmmove or asserted MCP-heldhandle release;
OSdescendant-handle matrix remains its own boundaryproof.

## Latest test safety correction (locala5, notyetpublic)

After final6f9securityassessment, primaryfound SEPARATE testhelper cleanup bug:
STOPwait could try_wait/reap a deadchild thenCONT through rawPID afterreservation
lost. Harmless LinuxPythonprobe confirmed SIGSTOPzombie succeeds/reap removesproc;
no unrelatedprocess signalled. Fixed at sharedsignalowner: &mutChild, try_wait
immediately beforeeverySTOP/CONT; Some/error returnsBEFOREcommand/PID use; None
keepsunreaped ownership during synchronouscommand. ProgramPath injection onlyin
privateLinux#[test] allows harmless0700witness script (writes$0.called, NOsignal).
AlreadywaitedChild rejects beforelaunch. No globalenv/PATHchange, dependency,
productAPI/schema/contract change. All4signalcallers use thisboundary.

Causalbeforeguard focusedtest FAILEDexactly 'an exited child must be rejected
before launching its signal command', running1test; child-signal-before-fix.log.
Fixedfocused1PASS9.31s; concurrent59PASS/0FAIL/0ignored231.20s,26162TERMINAL0.
55916TERMINAL0: CLIalltargets/allfeaturesClippy/strictstrings/3inventory/fmt/diff/
strictOpenSpec/scopedIssueOps PASS. Newnavsourcehash
fff2c5cee3b67897da68be0ed34052788fd4fee3cbd51314f8a86ee5a5739ee9 pairedinJSON+
source_contract;226tests/83selectors/historical440 unchanged. Luna741 PLAN+actual
source/witnessdiff APPROVED. No localtest/build processlive now.
Logs /tmp/projectatlas-739-child-signal-{focused,concurrent,clippy,strings,inventory}.log.

## Artifact and proof binding

Clean defaultrelease sourcea5 built/frozen4304TERMINAL0(42.57s) at
~/.cache/projectatlas-benchmarks/external-policy-739-child-signal-release.
140inputmanifest5971187e3ee26fc895657e958dabf008f51ec572a0f8e7d8bb2b5f97eaee993e.
CLI0358e60aec4b7a58d5861cf8c0c501091286d33ce71fa78f731939b0c96be55f/70000760B
and FShelper544eb8349e6803c427b11382b32b4297e071336f66b547c465ddc8a339d83fc9
BYTEIDENTICAL to measured76 and later93d. Only2of140inputs differ: navigationtest
and pairedsource_contract. Allsaved/Gita5inputsverified;79750TERMINAL0 fresh
optimizedflood1PASS8.421s incldeadchildcontrol andrealinitial/laterSTOP/CONT,
harness54b63375572345443ad5bafccb3120d381f2fa46656adc38f48c23d5e7c49b53.
/tmp/projectatlas-739-child-signal-optimized.json +release receipt/manifests.

Retain measuredSOURCE76/0358 18CLIcostsamples/6upgrades/2resource/5unchangedadapters;
replaceONLYcorrectedfloodproof, no newcost/upgrade timingclaim. 18FScomponent
samples retain historicalbyteidentityreuse andunisolated overlaplimitation.
Costdocs nowbindsourcea5/597,currentCLI0358, measurement76. Size+543800B/.78293%.
Ordinaryinit310.831→310.468ms/noop48.697→49.047ms;10krulesnoop70.363→98.219ms.
Completeprefix2guides×500links/depth200 cold3.458s+completecoverage/unchangedpub.
Cold1024links/2000rulesmedian.067477s. LocalLinux scopedresults, NOTgeneral/fourOS
SLO; #708performanceveto remains. Neveroverwriteimmutableartifacts/oldreceipts.

Task2.2 exactselector/cost/contracts/upgradehandoff POSTED+EXACTreadback:
715 comment6588173733;708 pointer6588197732 (spacingcorrected/readback).
https://github.com/styler-ai/ProjectAtlas/issues/715#issuecomment-6588173733
https://github.com/styler-ai/ProjectAtlas/issues/708#issuecomment-6588197732
Receiving tasks/source/state untouched. Proposal/live739 linkboth. 3.1/final
acceptance stillrequires allaffected-input gates; commitidentity alone doesNOT
invalidate proven behavior/artifact. IssueOps caught wording 'exact-head proof';
corrected to behavior-relevant/affected-input gates; no gateweakened.

## Security and review state (scopedPUBLIC6f9)

CodeQL41511859128 all7SUCCESS:Rust2080285011=26,Actions2080156021=2,others0.
WHOLE34 normalizedBEFOREverdict:28native+5securityreports+RUSTSEC-2026-0433.
All34 inlineprimary assessed/not_actionable onfixed6f9 and RENDERED34inapp.
Historicalgenuine resources/advisory remain explicitlyconfirmed/FIXED, notfalse
positives. No suppression/dismissal/exhaustivecleanrepo/generalavailabilityclaim.
FullREST/SARIF/instances: /tmp/projectatlas-739-flood-phase-exact-*.
Fullresult flood-phase-complete-triage.json; rendered-triage.json has12explicit
sinkexcerpts for1200charUIcap, fullclaims retained. Successreceipt
flood-phase-triage-render-success-receipt.json. Staticphase COMPLETE/LEFT before
newtestcleanupfix; no repoedits/tests/builds/apps/agents duringthatphase.

27continuingnative completefacts/orderedflows equal afterexactlinemapping and
verifiedrendererID reconciliation (explicitURIs/allregions/messages/rules/
properties kept; relatedlabelIDs/order onlywhenexhaustiveidenticallabels,
anonymousmetadata orderpreserved). 12of13Rustfilesunchanged;2709citedregions
mapunchanged.85priorguardanchors16sources mappedwithoutgap. Artifacts
flood-phase-{native-reconciliation,reported-source-map,assessment-location-map}.json.

New419 is privateLinux#[test] read_link(DirEntry::path()) underliteral
/proc/<decimalownedChildID>/fd; compareslinkvaluetoinotify, NEVERfollowstarget.
No productrequest/pathentrypoint, Cargo testnotshipped. Reply4447237134to4447070010
EXACTreadback; threadPRRT_kwDOQyFgJ86y1sD7 RESOLVEDtrue. Publicsecuritysummary
6563449602 current34 rewrite EXACTreadback. Codexcode6f9 completedfalse/false;
securitycompletednull/null +allRESTcomments/reviews showNOnewCodexreport.
Do NOTassumethisassessmentcoversfuturea5/newfindings beforefreshintake.

8GENUINEfixedthreads stillOPEN pendingfinalproof/replies/resolution:
4437820780 doconly;4438294526 controlledfullfallback;4438307216policyresource;
4439249685 fullnoopdocdrift;4440644526prefixresource;4441758314ordinaryancestor;
4441784180Unicode;4445386019fence(threadPRRT_kwDOQyFgJ86yxwfI).
Twoearlierfalsepositive threadsresolved(CodeQL417reply4444055416,
ancestorpolicyreply4444477324); new419thirdexplicitdisposition above.
CurrentPRbodyaccuratelyqualifiesunpublishedcleanup, published6f9proof,3.1pending.

## Next action

Finalownedmetadata review APPROVED byLuna741; behavior2.1/2.2complete/handoffverified,
NOTwholeissueacceptance. FinishcheapOpenSpec/IssueOps/strings/fmt/diff, commitONLY
costdocs/proposal/tasks/memory; verify140saved/Gitinputs unchangedvsfrozena5.
NORMALcleanpush using /tmp/projectatlas-739-child-signal-push{.py,.log,-session.json};
externalreceipt isprocessauthority, keepselectedcheckoutcleanwhilelive. No bypass,
force/admin/selfapproval/DBreset. Normalplanner sees sharedtestsupport change,
sofullmandatorysourceproof applies; reuseunchanged measurements only.

Afterpublish verifyactualremoteHEAD, currentrequiredchecks/bots; retrieveALLnew
nativeCodeQL+securityreports, normalizewholecollectionBEFOREverdict. Native419
maymove/newprivate-commandalertmayappear: inspectactualfullflows, neverassume
counts orfalsepositivity. Statictriage forbidsrepoedits/tests/builds/apps/agents;
leavephasebeforefixes. 6f9inputtemplates arestartingpoint, notfuturecleanproof.
Thenindividually address/resolve8genuinefeedback withrelevantpassingproof,
complete3.1/fiveacceptance withindependentfinalreadiness, normalmetadataaffected
gates/reuse, normalmerge+native739CLOSED BEFORE738→708→715RC1.

## Tools and parked work

source /tmp/projectatlas-rc1-test-env.sh; sharedtarget integration/target;TMPDIR
~/.cache/projectatlas-test-tmp. Runtimeunits --bin projectatlas NOT--lib;strings
cargo projectatlas-lints strict-strings. OpenSpec cachedCLI under
~/.npm/_npx/21a1280729add0a7/node_modules/.bin/openspec. No siblingcompiles duringCLIproof.
Atlas050 exact739 policy_drift/fullrefresh aftercandidate26; preserveprivateDB/
authoredstate, no downgrade/reset/scan fororientation; useboundedcurrentsource,
no stalegraphclaims. Mainprotectedverify/pr-state App15368 strict+conversation.
731/737 and741/743 MERGED/CLOSED.738 parked8814d6d+ownmemory;708 publicc65dfed,
local587ecbe+ownmemory, fullwarm1.127FAILED/focused1.031526PARTIAL noSLOwaiver.
Integration715threeprivatespecedits preserved. Later715evaluate744outsideRC.
