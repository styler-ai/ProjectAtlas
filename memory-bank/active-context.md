# Current #739 acceptance — final metadata publication next

All Eb1 hosted source proof is complete: run41534979269 SUCCESS with all four
native jobs, Rust, repository, diagnostics, plan and verify. Required verify and
pr-state passed; main6927 is unchanged. All six owning cases passed in each
platform log (24 total), verified by39141 and native-owner-proof.json under/tmp.
These jobs are TERMINAL, not live handles. All1820 local tests and normal gates
passed. Fresh35-input security assessment is complete; all21threads resolved.
The non-required aggregate CodeQL check stays red for28open/dispositioned alerts;
its seven analyses passed. No alerts/security settings were suppressed/changed.

All four implementation rows and all five acceptance rows are CHECKED local/live,
with exact readback and matching mitigations. Luna741 approves all five outcomes
with no remaining behavior/security/compatibility/test gap. Its sole metadata
finding (stale lower memory status) is now explicitly archived below. Final
fmt/diff/strictOpenSpec/scopedIssueOps passed15484TERMINAL0 after the transition.
715handoff6589398173 includes full native proof;708pointer6589401634 preserved.

The only pending work for739 is normal publication/protected validation/merge
and native closure. Commit ONLY memory/proposal/tasks, verify the metadata tree
has the same140build inputs and unchanged workflow as Eb1/frozen source907,
then NORMAL clean push. Source907/frozenCLI090 and manifest7e767a remain the
behavior-relevant proof. The final current protected checks must pass before
normal merge; reopen affected tasks/acceptance for a genuine new failure/finding.
Do not invent an exact-head-only proof loop or bypass checks. AutoMerge is off.

Goal remains ACTIVE until739nativeCLOSED, then738,708,715RC1 publication and
independent installed verification. Stable0.5.0 remains Latest; no stable promotion.

# Historical pre-acceptance checkpoint — superseded

Archived status below: task counts and live-job labels describe the earlier
checkpoint. Use ONLY the current section above for readiness. Technical artifact,
security-scope and parked-work details remain historical supporting evidence.

## Goal and ownership

Goal ACTIVE: finish, normally merge and verify native closure of #739/PR745,
THEN #738, THEN #708/PR728, THEN #715 RC1 acceptance/publication/independent install.
Publish v0.5.1-rc1 non-draft prerelease, Latest=false; stable0.5.0 remains Latest.
Stable promotion, #715 stable task3.2 and milestone closure need later instruction.
No approval pending. Primary writes; independent Luna Max reviews load custom
Rust/cognition/workspace skills (+database/SQLite when relevant) and accepted spec.
Preserve all parked work and the three private #715 spec edits in integration.

## Current selected/public state

Root .worktrees/issue-739, branch fix/external-excludes-739.
PUBLIC/local HEAD eb1f2fdc25b5a893cb08785ec36c5e94b6d044e3; PR745 OPEN/unmerged.
Source commit90719d9149c175c6f48fd80dfeafcd9271be141e. Main6927ed0ca64788db6947b4d43eafce29aac5ed61
last verified unchanged. Owned post-push dirt: this memory, proposal.md and tasks.md.
Task1.1/2.1 CHECKED local/live;2.2/3.1 and ALLfiveacceptance UNCHECKED. Both2.1
mitigations checked;2.2/3.1 mitigation open. Exact live readback + strictOpenSpec/
scopedIssueOps passed63262TERMINAL0. Current published tasks still pre-progress;
final metadata sync is pending, no Closes yet. Never merge stale task/body state.

Normal push53693/wrapper1361732/git1361736 TERMINAL0 at2026-11-02T22:10:35.611148Z.
DO NOT repoll as live. Receipt/log/script: /tmp/projectatlas-739-document-rows-push*
All1820PASS/0FAIL/12ignored34targets + required workspace check/Clippy/fmt,
doctests/rustdoc, dependency/security/strings/OpenSpec/IssueOps, actual scan/lint.
Clean after push; only subsequent owned metadata is dirty. No local build/test
process active. PR body rewritten to current candidate and EXACT app readback.
Detailed earlier implementation continuity lives in GitEb1/A5 memory and the
pre-consolidation backup /tmp/projectatlas-739-memory-before-native-gates-consolidation.md.

