## Accepted-main integration in progress on #739

Source/rootwatchcheckpoint832cdd5 safelycontainsall4previouslydirtysourcefiles.
Original5-filebackup /tmp/projectatlas-739-pre-main-zk6iey18 (manifest+patch+exactfiles),
privateAtlasDBuntouched. Main6927ed0 includesaccepted731DER24/SEM23and741ownership.
Sevenmergeconflicts reconciled bypreservingbothowners: lockpackageunion, DBexports,
allmainnavigationtests plus739externalpolicytest, derivedsourcehashes+inventory,
acceptedfutureissue-map plus739owner/RCchild andthis739continuity. Newsourcehasnot
beencompiled/tested; do notclaimmergedgatespassed. Keepall2.x/3.1tasksopen.
Finishconflictmarker/lockedmetadata/format checks, commitowner739mainmerge, refresh
sameprivateAtlas, thenaffectedsourceintegrationproof/independentLunaMaxreview.

## Current #739 selected; preserve root-watch delta and consume accepted main

#741CLOSED/PR743MERGED at6927ed0ca64788db6947b4d43eafce29aac5ed61 afterall9nativeCI
jobs/requiredchecks/reviews. Its reviewedandmergedtreesidentical; no further741work.
Primary nowONLY739, exactrootissue-739 branchfix/external-excludes-739. Existing6eb0dfd
plus4dirtysourcefiles(rootwatchrebind+graphbudgetpreallocation) preserved toexternal
snapshot /tmp/projectatlas-739-pre-main-snapshot-path.txt beforecheckpoint. Committing
that domain delta BEFOREmainmerge; old localproofsrecordedbelow arebaselineonly,
rootdeltaindependentreview+actualrawCLIrootreplacementproof stillpending. Do not
claimnewmaincorrectness fromoldproofs. Tasks1of4/allacceptanceopen. NoPRyet.
NO localexecutionlease; allhistorical731/739sessionclaimsbelow areterminal/obsolete.

Installed050 exactrootMCPbriefreturneddependency_closure_limit/fullchanged10001 for
4source+memorychanges; safeclosurelimitrefusal is not yet establishedasbug. Preserve
privateDB/config/purposes; refreshsameindex AFTERmainintegration. No reset/reinit.
Mainintegrationneeds accepted731 DER24/SEM23+741managed-stateadmission. Reconcilefinal
DERstrictlynewerthanALLprivatecandidates, preserveacceptedSEM; do notpublishstale12.
Acceptedreconcile-external-git-excludes fullspec/liveissue reread; directconfiglimits,
canonicalgix-config+ignorematching, boundedwatch/queue/cancellation andDBkeysetprobe
remaincontract. Preserveexplicitoperatorcapabilities. SharedDocprefixguard#738must
protectbothprojectionandnewprobe whencombined; investigateowningboundarybefore
claiming739complete. Latest741No localbuild/indexprooflease remains.

IntegrationglobalIssueOps foundpreexisting#7082.3livechecked/mainunchecked; preserve
private708work andrecordforitsowner, don'tunchecktohideprogress. Current739scoped
mirror remains1of4. QueuedCodexCIhandoff after741 closure isworkflowgapreviewonly:
syntheticmergecandidate verification/focusedaffectedPR+fullRCproof, notparallelissue
implementation. See/tmp/projectatlas-codex-ci-followup.json. Goalcontinues739->738->
708->715->RC1non-draftLatestfalseindependentlyverified; stablepublication/milestone
closeoutrequireslaterinstruction. Olderheadersbelow historicalwhereinconsistent.

# Active context

## Current root-watch delta and cross-lane lease (2026-10-29)

Committed6eb0dfd now has uncommitted runtime.rs, graph_projection.rs, policy_watch.rs,
source_observation.rs and memory changes. Preserve them. Confirmed native same-path
sourceROOT replacement with externalDB left recursivewatch on oldinode; causal
native test failed before and passes after event-retained conditional rootrebind.
No unconditional warm-read rewatch. Atomic hint survivesqueuedrain; nativecallbacks
performnoI/O, normal source/runtime/UnicodeexternalDB events remainfiltered. Same
caller budget/errorpropagation retained; restorehint onfailure. Pure9policywatch+
35observertests PASS. CLIunits372PASS, realpolicyCLI/MCP/linked/6FIFO E2E PASS,
3inventory+affectedalltargetClippy+strictstringsPASS. Receipt/logs
/tmp/projectatlas-739-root-replacement-{negative,positive}.log and
/tmp/projectatlas-739-current-cli-unit-tests.log. Graphprobe map budget preflight
moved BEFORE allocation withoutchangingestimate/limit. Whole-rootdelta Luna review
and actualrawCLI root-replacement proof stillpending. No live739build remains.

