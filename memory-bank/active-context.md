# Active context

## Native push stopped on inventory metadata; corrected retry ready

Unpublished4e574484f345e8021c58a9873870c0930af87aa4 normalpush33584 TERMINALFAIL
290.99s (owned3432091gone), solely3 E2E inventory checks: lifecycle source digest
was not updated for new Unicode cases. Receipt/log /tmp/projectatlas-741-acl-native-push.*
preserved. Remote branch independently still16ef88a2fc378100e6a7d210993b3317ba40025c.
Updated ONLY lifecycle digest in docs/v050-cli-e2e-inventory.json and owning
tests/support/source_contract.rs to75766c77429559237c66398a660347d848a85a3c19d2290c12d8275d412e0713.
All3 inventory checks PASS2.65s (including negative tamper controls); no selector,
baseline/facet/attribute/coverage contract changed. Normalpush reached408CLI unitsPASS,
E2Edelivery49PASS/3inventoryFAIL/2ignored before stopping; not a full native pass.
Independent final static delta review found no confirmed source blocker to publishing
for native proof, not final acceptance. Amend unpublished candidate with inventory
correction, then normalclean retry using NEW /tmp/projectatlas-741-acl-native-push-retry
receipt/log; preserve failed receipt. No active lease at this saved checkpoint.
Local/live1/5/allacceptanceopen, only741active. All Mac/Windows/HFS/ACL proof gaps below
remain. Absent-DACL case still lacks a native fixture; no native NFS4 claim.

## Current ACL/APFS candidate checkpoint (supersedes history below)

Only #741 / PR #743 active. Public head remains16ef88a2fc378100e6a7d210993b3317ba40025c
until the next normal push succeeds; local Git HEAD/dirty diff own candidate identity.
Local/live tasks now1/5 (only1.1), allacceptanceOPEN. Task2.3 reopened for new scope;
issue body exactly read back with ACL and native-format requirements. StrictOpenSpec
and scopedIssueOps PASS (5local/5remote/1checked). No other owner resumed.

Native Linux ACL loss reproduced causally before fix: exact receipt/log
/tmp/projectatlas-741-acl-before.json/.log; old publisher loses explicit ACL on first
host config. Current primary implementation preserves Unix uid/gid, Linux kernel
POSIX/NFS4 ACL values, macOS ordered exacl entries, Windows DACL/protection/owner/group
through safe handle wrappers; mode/ACL readback before payload/publication. Linux
NFS4 code path has NO native NFS fixture proof. New dependencies: Unicode9
unicode-casefold0.2.0, macOS exacl0.13.0, Windows windows-permissions0.2.4; Linux xattr
already locked. Exact bitflags1.3.2 ban exception documented for safe Windows wrapper.
No project unsafe code, shell runtime dependency or publication fallback.

Windows stage denies competing data handles (delete sharing retained); native tests
exercise protected/inherited DACLs, current-user DELETE denial, controlled failure/
cleanup/retry and outside hardlinks across the live host registry plus managed
replacement docs. NULL/empty tests require exact preservation OR causal refusal,
unchanged original access/bytes/identity and no stage. Wrapper NULL accessor is
avoided via native DACL-only SDDL; do not claim actual NULL behavior before host proof.
Its ordinary security-handle reopen with delete-only data sharing still needs native
proof. Mac ACL metadata/order tests use native chmod; Mac staging/inheritance behavior
also awaits native proof. Required independent review remains open, no acceptance.

APFS confirmed fi-ligature bypass repaired using pinned Unicode9 full folding gated
to actual insensitive APFS volume; HFS+ retains provisional simple-fold path until
native HFS+/HFSX proof. Fixture observes each name pair before missing-leaf admission,
including uppercase, long-s, fi-ligature, dotless-i, ZWJ and BOM. It collects all
native mismatches before returning; Windows Unicode remains unconfirmed until CI.
CI controlled modes expanded APFS/APFSX/HFS+/HFSX with opposite-case nested mounts.
Real CLI/configured-MCP writer controls now include fi/dotless-i/ZWJ custom/alias paths.
Strict Windows ERROR_ACCESS_DENIED5 directory-race branch proves OS prevention,
unchanged retained owner, no stage/output, sentinel and retry; Unix actual swap still
passes. Do not describe Windows OS prevention as a successful swap/retention test.