## Live hosted handles — poll SAME jobs

SOURCE41534979269 atEb1: plan125585156397, repository125585308228,
Rust125585308245, Linux125585308361 and diagnostics125585308475 SUCCESS.
ARM125585308397 nowSUCCESS; Intel125585308389 andWindows125585308473 LIVE.
Partial proof helper /tmp/projectatlas-739-eb1-native-proof.py,53460TERMINAL0,
verifiedall6owning cases in BOTH Linux/ARM rawlogs. Partialreceipt
/tmp/projectatlas-739-eb1-native-owner-proof.json; runSAMEhelper forremainingjobs,
it downloads only newly completed logs andneverrestartsCI. Last2native jobs have
progressed through resource proof into installed-host/release-asset checks.
All FOUR Native source-policy steps SUCCESS, including exact new row regression;
that earned task2.1. Remaining full native jobs are required for2.2/readiness.
MacIntel/Windows document-resource steps began22:36:04/22:33:54Z,30min caps;
these compile release CLI/harnesses and run actual document resource proof.
ARM progressed to plugin-update E2E. No hosted test failure reported.
Required metadata verify41535139625 waits on source; PRstate41535140393 SUCCESS.
Use current PR check rollup rather than assuming these metadata IDs stay latest.

CodeQL41534971844 TERMINAL ALL7SUCCESS. Rust2081336138=26 alerts,
Actions2081236583=2, other five languages0. No need repoll as live.
OldEc3source41528395756 CANCELED on superseding push, and oldmetadata41528416865
failed its correct live-head-change check. Neither is a product defect/restart.

## Latest row-budget fix and proof

Security report4448000523 was CONFIRMED onEc3: normalize-error, indexed-heading
and case-conflict shortcuts bypassed both matcher/prefix work quotas while
paging continued. Repaired at shared cold verifier BEFORE every shortcut with
one cumulative1,000,000 returned-row allowance and original control check first.
Private DocumentRows marker retains full PolicyUnavailable incomplete guidance,
serde-skipped wire compatibility, and no incremental/observed-MCP quota retry.
256-row keyset pages + one reread sentinel/page + at most one current over-limit
page bound read-ahead; no all-row materialization. No SQL/schema/API/dependency/
DER change. Existing policy128MiB/1Mcontexts and4Mprefix quotas remain.

Causal beforeguard56953 TERMINAL101 failed intended outside-link quota assertion.
Two later fixture-only failures were parser rejection of dir//target/bare'.';
replaced with actually admitted nested-colon/invalid-fragment selectors, not new
product bugs. Corrected85206 1PASS2.64s. Final91306 TERMINAL0:40document tests,
MCP quota/recovery1test, CLIalltargets/allfeaturesClippy/strictstrings/fmt/diffPASS.
Six families cover zero/page-boundary/one-short/exact allowances,10,800-row default
positive, cancellation and publication/identity/purpose preservation. ActualCLI
10,801-row cold/noop/later-target positive alsoPASS. Logs document-rows-* under/tmp.
Luna741 source/spec/read-ahead wording, exact4nativeCIselector and costpacket APPROVED.

## Frozen artifact / costs / compatibility

Clean default release55444 TERMINAL0,40.91s. Immutable directory:
~/.cache/projectatlas-benchmarks/external-policy-739-document-rows-release
CLI090ad69356456a4077650e336fa6831faf342d6f1b8bd3cb1c92c4697a8fc488/70000320B.
140-input manifest7e767ae5332ae90ff8dffb42b2f489338a981fb484c763e6346e93bf1bbe5cab.
FShelper544eb8349e6803c427b11382b32b4297e071336f66b547c465ddc8a339d83fc9/8234512B
byte-identical. All140 saved/Git907/Eb1 inputs verified; metadata binding receipt
/tmp/projectatlas-739-document-rows-metadata-binding.json. Only3inputs differ
fromA5:runtime.rs,graph_projection.rs,mcp.rs; unchanged E2E/support/build inputs
permit prior harness reuse with EXPLICIT fresh candidate executable overrides.