Primary nowowns731 sharedtarget lease92247 fornewgeneratorreviewreproduction;
no739build/index/benchmark concurrently.731public6639b0bd/nativepush51404 terminal
PASS/current4platform+Rust+verify/pr-statePASS. Final731DER/SEM mayadvance12for
newgeneratorrepair; final739/738combinedDER mustexceedFINAL731/allprivate andretain
FINAL731SEM. Do not publish proposedstandalone12 asfinalwithoutreconciliation.
Root715/RC1milestone37 exactchildrenANDblockers708/718/721/722/731/732/738/739
freshlyconfirmed, allcorrectparents/milestones; noopenclosedmilestone member.
NoRC exists, Latest050unchanged. Older entries below are superseded.

## Current source checkpoint and live execution lease

739 source checkpoint6eb0dfd is committed; later739dirt is memory-only. Alllatest
source checks passed: FS46,DB242,CLIunit370, realCLI/MCP/linked/policy-only/6FIFO E2E,
Clippy,3inventory,strictstrings,fullworkspaceRustdoc/doctests. Lunaapprovedcurrent
slice. Fivecausalnegatives failasrequired, source byte-restored, positivesPASS.
Stable050 exact739affectedrefreshPASS18parsed677unchanged; noDBreset.

731 accepted-main merge6639b0bd3829b122bed2e163e2c9801e2ee8252f resolves memory/map
conflicts whilepreserving own731owner+RC1child andacceptedfuture490/493graph.
NOproductioncode/dependency/workflow/toolchainchange frompublic3c9. StrictOpenSpec
andplannedIssueOps9/9/8PASS. Publicremote remains3c9 until normalpush completes;
3.1/acceptance remainopen. NativepushLIVE51404 on exactclean6639, sourceproof d0b,
log /tmp/projectatlas-731-accepted-main-native-push.log, externalreceipt
/tmp/projectatlas-731-accepted-main-native-push-session.json. Sharedtargetowner731;
DO NOT compile/run siblingCLI E2Es or benchmarks until terminal. Resumehandle,
do not restart from observation timeout. No permissionpending/noRCpublished.

After terminal, inspectgates/upload/currenthostedchecks andresolvefeedback; normal
737merge/closure onlyaftercurrentreadiness. Then consumeaccepted731+740in739,
combine738prefixguard into shareduncachedinspector/newfreshnessprobe, finalcontract
strictlynewerthanallprivate/SEM11; nativeupgrades/optimizedcost/wholeissue/full
native/hosted/installed/releaseproofstillpending. Olderprocessnotes superseded.

## Verified implementation checkpoint ready to commit

All source is restored after causal controls; no local build/index/task remains
live. 53502DONE/PASS: FS46, DB242, CLI unit370, real policy CLI/MCP E2E(all6FIFO
cases) and alltarget Clippy. 7550DONE/PASS final inventory3, strictstrings,
warningstrict fullworkspace Rustdocs and fullworkspace doctests. 98220DONE: root
and nested guard mutants FAIL as required (nested child bounded5s), byte-identical
restore, positivesPASS. Earlier13035 doc/alias/epoch mutants likewisePASS controls.
Luna Max approved current guards/probe/witness slices, no source blocker; full
issue acceptance still open for contracts/upgrades/cost/native/platform/release.
Final navigation frozen digest62cea574d7aad1541782783df9dfb83a35c76133e8cd9e6974ab6bb020f1e18f,
standalone inventory209 (finalunion218 expected incl731nine+738Docone).
Stable050 exact-root atlas watch once60s PASS, 695candidates18parsed677unchanged;
full text pass874rows/24MB retained as observed, not claimed sparse-performance
acceptance. Then short atlas next returned current Git-config source candidate.
No reset or siblingDB substitution; affected refresh only. Root remains issue739.

Commit meaningful reviewed source739checkpoint next, normalmerge acceptedmain740
and finish731readiness integration as required; retainalltask2.1/2.2/3.1unchecked.
IMPORTANT738merge: move its prefix-admission guard into shared uncached inspector
so BOTH projection and new freshness probe validate path prefixes before policy
inspection. Re-run its real CLI/MCP syscall/cancellation/sharedlinkedpositive on
combinedsource; don't preserve an unguarded probe or weaken security. Resolve any
changed danglingtarget typedreason against accepted contract. FinalDER>allprivate,
SEM11 after731included. NoRC/no stablepromotion.

## Active release and tracker authority (2026-10-29)

Continue until v0.5.1-rc1 is published and independently verified. Stable promotion
is outside the goal; keep GitHub Latest v0.5.0. Primary codes; existing Luna Max
reviews read-only using custom Rust/database/SQLite/graph skills. No permission
pending. Work only in .worktrees, preserve each exact private Atlas DB. Installed
atlas runtime0.5.0 verified again; root-manager hook warning is not runtime absence.

