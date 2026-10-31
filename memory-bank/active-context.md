# Active context

## Goal and serial owner

Continue until v0.5.1-rc1 is published and independently verified: non-draft
prerelease, Latest=false; stable v0.5.0 remains Latest. Stable publication and
milestone closure need later user instruction. Only #741 / PR743 is active.
Normally merge it and verify BOTH CLOSED before another issue. #731 / PR737 are
CLOSED on accepted main a5b1bebcaa79c8386b68e9c247eff73aa11e5e15. Dev retired;
archive/dev-2026-10-31/tag preserved; #720 CLOSED/not_planned.

Selected root .worktrees/issue-741, branch fix/generated-state-741. Published baseline
is fc60711ab494169a17c432efbfe785e57b457442. Windows SACL/root-error123, Mac
rustdoc and CI corrections are source-reviewed and prepared for normal publication.
Actual Git HEAD and /tmp/projectatlas-741-sacl-native-push.json own the next revision. PR ready/notdraft,
base main, milestone37, body Refs741. Tasks remain1/5 (only1.1); all acceptance
OPEN. Restore sole Fixes741 only at final readiness. No RC exists.

## Latest clean publication and native evidence

Normalpush16093 TERMINALPASS787.21s/34reported suites/1730pass/11ignored;
PID/PGID736971 gone. /tmp/projectatlas-741-hfs-native-push.json/.log and updated
-hfs-active-session.json. Immutable native93136 TERMINALPASS: binary
 eeefce8f235a757fc3f7871edc8417d847929198e3a01a83fd2aaed76e0be158,
137boundinputs, /tmp/projectatlas-741-hfs-debug-path.txt and adjacentreadonly
source-binding.json. ActualCLI/MCPadmission12.20s, writer5.23s, registered9.24s,
five stable050 upgrade/refusal/retry layouts5.98s allPASS; identity, ALLapproved
purposes, config and boundbinary/inputbytes preserved. This proof covers fc only.

SOURCE41229649223 TERMINALFAIL: repository124621049970/Rust124621049990/
Linux124621049947 PASS. Windows124621050098:30PASS, root-classification unit
FAIL native123. Corrected existing-file WRITE_OWNER denial and all native DACL,
retained-handle, snapshot-release controls PASS. Log -fc-windows-job.log.
MacARM124621050020: ordinary+ALL4formats33owning+privileged1+actualCLI/MCP PASS;
sole later failure is doc_markdown TempDir at project_state_macos.rs:29, nowfixed.
MacIntel124621050014: ALL4formats33owning+privileged1 PASS; APFS/APFSX/HFS+
actualCLI/MCP PASS; HFSX admissionPASS then finalwriter hit5minstep timeout.
No assertion failed. Current boundedstep10min retains every assertion/cleanup.
Logs /tmp/projectatlas-741-fc-macos-{arm64,x64}-job.log. Verify124625936979FAIL.
No source/native execution lease currently active. Do not restart timed-out
observations of live jobs; distinguish actual terminal timeout from observation.

## Current personally checked security feedback

Primary reread all review threads: /tmp/projectatlas-741-937-fresh-review-threads.json.
SEVEN CodexthreadsOPEN: Macname PRRT_kwDOQyFgJ86yPTKc, ACL PRRT_kwDOQyFgJ86yP6jJ,
SELinux PRRT_kwDOQyFgJ86yQ2zn, Macsetter PRRT_kwDOQyFgJ86yR85I, Windowssetter
PRRT_kwDOQyFgJ86ySZ0R, NEW WindowsSACL PRRT_kwDOQyFgJ86yTBSb/comment4432635494.
New code-review P1 PRRT_kwDOQyFgJ86yTfKy/comment4432831536 independently
reports the same mandatory-integrity omission; replied/readback, threadOPEN.
SACL omission statically confirmed/rendered; exact report
/tmp/projectatlas-741-937-sacl-triage.json. Reply4432764667/readback, threadOPEN.
No bulk resolution: corrected published/native proof still required.

Primary reconciled ALL25 CodeQL results on937: Rust2069001152 has23 fixture-mediated
flows, Actions2068961735 has2 event-bound checkout/cache cases. Rule/location
fingerprints match individually reviewed394. Exact per-result report
/tmp/projectatlas-741-937-codeql-reconciliation.json, supporting SARIF files.
17 new AdvancedSecurity discussions individually answered/resolved/readback earlier;
receipts /tmp/projectatlas-741-394-codeql-resolution-receipts.json. Original7 also
resolved. Alert records NOT dismissed/suppressed; scanner enabled. Fresh fc reconciliation found the same25 results (Rust2069142756/Actions2069103198),
report /tmp/projectatlas-741-fc-codeql-reconciliation.json; sevenCodexthreadsOPEN.
Nextsource analysis/threads still required. Current PRbody updated
readback but must be reconciled to later native results before readiness.