Fresh18CLI samples,4stable050/main ordinary+linked upgrade/rollback cases,
2DER25->26heading cases,2resource fixtures and6optimized CLI/MCP cases ALLPASS.
Initial73000 TERMINAL1 ONLY adapter harness identity selection BEFOREexecution;
selected correct known concurrent54b harness without relaxing hashguard and
reran ONLY adapters43987TERMINAL0. Lifecycle harnessb6cc575... from normal push.
72412 TERMINAL0 verifies CLI10801positive and complete1000links/depth200,
2guides x500/500complete0omitted, plus1024persisted cold-policy rows/2000rules.
Source receipt source_dirty:false; no overlapping builds during measurements.
ONLY18FScomponent samples reused via exact helper identity + original overlap
limitation. Never overwrite immutable receipts/artifacts.

Current scoped Linux warm-cache/unisolated figures main->candidate:
ordinaryinit309.05->312.71ms/noop48.27->49.71ms;10krulesnoop70.95->97.89ms;
completeprefixcold3.44444s; coldpolicy median.069673s; size+543360B/.7823%.
NOT #708SLO, persistent-MCP latency or four-platform performance acceptance.
Published docs/benchmarks/external-policy-cost.{md,json} bind these exact inputs.
Handoff715comment6589398173 and708pointer6589401634 BOTHexactreadback:
https://github.com/styler-ai/ProjectAtlas/issues/715#issuecomment-6589398173
https://github.com/styler-ai/ProjectAtlas/issues/708#issuecomment-6589401634
Earlier7156588173733 preserves exact selector/predecessor/rollback requirements.

## Security assessment COMPLETE and phase LEFT

No active static phase now. Archived35 current-source assessment rendered first;
then fresh WHOLEEb1 collection normalized BEFOREverdict:28native+6SecurityReview
reports+RUSTSEC-2026-0433. All35 assessed inline against publishedEb1 and RENDERED35.
No tests/builds/runtime apps/agents/repo edits were initiated during either phase;
explicitly LEFT each phase before subsequent workflow changes.

28 fresh native complete facts/ordered flows equal6f9 after exact line mapping
and verified renderer reconciliation; explicit URIs/regions/messages/severity/
rules/properties/every flow retained. All2713reportedregions unchanged.
All119priorguardanchors accounted113whollyunchanged+6manual current inspections.
All6freshREST review bodies equal originals. Preserve original_commit_id/line
separately from GitHub moved current position; newrow originalEc3 != mappedEb1.
Intake helper corrected this normalization BEFORE current collection verdict.
All35not_actionable onFIXEDsource; genuine historical resource/row/advisory
conditions explicitly remain confirmed/FIXED rather than false positives by SHA.
No exhaustive clean-repository/universal filesystem availability claim.

Canonical/raw/flows/instances: /tmp/projectatlas-739-document-rows-exact-*
Current result: document-rows-complete-triage.json; rendered-triage.json uses14
explicit sink excerpts to satisfy app1200charlimit; fullclaims remain canonical.
Successreceipt document-rows-triage-render-success.json. Archived-* files remain
separate and qualified. Helper scripts document-rows-{final-triage,native-compare,
assessment-location-map,source-map,exact-intake}.py. No unsupported new input was
auto-classified; fresh/current collection IDs and semantic flows all reconciled.

