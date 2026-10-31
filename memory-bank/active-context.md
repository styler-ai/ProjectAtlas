# Active context

## ACL normalization and creation-mode candidate

Only #741 / PR #743 remains active. Public HEAD is cdb0857; current dirty candidate
adds the owning fixes below. No native push is active yet. Tasks 1/5 and all
acceptance remain open. Do not claim Windows/macOS acceptance before hosted rerun.

Windows native cdb ordinary replacement differs only in AI, with exact owner/group,
presence/NULL/P/AR/ordered ACEs. Candidate accepts only AI normalization for admitted
regular-file readback; directories are outside this comparison. Pure controls reject
P/AR/NULL/empty/ACE/order/identity changes; private diagnostics remain. All three
special native fixtures are present: D:NO_ACCESS_CONTROL, D:P, D:. Microsoft .NET
reference source proves its NULL descriptor serializer omits D: (crafted allow-all
representation), so independent RawSecurityDescriptor binary class checks replace
the invalid text-presence assertion. Check class before/after; failed publication
requires exact old SDDL; successful publication permits only AI with owner/group,
ordered policy and NULL/empty/protection class intact. Native verification pending.

New Codex P2 4431642098 causal init-mode unit failed before fix. Managed config now
uses its Document creator. Broader accepted-main trace proves config, nonsource and
all host JSON writers previously used fs::write. Shared new-output publisher now
uses 0666 filtered by umask whenever metadata is absent; existing output mode/ACL
preservation is unchanged. Units cover all host registry entries and nonsource,
default/private/legacy init and repeat init; existing custom/external positives stay.
CLI/configured MCP exercise new config/nonsource/host JSON and default map-sidecar
creation modes before original hardlink/refusal controls. Final lifecycle digest
is ee25e9df237172f3c2c924669ac524fe3ffab7ec84038606ec91e57f5ceb761a in both pins.

Mac Intel cdb also passed 21 units, legacy repair, real CLI/MCP admission/writer/root
and registered-worktree proof, then failed the same nested APFSX hdiutil create.
Candidate maps diskutil's APFSX token to hdiutil's Case-sensitive APFS name (upstream
zsh completion lists that exact name); logs native create help and retains all four
APFS/APFSX/HFS+/HFSX outer modes with opposite-case nested mounts. No HFS proof yet.

Local proof: init causal before /tmp/projectatlas-741-init-modes-before.log; expanded
init4 and owning23 PASS; writer E2E PASS4.83s and umask027 PASS4.82s; 3 inventory,
workspace Clippy, strictstrings, strict OpenSpec and scoped IssueOps PASS. Final
23 owning units PASS0.08s; writer with missing map sidecar PASS4.96s and final
umask027 PASS5.02s; 3 inventory PASS2.66s, workspace Clippy PASS6.17s, strictstrings,
workflow Bash syntax, strict OpenSpec and scoped IssueOps PASS. Logs use
/tmp/projectatlas-741-acl-normalization-final-*. Independent Luna retracted a false
missing-D: claim after exact-byte recheck, approved the expanded mode/AI mechanism
and found no issue in the final map-sidecar/native-class test deltas. No final
acceptance claimed.

Next: commit the validated candidate and run normal CLEAN native
push with new /tmp/projectatlas-741-acl-normalization-native-push receipt/log. Preserve
all earlier receipts. No force/amend of public cdb; no parked owner or DB changes.

## Current GitHub security review and native blockers

Only #741 / PR #743 is active; public/local source remains cdb0857. No local
build/push/index lease is active. Tasks remain 1/5; all acceptance rows are open.

Advanced Security comments and REST alerts were re-read on cdb0857. Current Actions
analysis 2068197287 has two results: #377 and new #379 share the unchanged event-bound
checkout producer and cache-scope counterevidence. GitHub marks prior #378 fixed;
that scanner identity change is not a manual dismissal or claimed application fix.
Latest available Rust analysis is still 2068152217 on 3a6409c: five alerts #372–376,
all 20 flows traverse cfg(test) fixtures. Current affected guard source is unchanged
apart from unrelated Windows diagnostic code. No new actionable Advanced Security
finding, suppression, or alert mutation. Eight-item static report rendered and saved
at /tmp/projectatlas-741-security-comment-triage-cdb.json. Keep the Rust revision gap
explicit until a current-head analysis appears.