RC1 milestone37 v0.5.1-00: native715 children AND direct blockers exactly
708/718/721/722/731/732/738/739, all37. Closed718/721/722/732; open708/731/738/739.
PR728/737 also37. 725zero-work is explicitly deferred;720 is historicaldev backlog.
PR740 merged normally after all4platforms+verify+CodeQL/Codex checks at
3ef603aad39c435cf130545a555f8d746ec745ba. #490 stays open41 with all tasks unchecked.
Future493 owns/blocked310/314/388/490/563;490blocked310/563,314blocked310.
Consume accepted main planning updates before candidate publication. NoRCexists.

## Selected implementation: #739

Exact root issue-739, branch fix/external-excludes-739, HEAD c411202/base1b953.
Current production source is UNCOMMITTED and primary-owned. Latest plan/live body
reconcile-external-git-excludes strictPASS and plannedIssueOps4/4/1PASS. Tasks2.1/
2.2/3.1 and all acceptance remain unchecked. No derivation bump yet; proposed
standalone12/finalcombined newer than every private candidate; retain SEM11 after
including accepted731. No source from sibling worktrees silently copied.

Confirmed stable050: quoted global-excludes path spaces and linked shared info/
exclude missed; inherited ancestor targeted rules lost; ignored-parent leaf
negation reopened. Native Git oracle confirms selection. Canonical gix-config
0.62.2(defaults disabled,sha1 typefeature) + ignore0.4.34 now share one configured
WalkBuilder/full+IncrementalIgnore targeted policy. Captured direct config source/
value/path semantics, no process env mutation/includes/Git subprocess. 1MiB/file,
4MiB aggregate, regularfile/symlink positives, Unix O_NONBLOCK/nonregular refusal.
FS46PASS; exact dependency metadata38added/changed versions, maximum new declared
MSRV1.88 vs toolchain1.98, official registry/permissive licenses. Lockedfetch and
offline metadataPASS; cargo deny advisories/bans/licenses/sources ALLPASS. Inputs:
/tmp/projectatlas-739-{cargo-metadata,dependency-additions}.json, dependency-deny.log.
Binary/latency/RSS and complete native upgrade costs still pending.

External observation implemented for BOTH rawCLI/MCP: bounded256 native parents,
all selected/absent/symlink-ancestor paths, lazy onepolicy per relevant batch.
Cold explicit rebind fixes confirmed same-path directory replacement; postinstall
rediscovery bound3 catches newlyselected alias retarget before firstcoverage.
Publish selection before watchack; clear oldevents BEFORE install, preserve later
arrivals/overflow. Rootrecursive watch survives externalancestor retirement.
Pure callback prequeue filtering drops ONLY proven unrelated external siblings;
selected/root/control/rescan/Unicodecase/unknown uncertainty retained, no SQLname
specialcase. Original1024capacity/relevantoverflow assertion unchanged. Confirmed
unrelated SQLite-spill queue flood negative at observer-negative-flood.log; six
parallel32/32 repeats PASS before newest changes. Current tests8policywatch and
34observer PASS. Native replacement negative/positive logs retained. Scoped epoch
cleanup serializes preparation failure before reconcile release; delayed failed/
cancelled cleanup cannot erase successor. Same caller control covers observation
and refresh; ONLY Watcher errors get pollfallback, cancel/deadline propagates.
Existing stopped-batch test now requires propagatedcancel + explicitcallerretry;
all old state/purpose/symbol assertions retained (no original older_read weakening).
Luna approved this watcher/filter/cleanup slice, not whole issue acceptance.

Policy-only document defect confirmed in stable050 and oldcandidate realCLI:
removing exclusion for absent README link left watch reasonIgnored/gen1; forced
init reportedMissing/gen2. Same faulty assumption: sourceNode equality does not
prove derived projection equality. New DB indexed exclusive keyset pages256 over
minimal unresolveddoc inputs; same graph resolver classifies nonindexed targets,
uncached probe keeps only one page (existing sourceinventory maps budgetcharged).
First mismatch returns existing PolicyDrift/full (counts0, no fake completeclosure).
Captured source marks document projection changed, preventing staged node-only
no-op. Existing full atomic graph publisher reuses unchanged symbol parses and
revalidates target states; no schema/new metadata/hashpolicy parse invalidation.
DB index/cancel/corruptsentinel testPASS. Runtime520rows/3pages no-mismatch/late
mismatch/newindexedtargetPASS; policy-only cancel,lastvalid,0parsed,noop and offline
danglingtarget transitionsPASS. Existing generation-fence testPASS. Luna reviewed
mechanism/bounds; earlier memo finding superseded by current inspect_absent_reason.

## Native-input robustness follow-up