## Dirty Windows SACL implementation and plan

Accepted OpenSpecproposal/design/spec and live741 WhatChanges amended BEFOREcode;
strictOpenSpec/scopedIssueOps1of5 passed. Existing independent gpt-6-luna Max
review_741 approved least-privilege plan and second private unsafeexception:
GetSecurityDescriptorControl only. Canonical windows-permissions lacksgetter;
SDDL omits SACL_DEFAULTED. New project_state_windows_security.rs owns safe opaque
SACL snapshots/copy around that sole getter; workspaceunsafeDENY elsewhere.

Try original nofollow source handle with ACCESS_SYSTEM_SECURITY; fallback only
native5/1314 to READ_CONTROL+attributes. No runtime shell or globalprivilege toggle.
FullSACL+LABEL snapshot retains LocalBox descriptor, ordered nativeSDDL/controlmask
0x2a30; defaulted SACL refused. PartialLABEL query must have control0 (not merely
empty ACEs) for BOTHsource/stage. Existing full rights acquired on originalstage
creation handle; apply throughheldhandle, NULLguard avoids dependency panic,
strictcontrol+SDDLreadback beforepayload. Source+stage policy rechecked before
persist. Original Windows snapshot dropped only afterfinalchecks, beforeMoveFileEx.
Ordinary noSACL/non-elevated replacement stays supported; inaccessible nonempty or
unsupporteddefaulted SACL replacement refuses retainingvalidoutput. Docs updated;
no raw-descriptor fidelity or every-Windows-attribute promise.

New native fixture uses independent namedSDK C#calls in childPowerShell ONLY;
privilege enabled/disabled onlyfixturechild. Ordinary source/stage absence and
capture-rights state; oldDACL-onlypublisher causal loss; sixregistryroles x label,
audit,mixed policy withAU/MLclass assertions; filteredLABEL retainsSP but hidesAU;
representative NULL/empty/protectedempty/unprotectedaudit now assert native control,
pointer class and ACEcount. Inheritedstage mismatch, finalsourceSACLchange,
SystemPolicy.apply afterforeignstagehardlink substitution, finalstageSACLchange,
outsidepolicy/bytes/cleanup controls. These NEW Windows tests have NOT run natively.
Defaultedguard low-level unit is NOT actualdefaulted-fileproof. CIselector
project_state:: includes this nested native module unit. No ReFS native proof.

## Other current corrections and local proof

fc Windows123 occurs because direct spelling query propagates invalid-name from
verbatim literal shortcut/../ paths before existing owning-prefix fallback.
Current fix treats ONLYNotFound/Windows123 as direct-proof-unresolved, including
Unicode directory identity; owning fallback resolves originalprefix before suffix.
Permission/othererrors stillpropagate. No lexical collapse first. Existing causal
root-alias/physical-external test expanded to ASCII and É roots; LinuxPASS.

Final3698 TERMINALPASS: Linux31owning+1privilegedignored0.20s, CLIalltargets/
allfeaturescheck3.32s, Clippy6.40s, fmt/strings/OpenSpec/IssueOps1of5PASS. YAML and
bothowningBashblocks and lifecyclehashPASS. Sourcebytesunchanged since these checks.
Logs /tmp/projectatlas-741-sacl-{linux-unit,linux-clippy,final-check,strings,issueops}.log.
Actual WHOLE project_state.rs + owningtests compile-only WindowsGNU Clippy
PASS0.68s, exactdirectlockversions/workspacelints; /tmp/projectatlas-741-windows-security-typecheck.
Caught/fixed2missinghelperargs, cfgWindowsconstantimports andnative-onlydoc/style.
Onlyharnessdead_code allowance (CLIcallers omitted); three targeted Windows readonly
expectations document canonical attribute-only behavior, no productlintbypass.
Actual wholeowning MacARM compile-onlyClippy PASS4.26s; analogous harness
/tmp/projectatlas-741-macos-owning-typecheck. NO nativecalls/link/runtimeclaim.
Final actualwholeowning cross-targetClippy --tests: WindowsPASS0.27s, MacPASS0.03s.
Diagnostic readback includes control words/SDDL-match booleans without identities. Std cross targets installed; no compiler/OS
packages installed. MacTempDir doc fix included. YAML+2owningBash blocks PASS.