Current Linux20 owning unitsPASS0.07s, including all6replacement roles, explicit/no
ACL, inherited defaults, controlled failure cleanup, outside owner and uid/gid/mode;
workspaceallfeaturesClippyPASS7s; full locked/allfeatures cargoDeny(advisories/bans/
licenses/sources)PASS and strictstringsPASS. Plain default-feature cargoDeny was an
INVALID invocation for this workspace and reported unused optional deps; corrected
allfeatures gate passes without weakening policy. CLI/MCP writerE2EPASS4.52s before
the final ZWJ variant; normal native hook must rerun final source. Full Rust/docs and
actual CLI/MCP/upgrade/platform proof for this candidate remain required. Old immutable
418/131-input proof is obsolete for changed source. No new immutable artifact yet.

NEXT: current independent Luna static delta review, then commit coherent candidate
and normal CLEAN native push with hooks/telemetry/MCP override discipline. Use new
receipt /tmp/projectatlas-741-acl-native-push.json/log, poll its actual handle; no
source edits/build/index during clean-push proof. At this saved checkpoint no push
is active yet. Actual Windows/Mac data determines remaining fixes; keep review
threads PRRT_kwDOQyFgJ86yPTKc and PRRT_kwDOQyFgJ86yP6jJ OPEN until published proof.

## Security-comment review and confirmed hosted failures (2026-10-31)

Only #741 / PR #743 remains active; public/local HEAD is 16ef88a2fc378100e6a7d210993b3317ba40025c.
User requested Advanced Security bot review. Current Rust analysis 2067979442 has
five path alerts #372–376, all 20 reported flows through cfg(test) owning fixtures;
prior independent dispositions remain valid. Actions analysis 2067944316 has two
cache alerts #377/#378 sharing the same event-bound head producer and checkout.
Schedule uses default-branch github.sha; dispatch has no revision input and uses
selected-ref github.sha; PR caches remain merge-ref scoped. Independent Luna Max
agrees no default-cache poisoning path is established. Read-only GITHUB_TOKEN and
absence of explicit cache actions are NOT the mitigation. Technical reply
4431405184 exactly read back; cache thread PRRT_kwDOQyFgJ86yP35t resolved/read back.
Actual seven code-scanning alerts remain open/undismissed; no suppressions applied.
Current SARIF and extracted flows: /tmp/projectatlas-741-{rust,actions}-16*.json.
Eight-item static triage rendered through Codex Security; saved separately below.

NEW Codex Security review comment 4431352544 / thread PRRT_kwDOQyFgJ86yP6jJ remains
OPEN: replacement preserves std::fs::Permissions, not destination ACL/DACL. Static
Windows mechanism confirmed (medium confidence): generated host writer reaches
publish -> new tempfile -> basic permissions -> MoveFileExW replacement. Parent
inheritance can grant broader rights than the old explicit DACL. Private restrictive
parents can defeat exploitation; no native ACL reproduction or code execution was
demonstrated. Fix at existing #741 publication boundary, preserve normal regeneration,
atomic failures and outside-hardlink behavior; check analogous supported Unix/macOS
ACLs and all replacement roles. Native proof and independent review still required.
This is a new unresolved finding, NOT a completed fix or accepted exception.

Hosted 16 source run 41191137884 confirmed both Mac jobs fail the owning case fixture:
APFS aliases conﬁg.toml (fi ligature) to config.toml. Unicode SIMPLE folding is
insufficient. Preserve actual volume semantics; APFS Unicode9 fullfold candidate
needs implementation/proof, HFS+ differs and must not be blindly fullfolded.
Windows now passes 14/15 state units; remaining replacement test reports native
rename ERROR_ACCESS_DENIED (5), not an absent callback. A strict OS-prevention branch
may assert original owner, staging cleanup, unchanged foreign sentinel, no output,
and successful retry; arbitrary error skips are prohibited. No such fix yet applied.
Logs: /tmp/projectatlas-741-case-{macos-arm64,windows}-job.log.
Windows Unicode alias lead remains UNCONFIRMED pending actual native probe.
Task 2.1 and all acceptance remain open. No production edits since published16;
dirty memory only. Existing immutable418 proof cannot cover future source changes.

## Latest published candidate and hosted verification