NEW separate Codex P2 thread PRRT_kwDOQyFgJ86yQn8p: fresh default
config creation in atlas_map::init_project_with_config uses the Generated guard,
bypassing Document creation mode 0666/umask and retaining tempfile 0600. Source
confirms the routing mismatch. Fix under this same owner; thread remains open.
Prior Mac alias and ACL threads also remain open; no acceptance approval.

Current Windows job 124532093706 is terminal FAILURE: 16 passed / 3 failed owning
units. Ordinary host/config replacements match owner, group, presence, NULL state,
protection, AR and exact ordered ACEs; ONLY auto_inherited_matches=false. This is
native field evidence, not a guessed normalization waiver. Special-DACL fixture
also fails with 'native file descriptor has no DACL'; inspect its exact fixture
boundary before changing production or claiming absent-file-DACL behavior.
Log: /tmp/projectatlas-741-cdb-windows-job.log. Windows Unicode remains masked.

Current macOS ARM job 124532093674 is terminal FAILURE after all 21 owning units,
legacy repair and actual CLI/MCP admission/writer/root-validation tests passed.
Controlled-volume setup creates outer APFS, then nested APFSX hdiutil create fails
'Invalid argument' before any controlled-volume tests. Investigate supported image
format arguments; retain APFS/APFSX/HFS+/HFSX coverage. HFS behavior remains unobserved.
Log: /tmp/projectatlas-741-cdb-macos-arm64-job.log. Mac Intel/Linux/Rust source jobs
were still active at last bounded read; poll exact handles, do not restart them.

## Diagnostic candidate published; awaiting native field evidence

Public/localHEADcdb085722c0ff4e925a4b9b55cc8e3663f79fdb9 verified throughPRmetadata.
Normalpush60652 TERMINALPASS772.88s:34suites/1719passing/10conditionalignored,
allRust/docs/dependency/repository/actualscan+lint;3743823gone. Receipt/log
/tmp/projectatlas-741-native-acl-diagnostics.*. No active locallease.
Current sourceCI41199414431 andCodeQL41199412032 active; prstate41199413031SUCCESS.
Candidate fixesMacfixture only andaddsWinfield diagnostics; WindowsstrictACL
predicate remains unchanged and ordinaryreplacement is NOT yet fixed. Await exact
owner/group/presence/NULL/P/AR/AI/opaqueACE match booleans from currentWindowsjob.
Do notguessinheritancewaiver. Preserve3a nativeevidence below; oldWindows16/18failure
wasgenericandnotexactfieldproof. NativeHFS/WindowsUnicode stillpending/masked.
Live/local1/5/allacceptanceOPEN; only741/743 active, no merge/release/Latestchange.

## Published ACL candidate: native results and diagnostic follow-up

Public/localHEAD3a6409c20da52af43167edb4005c3ae1aab7a035. Normal native retry8236
TERMINALPASS792.50s,34suites/1718passing/10conditionalignored; owned3547566gone.
Exact receipt/log /tmp/projectatlas-741-acl-native-push-retry.*. AllRust/docs/dependency/
repository/actualscan+lint passed;14knownnonfatalassetadvisories remain. No activelease.
Current sourceCI41196745927, CodeQL41196744289, prstate41196744970+41196946308PASS.
PRbodyupdated/readbackexact toRefs741 and currentproof/pendinggaps (closingkeyword
must return only at finalreadiness). MetadataCI41196946146 tracks source, no sourcecancel.

