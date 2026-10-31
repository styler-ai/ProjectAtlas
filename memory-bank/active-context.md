# Active context

## Goal and serial owner

Continue until v0.5.1-rc1 is published and independently verified: non-draft
prerelease, Latest=false; stable v0.5.0 stays Latest. Stable publication and
milestone closure require later instruction. Only #741 / PR743 is active; normally
merge it and verify BOTH CLOSED before another issue. #731 / PR737 are CLOSED
on accepted main a5b1bebcaa79c8386b68e9c247eff73aa11e5e15. Dev is retired;
archive/dev-2026-10-31/tag retained; #720 CLOSED/not_planned. No RC exists.

Selected root .worktrees/issue-741, branch fix/generated-state-741. PUBLIC/local
baseline 8d698c3684660343efe44bc9b9ce7dd695c3a79c; current dirty source below is
approved by independent Luna Max for NATIVE publication only. No local execution
lease. PR ready/notdraft, base main, milestone37, Refs741. Tasks1/5 (only1.1), all
acceptance OPEN. Restore sole Fixes741 only at final readiness.

## Published and immutable evidence

8d normalpush32856 TERMINALPASS782.69s/34reported suites/1730pass/11ignored;
PID/PGID1311561 gone. /tmp/projectatlas-741-security-probe-pinned-native-push.*.
Immutable86109 TERMINALPASS, unique8d artifact, binarySHA
 ddad35d8b81b900c1c0ccdf904cdf7fd26b2f77e54d8b6116ca87104508e93e6;
/tmp/projectatlas-741-security-probe-pinned-debug-path.txt and adjacentreadonly
source-binding.json bind138inputs INCLUDINGWindowssecuritymodule. Actual CLI/MCP
admission4.57s,writer5.13s,registered9.19s,five050upgrade/refusal/retrylayouts5.88s
PASS; identity,ALLapprovedpurposes,config andallboundbytes unchanged. This covers8d,
not subsequent dirty source. Newfreeze must use NEWhead/prefix to avoid collision
when onlyWindows changes leave the Linux binary hash unchanged.

Earlier diagnostic attempts:18462ec normalpush60016 terminalFAIL253.97s (old CI
needs literal,51deliverypass/1fail/2ignored). 716b5f3 retry79652 controlledstopped
AFTER3hard inventorydigestfailures, terminal-2/130.29s; session1247963 andowned
orphanfixture1308137 gone. Both currentdeliverypins now
5b567030be7b54cab74f8cd63482bf0f4b00d4173a400a28beafdc398d6b604b; selectors/counts
unchanged, Luna independentlyrecomputed. All3inventorycontrols andworkflowtestPASS.
No bypass/force used. No e2e_delivery/pin changes after8d.

## Exact native Windows findings and current correction

SOURCE41249974092 on8d: plan124681256720/repository124681289708PASS;
Windows124681289695 terminalFAIL21pass13fail; Rust/Linux/bothMacs stillactive at
lastread. prstate41249972612PASS. Previous e222 SOURCE41233463389 passed repository,
Rust,Linux,bothMacs, including actual APFS/APFSX/HFS+/HFSX ownership/ACL/CLI/MCP.
Windows root123 fallback and corrected existing-object take-ownership controls PASS.
Newhead still needs exact hosted/native proof before acceptance.

SDK-onlyWindows124681289707 terminalFAIL but50 observations retained:
/tmp/projectatlas-741-8d-windows-security-diagnostics.log and -matrix.json.
/tmp/projectatlas-741-8d-windows-job.log gives causal ordinary failure:
`800|absent|0|`; SACL absent but AI set. RUNNER_TEMP namedSDK ordinary is0/absent.
Dirty guard now rejects partial policy onlywhenSPset, retainingallotherbits for
strictsource/stagecomparison. DEFAULTED refusal appliesonlywhenSPset (SDK ignores
it whenabsent). Low-levelunit now covers0800 and0820 absent, pluspresent/defaulted
andhiddenaudit refusals. OrdinarynativefixturedecodesSP/class/count instead of
requiring allcontrolbits0. No SACL inheritance normalization/setter waiver.

CRITICAL serialization omission confirmed by primary+Luna: fullquery SACL|LABEL
returned ML ACEs, but production and oracle serializedSACL-only8, droppingML text.
Native label count1/S:PAI; mixedcount2/onlyAUtext. Dirty production and C#oracle now
serialize SAME query flags. Added actualpublisher forcedstageAU+ME vs identicalAU+
HI source/equalcontrol bits, heldstage/sourcecomparison mustdiffer, finalHI and
outsidehardlink preserved. Audit-only andlabel-only sourcechanges independently
refuse; finalstage label-onlychange refuses. These new Windows controls NOT yet run.

DACL-helper objects give2810/presentunknownpolicy. .NETSet-Acl sideeffect is only
an unconfirmed hypothesis; DO NOT waiveSPpresence. Dirty fastSDK matrix now measures
before/after .NETDACL vs nativeDACL-only, NativeGetTempPath vs RUNNER_TEMP, named vs
retained-handle descriptor-only API (matches canonicalwrapper), with/withoutprivilege.
Only relevant masks8/16/24 retained. Need native data before changing helper/boundary.

Private creation OW/0x80 proposal NOT adopted in product: old MAX probe failed
native5 insideCreatorFileControl, exactphaseunknown. Current C#probe adds phase
context, explicitRW/RC/WD/WO/DELETE, owner/groupset throughoriginalhandle, MAX vs
nonMAX matrix. Nativeunit explicitly tests nonMAX; failedMAX remains observable.
Initial DACL present/protected/oneOWmask80 validated, DELETE/WD/data secondopens
mustnative5 BEFOREpayload; share7 makes it a DACL control, not productshare4 proof.
Parentpermitscreate butnotDELETE_CHILD. Fixture-onlydirectDeleteFilecleanup replaces
PowerShellRemoveItem failure, restoresparentpermissions in finally. NOTRUN natively.