Public/localHEAD16ef88a2fc378100e6a7d210993b3317ba40025c verified. Normal nativepush
68872 TERMINALPASS759.75s:34suites/1716tests,10conditionalignored, allRust/docs/dependency/
repository/actualscan+lint. Receipt /tmp/projectatlas-741-case-native-push-retry.json/log;
ownedPID3234064 gone. Noactivelease. Original92pushfailedprivatepath22.73s; unpublished
amended16 correctednote; strictpolicycheckscommittedHEADtoo (dirtyfixalonecannotpass).
Immutable418 all131boundinputs equalpublishedsource; supplementary committedbinding
/tmp/projectatlas-741-case-committed-binding.json, originalsourcebindingpreserved.
CurrenthostedSOURCE41191137884 ACTIVE, prstate41191136269SUCCESS. PRbodyrewrittenfor
finalscope/proof/pendingMac+Windows; freshreadbackneededaftermetadataupdate.
Local/live2/5/allacceptanceopen. MacP2threadstillopenpendingactualnativeproof.

NewestboundedLuna review found credible UNCONFIRMED WindowsUnicodealiaslead: long-s
passesregexcandidatebutWindowsnamedASCIIrejects; dotless-i configlikelyrejectedbyregex.
NeedactualnativeFSproveASCIIentry↔Unicodepathtry_exists/same_file, reverse spelling,
thenCLI/MCP/owningadmissionfailure besidecaseSensitivecustomcontrols. Do notclaimbug
confirmed fromUnicode tables, blanketfoldWindows or silentlydeclarelegitdirunsupported.
PrimarysourcesMSWindowsuppercasefilenamecomparison/NTFSUpCase makeitworthboundedproof.
Macsimple/fullfoldalsolefttoactualconﬁg.toml filesystemnegativecontrol; primaryAPFSdocs
alone don'tspecifysimplevsfull. Keep741only; Windowsreplacementprecisediagnosticfrom
current16 hostedrunstillrequired. Nextsafeactionwatchactualhostedjobsandthenfixowning
confirmedfailure; sourceeditsallowedafternativepushterminal. Olderparagraphsbelowarehistory.

## Active RC1 goal and current owner

Continue until v0.5.1-rc1 is a non-draft prerelease, Latest=false, independently
verified; stable0.5.0 stays Latest. No stable publication or milestone closure.
One issue at a time: only #741 / PR #743 implementation active until normal merge
and BOTH CLOSED verification. #731 / PR #737 BOTH CLOSED via a5b1bebcaa79c8386b68e9c247eff73aa11e5e15 on2026-10-31.
Parked #739/#738/#708/#715 untouched. Five RC1 issues remain open. No RC published.

Exact checkout issue-741 / fix/generated-state-741. Public head3caf70f62c6cdc40a309f153795862cd90647cf4.
Current reviewed candidate adds macOS case-alias fix, native fixtures/CI, documentation and tasks; Git HEAD owns the commit identity.
PR743 ready, milestone37, sole Fixes741. Local/live2/5 checked (1.1,2.3);2.1 REOPENED
for Macfinding,2.2/3.1/all5acceptance OPEN. Current candidate still needs normal native push and hosted verification.
First normal nativepush of92e8cbf failed22.73s solely private-absolute-path in this handoff note; removed the pinned machine cache path. Receipt /tmp/projectatlas-741-case-native-push.json. No active lease at this checkpoint; retry through normal hook. Do not switch owners.

## User-authorized dev retirement completed

On2026-10-31 user explicitly requested retiredev now and close720 asnotneeded.
Remote refs/heads/dev DELETED after verifying zero unique commits,78behindmain,
zero open PRs to/from dev. Tip7cd5a799f27b9e3b1d7ae590d95985f557cd44c3 preserved
in annotated archive/dev-2026-10-31 tag and frozen dev tag (old blob/dev links work).
Old dev protection removed solely to retire that branch. Main tip and ENTIRE main
protection JSON independently verified unchanged. Local origin/dev removed with exact
OID lease; no local dev branch existed. All131 current RC1 bound source inputs unchanged.
#720 CLOSED/not_planned at2026-10-31T09:39:27Z, live readback verified, ReleaseScope
records retirement and explicitly DOES NOT claim historical check-selection bug fixed.
Receipt /tmp/projectatlas-retire-dev.json and backup protection JSONs. Releaseowner715
must retain this branch-policy disposition; no need to implement obsolete devsync.
GitHubworkflow compatibility filters may still name dev but no active branch/automation
requires it. Current CodeX public repo usesmain integration andtopicdev/person/... refs,
no shared exactdev/develop; this was read-only research, not a reason to weaken gates.