IMPORTANT: GitHubAdvancedSecurity aggregate CodeQL check125584652182 remainsRED
from28OPENalerts(1critical/27high), even though7analyses SUCCESS. Critical410 is
private cfg(test) current_exe re-entry with FIXEDlibtestargs/no shell/no shipped
request path, runtime.rs15836. Luna741 independently verified this and current
source-specific triage: no product-code fix/dismissal needed; first4technical
review dimensions supported SUBJECTto remaining hosted proof. Do NOT claim
wholeissue ready yet. Alerts NOTsuppressed/dismissed, security config unchanged.
Live main protection strict verify/pr-state App15368ONLY; branch rules API[];
gh pr checks --required confirms verify pending/pr-stateSUCCESS. CodeQL aggregate
is not a required check. Public security summary6563449602 UPDATED/EXACTreadback
states this explicitly; don't equate analysis success with aggregate green.

CodexEb1 code+security COMPLETED; codefalse/false,securitynull/null. Fullpaginated
post-completion RESTcomments/reviews showNONEWbotreport;6report bodies retained.
Null alone was NOT absence proof. All21threads RESOLVED; bundled thread readback
/tmp/projectatlas-739-document-rows-resolved-threads.json. Newrow reply4448558259
EXACTreadback before resolvingPRRT_kwDOQyFgJ86y34bi. AutoMergeRequestNULL checked.

## Next actions

1. Poll SAME2remaining native/source handles (Intel/Windows); no restart on observation timeout.
2. After native completion fetch raw joblogs with gh api --allow-escape-sequences
   into files, never emit ANSI. gh run view --log is unavailable until whole run
   ends. Earlier55179/53970 failed ONLY log retrieval, not jobs. CorrectedLinux
   rawdownload78138 TERMINAL0 at /tmp/projectatlas-739-eb1-native-linux.log.
   All6owning cases verifiedPASS there (4CLI/MCP +pollhandoff +newrow unit).
3. Require current4platform owning cases and source aggregate/metadataverify,
   then check earned2.2, reconcile final independent readiness,3.1/fiveacceptance
   with exact live/task/mitigation state. Never silently accept missing proof.
4. Normal clean metadata publication; reuse proven behavioral/artifact inputs
   honestly (no blanket exact-head proof rule), retain all required native checks.
   Normal merge/no admin/no force/no bypass/no selfapproval; verify native739CLOSED
   and acceptedmain exact inputs BEFORE starting738. Don't invent new approval.

OrdinaryRefs source integration plus same-owner acceptance-onlyPR is permitted
by workflow, but Luna notes earned2.1/2.2 must be checked promptly and exact
local/live sync retained; no decision to bypass final source/task gates was made.
Current intended full issue acceptance remains required, no prematureCloses.

## Tools / parked work

source /tmp/projectatlas-rc1-test-env.sh; shared target .worktrees/v0.5.1/target,
TMPDIR~/.cache/projectatlas-test-tmp. Rust units --bin projectatlas, not--lib;
strings cargo projectatlas-lints strict-strings. OpenSpec cachedCLI:
~/.npm/_npx/21a1280729add0a7/node_modules/.bin/openspec
IssueOps: `python3 .github/scripts/issue-checklists.py --repo styler-ai/ProjectAtlas --pull-request 745 --base 6927ed0ca64788db6947b4d43eafce29aac5ed61`.
Thread-aware script under usb-import/github/0.1.6+local.1/skills/gh-address-comments.
Atlas050 exact739 has policy_drift/full-refresh guidance; preserve private DB and
approved purposes, no downgrade/reset/scan merely for orientation. Use bounded
current slices; no stale graph claims. Read installed version-matched skill after
compaction before Atlas calls. Linux login:false; NEVER sandbox_permissions.

731/737 and741/743 MERGED/CLOSED.738 parked8814d6d+ownmemory;708 publicc65dfed,
local587ecbe+ownmemory. Fullwarm1.127 FAILED/focused1.031526 PARTIAL; no SLOwaiver.
Integration715three private spec edits preserved. Later715evaluate744 outsideRC.
Stable0.5.0 Latest;RC1unpublished at last check; goalACTIVE until actual verifiedRC1.