Luna cleanup design review: productionprivatecreationneedsretainedDELETEhandle+
SetFileInformationByHandle dispositionfailurecleanup; currentTempPathDrop ignores
pathunlinkerrors andcanleakstage whenparentcreateallowedbutDELETE_CHILDabsent.
No productionCreateFile binding/dependency change yet. Do not roll out hypothetical
creation rights until SDK data; require accepted controlledexception review first.
No process-wide privilege changes, shell productdependency or blanket elevation.

## Reviews, gates and next publication

THREE Codexthreads resolved individually with primary+Luna exact e222 native proof,
reply/resolution readbacks: MacfilenamePTKc/reply4433526580;
MacsetterR85I/reply4433527823; SELinuxQ2zn/reply4433539384. SELinux resolution is
kernelxattrpreservation/refusal, not LSMenforcement/exploit proof.
FIVE remainOPEN: overallACL PRRT_kwDOQyFgJ86yP6jJ; Windowssetter...ySZ0R;
SACL...yTBSb; integrity...yTfKy; stagecap...yT5Gl. Stagecap reply4433483542 readback,
confirmedtriage /tmp/projectatlas-741-e222-stage-capability-triage.json rendered.
A retained DELETE/WD capability survives DACL tightening; handle rename alone is
insufficient. Initial stage inheritance must not grant new capabilities.

All25 e222 CodeQL results individually reconciled against prior rule/fingerprint/
flow boundaries: Rust2069256388/Actions2069216390,
/tmp/projectatlas-741-e222-codeql-reconciliation.json. 24AdvancedSecurity discussions
previously individuallyanswered/resolved/readback (original7+17new), reports394/fc
retained. No alert dismissed/suppressed, scannerenabled. Latest8d/nexthead analysis
and comments still need review; do not infer newfindings are oldfixtures.

Current local proof PASS: Linuxowning31+1ignored0.33s; CLIalltargets/allfeatures
Clippy7.54s; actual WHOLEowning WindowsGNU compile-onlyClippy --tests; fmt/strings/
strictOpenSpec/IssueOps1of5; YAML+explicitBash parse; exactworkflow and3inventory
controls. NewWindows-onlychanges afterLinux run do not establish Native behavior.
Latest logs /tmp/projectatlas-741-label-serialization-* and -label-{workflow-contract,inventory}.log.
LunaMaxreview_741 approved source/diagnostics plus final nonMAX/defaulted delta for
NATIVEpublication only. Primarywritesallcode. Reviewers noedits/build/index.

Next: finalreview/checkpoint, commit issue741 only, CLEANnormalpush with a NEWprefix
using python3 /tmp/projectatlas-741-acl-native-push.py /tmp/NEW-PREFIX. Every shell
login:false; source /tmp/projectatlas-rc1-test-env.sh beforeCargo. Sharedintegration
build target/cacheTMPDIR. No selectededits/competingbuild/index/bench during push
orimmutableproof. Resume sameactualsession toterminal; observationtimeoutisnotfailure.
Then new uniquehead138inputfreeze/actualCLI/MCP/registered/5upgrades, fastWindows
SDKdata, fix confirmedremainingboundaries, native4platform proof, finalreviews,
tasks/acceptance and normalmerge+BOTHCLOSED before next issue.

## Atlas and release continuity

Readcomplete installed050skill+shortguide before Atlas aftercompaction. Exact741
installedbrief known policy_drift/fullchanged0 tracked739. PreserveprivateDB; no
orientationreset/installerrepair. Normalpush8d required scan/lintPASS, newWindows
modulepurposealreadyapproved/agent_reviewed; refreshchangedsource normally, fullscan
onlytypedrequired. Frozen current-contract executable can do authoring if050 refuses.

Linux uses retainedFD uid/gid/mode/POSIX+NFSv4ACL/SELinux+3Smacklabels; equal-empty
probe avoidssetters, changedpolicy+bytes prepared anonymouslybefore naming. Mac all
replacements privatewriter0700/ACLempty; descriptorSDK ACLs, no /dev/fdproxy. Windows
DACL exactorderedACE/P/AR/NULL/empty; ONLYnative-proved regularfileDACL AI normalization.
SACL AI remainsstrict. No universalhostileancestor guarantee, no LinuxNFSv4/ReFS
nativefixture/physicaldefaulted-file claim. No741schema/DER/SEM bump. Lifecyclepin
still ee25e9df237172f3c2c924669ac524fe3ffab7ec84038606ec91e57f5ceb761a.

Parked7396eb0dfd+dirt,7388814d6d+dirt,708publicc65dfed/local587ecbe/PR728 and715
integrationpreserved. 708warm120sample1.127FAIL/focused1.031526partial, noSLOwaiver.
715children+blockers708/718/721/722/731/732/738/739/741;718/721/722/731/732 CLOSED.
QueuedCodexCIadoption onlyafter741merge+BOTHCLOSED: /tmp/projectatlas-codex-ci-followup.json.
AcceptedmainDER24/SEM23; final715DER>everyprivatecandidate,SEM>=23. Final715combined
installedCLI/MCP/plugin/skills/hooks/all4OS/upgrades/cancellation/concurrency/security/
performance/packages/checksums/non-draftRCpublication+independentdownload/install
verification. Stable050Latest unchanged; no fake goalcompletion or ETA.