## Current #741 implementation and independent source review

Managed DB/WAL/SHM admitted before EVERY SQLite open; generated outputs before init,
identity repair, roottransition and registered hydration. Four document roles: private
config.toml, rootlegacy projectatlas.toml, root.gitignore, privateprojectatlas.json.
Atomic/no-clobber publication, retained identities, outsidehardlink preservation,
existing permissions/ordinaryumask, supported explicitcustom/external outputs andaliases.
Custom init uses generated writer only for owned parent AND exact DocumentSome;
read-only live nonsource aliases preserved; dangling/create aliases refused.
No schema/DER/SEM bump: acceptedmain731 DER24/SEM23. No universal ancestor-race claim.

New Codex P2 PRRT_kwDOQyFgJ86yPTKc/comment4431106754 remains OPEN until actual
published/native Mac proof + technicalreply/readback/resolution. Fixed shared classifier:
parent owner first, pure Unicode simple-fold candidate (existing regex; non-expanding
fold rejects ligature/sharp-s custom controls); exact/ASCII fastpath retained. Mac named()
uses existing locked whichdisk0.7 directoptionalMacdependency, no newcrate/unsafe/cache.
Resolve existing ORIGINAL parent prefix, reduce unresolved suffix, then query prospective
DESTINATION volume (nearestexisting ancestor ifstillmissing). Unknown/error failsclosed
only on plausible owned alias. Case-sensitive custom names remain explicit paths.
Fallible directdirname/reduction/generatedcapture propagate errors. Native volume lookup
is never ownership proof. No confirmed analogous Linuxcasefold/Windowsdir-case route;
record those adjacent unverified behaviors, do not claim coverage.
LunaMax review_741 source/plan reviewed: Unicode gap and cross-mount wrong-prefix gap
both fixed/re-reviewed, no remaining material production source blocker. Actual Mac
proof still REQUIRED. Implicit root-fromDB exactdirname accepted: implicit names are
literal productdefaults; explicit selection derives root independently.

CI adds all4platform ownershipselectors plus BOTH controlled APFS/APFSX volume modes
on BOTH Macarchitectures. Each mode also mounts opposite-case volume below.state/bridge;
all5roles test missing/../.. destination-case selection; fixture probes verify actualcase.
Owning units cover all5names absent/hardlinks, main/WAL/SHM, upper+long-s dir/leaf aliases,
dangling4documents, existingGitignorenoop, sensitive sharedinode and actualnon-alias
ligature/sharp-s controls. Existing real CLI/MCP rows extend uppercase/Unicode DB startup
and config/map hardlink writes. Inventory221E2Etests/78workflowselectors,currenthashes.

## Current Windows proof gap (do not hide or guess)

Hosted SOURCE3caf CI41185098938 Windowsjob124490361702 FAILED ownershipstep:
14stateunits,10PASS4FAIL. Exact log /tmp/projectatlas-741-windows-3caf-job.log.
Three failures were invalid fixtures: canonical Windows verbatim PathBuf joins erase
missing/..; nonverbatim Win32 APIs can also resolve before traversal. New test-only
unreduced_path appends native components through OsString and asserts ParentDir retained;
rootalias keeps verbatim prefix; logical/physical controls repaired. Expected managedSome
versus unresolvedErr follows ACTUAL parent canonicalization. Map serializedabsolute
verbatim path preserved by AtlasMap absolutize; refusal verifies sidecarbytes+hardlink
identity, outsidebytes and missingparentinventory; resolvedparent proves publication.
Independent Luna review found these corrections sound; actual Windows branch coverage
must still be observed, not claimed from Linux.
FOURTH failure directory_replacement_never_unlinks_a_foreign_staging_name was ambiguous:
old stage_name assigned only after rename+alias, so callbackdidnotrun hid actualerror.
Current change is DIAGNOSTICS ONLY: callbackentry, stageenumeration, renamecompletion,
aliascompletion and underlying result. NO guessed Windows skip/OSprevention branch.
Need normalpush currentrevision, inspect actual Windows error, then fix confirmed owner
or accurately prove kernelprevention with strict controls if native evidence supports it.
Main Mac source review clean does not close this Windows proof gap.