## Atlas authoring and next publication

Read complete installed050skill+shortguide beforeAtlas aftercompaction. Exact741
installedbrief stillknown policy_drift/fullchanged0 tracked739. PreserveprivateDB;
no orientationreset/installerrepair. Frozenfc requiredauthoring incrementalwatch
PASS6parsed533unchanged: /tmp/projectatlas-741-sacl-purpose-watch.log. Subsequent
purposeSet hit dependency_closure_limit because source changed again. AFTERsource
settles follow requiredFULLscan, then approve/readback newWindowsmodulepurpose.
This is a required authoring refresh, notorientation. RequiredFULLscan91359 TERMINALPASS725summaries/174784relations; purposeapproved
and readbackagent_reviewed/live-source in -sacl-purpose-readback.json. No reset.
Later diagnostic-only module edit is not covered by that source summary; normal
push refresh must capture final source. Approved authored purpose persists.

Primaryownsallwrites; existing LunaMaxreview_741 read-only. Latestreview findsno
productionblocker and allowsnativepublication; NULL/emptyfixtureclass gap addressed
with nativecontrol/pointer/ACEcount oracle; independentreview explicitlyapproved
latestsource/tests/workflow for NATIVEpublication only. No acceptance.
Next: commit sole(#741), CLEANnormalpush /tmp/projectatlas-741-sacl-native-push
through enabledhooks/nooverrides, then new exacthead138inputfreeze/proof.
Then NEWimmutable138inputunion (ADDWindowsmodule), realCLI/MCP/registered/5upgrades,
actual4platform/nativeSACL/4volume proof and finalbots/review/tasks. Old137 proof
mustNOTbeclaimedfornewmodule/source. Normalpush helper accepts NEWprefix:
python3 /tmp/projectatlas-741-acl-native-push.py /tmp/NEW-PREFIX.
No selectededits/competingbuild/index/bench during cleanpush or immutableproof;
poll same actualsession toterminal. Every shell login:false; source
/tmp/projectatlas-rc1-test-env.sh beforeCargo; sharedintegrationtarget/cacheTMPDIR.

## Release continuity and bounded claims

Threehost+private/legacyconfig+JSON replacement roles sharepublisher. Linux retains
uid/gid/mode, POSIX/NFSv4ACL, SELinux+3Smacklabels viaheldFD; equal-policyemptyprobe
skipssetters, otherwiseO_TMPFILE bytes/sync thenmetadata/readback/sync before naming.
Macactual6SDKdescriptorACL primitives replaced disproved /dev/fd proxy; opaqueRAII,
fallible1MiBalignedOSencoding comparison, absence/allocatedempty distinct. All
existingMacreplacementsprivatewriter-owned0700/ACLempty; newfilesinheritnormally.
WindowsDACLexactorderedACE/P/AR/NULL/empty and ONLYnative-provedregularfileAI
normalization retained. SACL AI staysSTRICTpendingnativeevidence.

Linux security-xattr STORAGE is not enforcedSELinux/Smackexploitcoverage; LinuxNFSv4
filesystemnativeproof absent. No universal hostile-ancestor race guarantee. Windows
lockedtempfile pathattribute changes are inside accepted finalcheck/persist interval;
no supported bypass established, /tmp/projectatlas-741-windows-persist-review.json.
No schema/DER/SEM changes741; acceptedmainDER24/SEM23, final715DER>everyprivatecandidate.
Lifecyclefile digest unchanged BOTHinventory/supportpins:
ee25e9df237172f3c2c924669ac524fe3ffab7ec84038606ec91e57f5ceb761a.

Parked7396eb0dfd+dirt,7388814d6d+dirt,708publicc65dfed/local587ecbe/PR728 and715
integration specs preserved. 708warm120sample1.127FAIL/focused1.031526partial,
noSLOwaiver. Root715children/blockers708/718/721/722/731/732/738/739/741; closed
718/721/722/731/732. Queued Codex CI adoption onlyAFTER741 merge+BOTHCLOSED:
/tmp/projectatlas-codex-ci-followup.json. Thenserial739/738/708/715 aslivegraphdictates.
Final715combinedinstalledCLI/MCP/plugin/skills/hooks/all4OS/upgrades/cancellation/
concurrency/security/performance/packages/checksums/non-draftRCLatestfalse and
independentdownload/installverification; stable050Latestunchanged.