Bounded isolated-child stable050 tests confirmed FIFOs at common info/exclude,
root.gitignore and root.ignore block the canonical native parser (3s harness;
only isolated children terminated). Results native-policy-input-stable-results.json.
The accepted nonregular requirement now guards all native policy families at
root/ancestor and admitted nested boundaries, with regular symlinks preserved.
RootPolicy guards before construction; walker filter guards BEFORE child enqueue
(the visitor is too late); targeted matcher checks parent prefix admission BEFORE
inspecting its rules, preserving ignored-parent pruning and Git-only helper scope.
Absent/non-directory prospective ancestors skipped. First parallel guard failure
uses an atomic flag to retain only one error, stop more policy I/O/queuedwork;
all errors propagate after worker shutdown, builder dropped before Arc unwrap.
Unit root/targeted/pruned/native-regularsymlink/legacy-family/non-directorypositive
PASS, real E2E now covers six root/nested CLI FIFO cases under hard5s child timeout,
lastvalidpublicationpreserved; prior FS46+DB242/E2E/Clippy PASS. Latest first-error
flag rerun53502 pending. This is repeatable static-input protection, not a claim
of universal adversarial path-replacement or kernel-I/O preemption.

## Current verification and live process

188runtime testsPASS /tmp/projectatlas-739-runtime-current-tests.log; current
CLI+FS+DB alltarget/allfeatureClippyPASS before latest fixture-only constant edits.
StrictstringsPASS /tmp/projectatlas-739-current-strings.log. Real debugCLI positive:
initialIgnored/gen1 -> watchMissing/gen2; /tmp/projectatlas-739-policy-document-cli-
positive{,-records}.json. Stable negative retained separately. Real permanentE2E
external_git_exclusions_agree_across_cli_mcp_and_refresh PASS: quote-space global,
linkedcommon/nativeGit oracle, localwhitelist, persistent MCP typed stale read
(no mutation), zeroParsed repair, CLI/MCP agreement, identity/approvedpurpose/noop.
MCP typed freshness guidance correctly has isError=false; harness uses call_tool.
E2E log /tmp/projectatlas-739-external-policy-e2e.log. Final fixture constants only
changed after pass; source digest2a6db9b679bf3ef64212e62cdfb20fc1fa230f06eca2daa04aa783ff5955467c.
Inventory209 and frozen source-contract updated; three inventory tests passed on
prior literal-equivalent fixture hash, rerun finalhash with affected checks.

DONE13035 ran /tmp/projectatlas-739-causal-controls.py: three sequential causal
mutants (docfreshness,alias handoff,scopedcleanup), each restores source in finally,
checks byte-identical restore and runs positive. All three causal negatives failed as required; each source restored byte-identically and the positive passed. Results /tmp/projectatlas-739-causal-control-results.json
and causal-{negative,positive}-*.log. Sharedtarget owner739. No sibling builds/E2Es.
Source is restored. New live gate53502 runs FS+DB+all CLI unit/E2E/Clippy after native-input guard work; resume its handle, no sibling builds.

Next: finish causal controls, final inventory/Clippy, remaining package/docs/full
source gates and complete Luna review; commit significant working739slice, consume
acceptedmain740 and finish731acceptance/merge as required, combine731/738 without
losing contracts/source; native050 upgrades/all approved purposes/rollback/CLI/MCP/
continuouswatch/performance/dependency size and4platform proof; finalunionselectors.
738source8814d6d has reviewed Docprefixguard/standalone proof but awaits739shared
linked positive and finalcontract.731publicPR7373c9ed8a has cleanopt DER11/SEM11,
fullnativepushPASS+exact4platform/Rust/verifyPASS; finalreadiness/acceptedmainupdate
and finaltask3.1/acceptance/merge pending, memory-only dirt preserved.708closure
fixture proof and combined4platform performance remains.715finalrelease packaging/
installed/plugin/cleanparser/concurrency/upgrade/tagbootstrap/checksum/Latest proof
pending. No RC tag/release exists. Ten inherited731CodeQLstaging leads dispositioned
as confined/guarded (not dismissed); real alert244owned738 remainsopen until combined
fix analysis.715security inventory stillopen. See integration/731/708 detailed
memory for immutable artifact and campaign paths; reuse verified proof.

Execution: source /tmp/projectatlas-rc1-test-env.sh (cleanPATH,filesystemTMPDIR,
sharedtarget v0.5.1/target). Resolve the installed short atlas forwarder from PATH. No sibling
CLI builds duringE2Es; no builds/indexing duringbenchmarks. Normalhooks/protection,
no bypass/force. GitHubauthenticated. Native stableAtlas current739DBstale after
17+source/planning changes; don't use indexed selectors without affected refresh,
never reset/reinit merelyfororientation. Exact established owner reads are current.