## Immutable source/proof and required next actions

Current immutable defaultdebug418c8a906c9764960acb4c2c673ceac87c6183a1c4b8c06fbce69fcd87fadbe1:
/tmp/projectatlas-741-case-debug-path.txt points to read-onlybinary; adjacent source-binding
binds131inputs at dirty3caf. All131 independently verified unchanged afterdevretirement.
Current18stateunitsPASS; allfeatureworkspaceClippyPASS6.08s; strictstringsPASS;
3inventorychecksPASS2.69s; writerfixturePASS3.84s; finalnativebuildPASS4.89s.
Artifact418 nativeautomaticadmissionPASS3.44s, writerCLI/MCPPASS3.77s,
registeredholistic explicitincludeignoredPASS8.18s. Session18899 TERMINALPASS.
5native050upgrade/refusal/retrylayouts(auto,legacy,external,rootalias,privatecustom)
ALLPASS identity+ALLapprovedpurposes+config; /tmp/projectatlas-741-case-native-upgrade-proof.json/log/py.
No actual Mac beforeimage executed; casebefore defect is source/nativeFS finding, current
owning fixtures would fail oldclassifier on actualinsensitive volume. Do not invent proof.

NEXT: strictOpenSpec/scopedIssueOps and owned diff checks PASS; publish currentreviewedsource
and diagnostics through CLEAN normal nativepush (hooks enabled, telemetryoverrideUNSET, MCP
executableoverrideUNSET). Preserve exactterminal process/receipt; no sourceedits or
competing builds whilepushactive. HostedbothMacvolumeproof and actualWindowsdiagnostic
are next dependent evidence. Correct Windows finding withoutstartinganotherowner;
then currentfullproof/security/feedback/finaltask+acceptance/normalmerge+BOTHCLOSED.
Review current PRbody around final behavior/proof (old403/39/fourlayout text is stale).

Prior3caf normalpush53150 TERMINALPASS761.28s34suites/1714tests/allRust/docs/security/
repository/actualscan+lint. Prior52 fullpushPASS937.02s/1713tests; hosted52 ALL8SUCCESS
but it lacked new explicitownershipstep, do not reuse as current native4OSownershipproof.
Prior CI-onlynative inventory failure fixed exactly4selectors; e957pushinterrupted130
for confirmedcustomsourcefix, NOT pass. Old82 and3b97/b6aa immutable artifacts preserved;
originalcustomP2 published/replied/resolved. Existing causal sharedclassifier/capture/
legacyrepair/custommode evidence and original receipts remain /tmp/projectatlas-741-*.

Five first CodeQL372–376 alerts independently notactionable: all20flows pass cfgtest
fixture plumbing; closedderivedroles/retainedownerchecks preserve supportedoperatorpaths.
Per-alert technicalthreadsreplied/resolved; actualalertrecords leftOPEN/undismissed.
RustSARIF /tmp/projectatlas-741-first-rust.sarif.json and extractedflow summaries.
New-head CodeQL/threads still need fresh reconciliation, no blanketbans/suppressions.

## Tool/worktree discipline

Read complete installed050Atlas skill+shortguide aftercompaction beforeAtlascalls.
Exact741MCPbrief returned known stable050 policy_drift/full; preserve privateDB,
no reset/installerrepair or broadscan fororientation. Use verified owner slices.
Source /tmp/projectatlas-rc1-test-env.sh beforeCargo; sharedtargetv0.5.1/target.
login:false ALWAYS. Unset PROJECTATLAS_NO_TELEMETRY and MCP_CONTRACT_EXECUTABLE for
normalnativeproof/push; isolatedreadonlyAtlas/probes use telemetry1. No force/admin
merge/bypass. Exactone(#741) subjectreference; soleFixes741PRbody. Resolve the installed OpenSpec command from PATH or the existing npm cache; do not install a different version.
Primary implements; existing LunaMax review_741 read-only reviewer; followup_task toreactivate.

## Authorized CI follow-up after #741

User requested adoption of useful Codex workflow practices. Finish741first, then
handle ONE focused CI issue: bounded gap review of synthetic merge-candidate verification
and affected-check feedback, preserving required complete RC1 platform/release proof.
Existing affected routing may already cover parts; do not invent changes or copyBazel.
Queue /tmp/projectatlas-codex-ci-followup.json. Plan/spec/review before implementation.