Default immutable native eee8d5cc174272c9d8f4b15196153f9fa3df4d95d23a1135216464a378257487
at marker /tmp/projectatlas-741-acl-debug-path.txt;131inputs bound clean3a +normalpush
receipt. Artifact/inputs reverified unchanged afterproof. Exact CLI/MCP admission
PASS3.41s, writerPASS4.84s, registeredholistic inclignoredPASS8.50s;1830TERMINAL0.
5stable050upgrade/refusal/retrylayoutsPASSidentity/allapprovedpurposes/config,
/tmp/projectatlas-741-acl-native-upgrade-proof.json/log/py. This proves published3a;
current diagnostic-only production delta below needs own sourcebinding if published.

Both native Mac jobs124524363792/124524363825 TERMINALFAIL: all20owning units including
orderedACL passed, legacyrepair andautomaticCLI/MCP passed. WriterE2E EEXIST before
controlledfourvolume step. Source diagnosis: uppercaseJSONsidecar aliases an already
existing lower default; newconfigvariants also wrongly reused configalias for mapalias.
Primary fixture fix now independently probes mapalias and removes only its confirmed
fixture-owned collision before outsidehardlink setup. LinuxwriterPASS4.86s. Bothhash
pins updated to736c83fbce7c9ebb27cd336dc9ca8fc1c3c57d248cd6a9b1278cd6b5e8cdbae3;
3inventorycontrolsPASS2.64s. HFS+/HFSX still UNOBSERVED; Macunitpass is APFSdefault only.
Logs /tmp/projectatlas-741-acl-{macos-arm64,macos-x64}-job.log.

Windows job124524363941 TERMINALFAIL16/18units. Newprotected/inherited restrictiveDACL,
NULL/empty andstrictrename5 controlsPASS. Existing externalhardlink replacement and
managedcase fixture refuse ordinary host/config writes with 'staged DACL differs'.
Current predicate conflates owner/group/full-SDDL; actual mismatch property UNKNOWN.
Do not guess anAI/ARwaiver or disableACLpreservation. Primary added DIAGNOSTICS ONLY:
owner/group,presence,NULL,P,AR,AI,opaqueorderedACE equality booleans, no SDDL/SID text.
Newprivacy/differenceunit+all21LinuxowningPASS0.08s; workspaceClippyPASS7.53s. Predicate
unchanged; native nexthead must provide exactfielddifferences before fixingowner.
WindowsUnicode stillmasked/unconfirmed. Log /tmp/projectatlas-741-acl-windows-job.log.
Native specialfixture expandedtoD:unprotectedempty with noninheritable directoryACL,
and verifies actualrequestedNULL/empty/protectionclass before assertingpreservation.

SDK SE_DACL_PRESENT guarantee (Microsoft SECURITY_DESCRIPTOR_CONTROL) +independent
Luna confirmation disposes impossible 'absent file DACL' proofgap: absent means an
unattached descriptor; GetSecurityInfo(realfile) alwayshaspresentbit. Designupdated;
retainnativeNULL/emptycontrols. No LinuxNFS4 filesystem proof. Potentialstagecleanup
edge with sourceDELETEdenial ANDparentNO_DELETE_CHILD remains unconfirmed; current
nativeWindowscontrols retainparentdeletionandpass. Preserve this boundeduncertainty.

Current3aCodeQLreconciled: Rust2068152217 fivealerts/all20flowscfgtest; Actions2068117750
twoalerts same event-bound producer. No new actionablesecuritythread; two Codexthreads
remainopen. CurrentSARIF /tmp/projectatlas-741-{rust,actions}-3a.sarif.json and
/tmp/projectatlas-741-security-3a-reconciliation.json. No alertdismissal/suppression.

NEXT: publish coherent Macfixture +Windows diagnostic delta through normal CLEAN push
after finalscopedgates. New receipt /tmp/projectatlas-741-native-acl-diagnostics.json/log;
do not overwrite3a receipts. IndependentLuna found no publicationblocker, not acceptance.
Only741active, live/local1/5/allacceptanceOPEN. No merge/release claim. Keepimmutable
andnativeproofboundtoactualsource; afterdiagnosticdatafixconfirmedWinowner, thenfull
nativefourformat/interface/upgrade proof +finalreviews before normalmerge+BOTHCLOSED.

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
