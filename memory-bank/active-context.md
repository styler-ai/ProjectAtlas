# Active context — #739, v0.5.1-rc1

## Objective and ownership

Finish and normally merge ONE issue at a time: #739 → #738 → #708 → #715,
then publish v0.5.1-rc1 non-draft prerelease with Latest=false and independently
verify downloaded installed artifacts. Stable0.5.0 remains Latest. Stable release
publication/milestone closure need later user instruction. #731/PR737 and
#741/PR743 are merged/CLOSED. Four RC issues remain OPEN; RC1 unpublished.

This checkout: .worktrees/issue-739, fix/external-excludes-739, PR745 (OPEN,
Refs #739). Accepted main/base6927ed0ca64788db6947b4d43eafce29aac5ed61 includes
#731/#741. Current public candidate a2ccd7ba370c26d99b7fe5b492e6ac46181c1a46.
Native GitHub app readback verified the uploaded SHA and updated PR body exactly.
Only implementation task1.1 checked;2.1/2.2/3.1 and all5 acceptance tasks remain
open until remaining current-head platform/security/readiness proof is complete.
Live739 scope is expanded and exactly read back. Strict OpenSpec and planned
IssueOps PASS. Architecture N/A: existing filesystem and publication owners.

## Accepted mechanisms and broader bug scope

Use gix-config0.62.2 defaults-off/sha1 and ignore0.4.34. Direct configuration
sources/values/paths are bounded (1MiB/file,4MiB aggregate), section-aware with
last-value/source precedence; include/includeIf and full Git loading remain
explicitly unsupported. Common info/exclude outranks global; local rules outrank
both. Preserve no-Git .gitignore, inheritance, ignored-parent pruning, whitelist
controls, regular-file symlinks, explicit operator config/DB/root capabilities.
Static FIFO inputs are refused at root/ancestor/admitted nested owners before
parser opens, without loading pruned child policy or disabled ignore families.

RootScanPolicy captures original lexical selected root, anchors relative CWD once
and rebases absolute API targets before policy matching without canonicalizing
targets; canonical plans/persistence remain unchanged. Native non-UTF8, APFS
EILSEQ/Unicode and Windows ordinary/verbatim-path controls remain intentional.

Cold policy observation rebinds selected external parents and recursive source
root after supported directory replacement. Queue1024/registrations256/
convergence3 retained; no callback filesystem/policy I/O or unconditional warm
rewatch. Structural marker/event/sequence share ingress fence; receiver waits
release mutex before callback progress. CLI exact-root rename-gap uncertainty
survives original debounce; unrelated failures propagate, no added retry budget.
Mac synthetic source paths model canonical native registrations, with actual
native selected/canonical distinct-leaf controls.

Policy-only document freshness uses existing unresolved-input keyset pages256,
unchanged parse reuse and atomic full projection. Fresh policy from original
ScanOptions revalidates staged outcomes before transaction. Shared #738 native
prefix admission protects cached, uncached and staged consumers, escape/reentry,
under original control. #738 still owns its separate complete acceptance.

Document-only source changes use WatchChangeSet::has_changes at the ingress
fence, invalidating warm epochs even with no source paths. Read-only queries
retain typed refresh guidance and last-valid publication; explicit refresh fixes
ignored↔outside_root under local/global rules with zero unchanged source parsing.
Do not revert this repair to hide database self-events.

Windows source CI41366456477 at2c206 failed managed short-name DB MCP reads;
Linux/both Macs/Rust/repository passed. Linux external operator DB symlink
reproduces the same actual SQLite target-WALCreate false invalidation. A distinct
source symlink to the DB separately reproduced stale document missing/outside_root.
Current owning repair preserves selected SQLite opens and captures target in
process-local observer identity. Runtime filtering compares native parent/entry
namespaces over at most two bases and three sidecars, preserving main structural,
retarget/unknown, distinct source symlink/hardlink, metadata-directory alias,
config and policy events. Existing Windows literal-name and actual directory-case
helpers reused. No schema/public surface/contract increment/reset/blanket metadata
exclusion. DER25/semantic23. One target canonicalization per observed REQUEST,
including warm reads; no isolated persistent MCP warm-latency claim. Unconfirmed
review edge: exact DB base with a case-variant removed suffix remains observable;
SQLite emits fixed lowercase suffix, no supported producer failure established.

## Current exact artifact, costs and causal proof

Immutable ~/.cache/projectatlas-benchmarks/external-policy-739-cohort-release.
Fresh locked default release58728 TERMINAL0,41.26s. CLI SHA256
767986fa05e456222952716077d87eede317effed433d55ec09cb06840de700c,
69,784,464bytes.139-input manifest7870a1cf6276003fa4828baa2c71489febbea8d8231abb00e540c6582e7013c0.
All139inputs match a2 Git tree: /tmp/projectatlas-739-cohort-commit-source-binding.json.
FShelper68663dbeaf98788375eb33213c0b472b4e0fcc3c3bd5da176089e79113aebb72
relinked byte-identically; ONLY18FS component samples reused. Fresh4upgrade/
rollback cases and18CLI samples PASS. Optimized permanent adapter scenarios PASS:
lifecycle2.03s, document2.14s, externalpolicy1.26s, rootreplacement8.13s.
Logs/receipts /tmp/projectatlas-739-cohort-{upgrade-proof,cli-cost-proof,optimized-*}.
Both new permanent MCP cases fail against prior frozen c41b at intended assertions;
negative logs /tmp/projectatlas-739-cohort-{lifecycle,documents}-negative.log.
Native WALCreate negative and distinct DB-object alias negative are preserved in
/tmp/projectatlas-739-{cohort-native,database-document-alias}-negative.log.
Observer42PASS2.22s; two debug E2Es1PASS4.36s/1PASS4.24s; Clippy/source-policy PASS.

Inventory remains224tests/82selectors; normalized lifecycle9029b55bb0cfdd5e65386c2d9f74242a1a28c9a52c704dc1c7d845b67735bc21,
navigationb226b2463f3f7ceb2690be6f76a3380ca23f0fb125196ef2a62076b56637c0fe.
Inventory positive/tamper3PASS2.70s. Historical baseline/facet/symbol/fixture and
enforcement contracts preserved. Cost docs bound exact CLI: +327504bytes/+0.47%
vs acceptedmain; ordinary init292.7→292.8ms/noop49.9→49.6ms;
10krulesnoop71.0→102.0ms. No broad speedup or #708 SLO waiver.
Luna731 approved source/spec/E2E and broader acceptance reconciliation; Luna741
approved inventory/cost packet. Both require current Windows hosted proof.

## Terminal push and current hosted proof

Normal push55829 TERMINAL0; wrapper2545981/git2546125 finished2026-11-02T01:51:19Z.
/tmp/projectatlas-739-cohort-push-{session.json,log}. All34suites:1783PASS/0FAIL/
12ignored plus locked Rust/docs/dependency/repository and required Atlas gate.
No local push/build/index remains live. Old51590 terminal0 uploaded2c206;77584
terminal1 inventory failure corrected;11389 intentionally SIGTERM143 for P2,
not timeout. Never restart terminal handles.

Fresh source CI41374636469 LIVE: plan125052113731 PASS; MacARM125052149384,
Linux125052149393, repository125052149413, Windows125052149455,
MacIntel125052149464, Rust125052149830 and Windows diagnostics125052149418 live.
CodeQL41374632974 LIVE all7languages. pr-state41374634060 SUCCESS. Poll exact
run/job IDs; no hosted source/platform acceptance claim yet.

Parent2c206 CodeQL41366452455 SUCCESS:13Rust/1Actions/other5zero. Complete Rust
facts/flows equal16338 after exact unchanged-source mapping; Actions2flows
separately reassessed for one fixed CI selector. Artifacts /tmp/projectatlas-739-
2c206-{rust-reconciliation,actions-reconciliation}.json. No clean-scan claim,
suppression or alert dismissal. Prior14-input triage prepared at16338; fresh a2
per-input scan reconciliation/render required. Public security comment6563449602
still describesbad7. Codex2c206 code completed/no findings, security completed;
current a2 bot state pending. Old7threads resolved. Document-only P2 thread
PRRT_kwDOQyFgJ86yfjX1 remains OPEN pending current public fix/platform verification.

## Next and preserved work

Monitor exact-head CI/bots, fix confirmed findings in this owner, earn task and
five-row acceptance checks, refresh live/task/PR/continuity, normally merge745
and verify native739 CLOSED before738. Do not force/admin/bypass or self-approve.
Final task/continuity-only changes may reuse expensive proof only with verified
unchanged behavior inputs/artifact identities; required protected checks remain.
Use stable050 complete installed Atlas skill, exact root/private DB and approved
purposes. Previous orientation returned bounded dependency_closure_limit10001;
normal pre-push candidate scan has now refreshed its own required gate. Do not
reset/restart navigation blindly or treat a timeout as process termination.

#738 parked8814d6d plus dirty memory; #708/PR728 publicc65dfed/local587ecbe plus
dirty memory preserved (warm1201.127ms FAIL, focused1.031526ms partial, no waiver).
Reconcile its live/main task2.3 drift when accepted; don't uncheck private progress.
Integration .worktrees/v0.5.1 has3 private #715 spec/design/proposal edits; preserve.
Later715 evaluates PR744 outside RC milestone and queued Codex CI gap. Integration
entry point .worktrees/v0.5.1/memory-bank/active-context.md; prior708 checkpoint
.worktrees/issue-708/memory-bank/active-context.md remains preserved.


## Superseding working checkpoint — controlled watch fallback and Mac fixture

Public head remains a2ccd7ba; new repair is UNCOMMITTED. Source CI41374636469:
Linux/Rust/repository/windowsdiagnostics/plan SUCCESS; MacARM125052149384 FAILED
only new database_alias_retarget_and_policy_events_remain_observable (41PASS1FAIL).
Windows125052149455 and MacIntel125052149464 still LIVE at last poll. Windows
managed-state ownership/short-name step SUCCESS; no final all-platform claim.
Mac log /tmp/projectatlas-739-a2ccd-macos-arm.log. Attempted live Windows log read
returned404 BlobNotFound; retry only after terminal, not proof of absent tests.
CodeQL41374632974 SUCCESS all7: Rust2073822965 has14results, Actions2073773340
has1; other5zero.13existing Rust full facts/flows and1Actions full result/twoflows
match parent after exact unchanged-line mapping. New414 is private cfg(test)
fs::copy of TempDir-owned DB in alias-retarget test; four complete flows include
fixed write, lease/SQLite Result aggregate taint, never assignment to DB path.
No claim that copy is metadata-only. All15 current native alert instances fetched;
complete static dispositions prepared /tmp/projectatlas-739-a2ccd-complete-triage-prepared.json.
Not rendered yet; successor scan/reconciliation/render mandatory before final
security summary. No dismissal/suppression/clean-scan claim. Public oldbad7 comment
is not current evidence. Newcurrent15input staticfacts are reviewable artifacts.

Codex P2 PRRT_kwDOQyFgJ86ygrn7/comment4438294526 confirmed: continuous notify loop
switched to controlled entry but lost fallback from now-test-only convenience
wrapper. Exact production-entry oversized test0PASS1FAIL Entries100000/100002;
/tmp/projectatlas-739-controlled-fallback-negative.log. Real CLI onefile edit on
large persisted graph also exits1 DependencyClosureLimit with last publication,
identity and symbol count unchanged: /tmp/projectatlas-739-watch-closure-{native,cargo}-negative.json.
Current repair uses attempt_incremental_refresh_controlled returning Some(report)
or None(fullrequired BEFORE fullwork). Watch, assessment and hydrated consumers
route full selection separately and recover ONLY incremental-limit errors under
original control. Raw read-side facade retains its own error/admission semantics.
Initial naive outer catch would repeat already-full failures; Luna caught it and
it is FIXED at shared outcome owner. No heap allocation or new polling budget;
closed native Option avoids boxing208byte report. Existing predicate still only
incremental closure/entry/source-byte limits; cancel/deadline/unrelated propagate.
Selection test covers fullrequest/contractmismatch/deletedfolder before publication.
Oversized synthetic source AND document-only batches preserve stopped controls,
identity/purposes, invisible addition and one complete generation. Do not describe
100k event fixture as native notify queue reproduction (queue1024 remains).

Permanent source_root_replacement CLI test starts with a large persisted graph,
prepends watcher_ready before initial watch, changes one file to closure_target,
compacts, then proves root replacement and later-child edits. Placing growth only
after replacement or starting with a small graph let oldCLI pass; corrected
causal old767 run0PASS1FAIL4.47s atclosure_target DependencyClosureLimit changed1.
/tmp/projectatlas-739-watch-closure-permanent-negative.log. CurrentdebugPASS36.97s,
optimizedPASS14.35s. Largefixture phase bound60s replaces smallfixture10s and
cycles8→10 preserve replacement margin; this is NOT an indexing/performance SLO.
Wrong footprint.truncated precondition was removed (not the limiting graph count);
actual oldfailure is the causal proof. No fake bound assertion.

Mac retarget fixture injected raw selected-root event spelling outside captured
canonical root. Forced root symlink reproduces exactfailure onLinux0PASS1FAIL;
/tmp/projectatlas-739-cohort-native-root-fixture-negative.log. Correct event paths
use binding.root.join(selected.db) while raw operator DB/root/symlink I/O remains.
Full42observerPASS2.33s, Clippy/stringsPASS; no production normalization change.
Other new fixtures already use binding.root/canonical source paths. Luna731
approved fallback source/Linux adapter and bounded Macfixture/spec correction;
fresh Mac/Windows/other hosted proof still required. Live739 expanded fallback
scope read back exactly; technical P2reply4438359277 read back exactly, threadOPEN.
Old document-only P2thread also OPEN. Only task1.1 and no acceptance rows checked.

Final artifact (supersedes prior767/c41b evidence for current working source):
~/.cache/projectatlas-benchmarks/external-policy-739-controlled-fallback-release.
Freshdefaultlockedrelease19637 TERMINAL0,38.51s, then freeze/proofs52557 TERMINAL0.
CLIad884f80ca0f3f599ff418c85fd6c3136a79715f58d8e6a38731df5df3a5bf5a,
69784176bytes.139manifest8d67807423652d68d051345e70c61cd44a21b8c6cb795fd3556ba0cc19e3f93f.
FShelper68663 byte-identical, ONLY18FScomponent samples reused. FreshCLI18samples,
4upgrade/rollback cases PASS. Optimized lifecycle2.03s/document2.09s/external1.31s/
replacement+closure14.28s each1PASS. Costdocs +327216bytes/+0.47%, ordinaryinit
304.8→314.9ms/noop48.6→48.4ms,10krulesnoop adds27.2ms. No speedup/SLO waiver;
recoverybranch and persistentMCP warmlatency not isolated by CLIcost fixtures.
Full CLIbin458PASS0FAIL2ignored37.81s; currentMacfixture42PASS overrides its old
fixture slice. Current Clippy/strictstrings PASS. Navinventorydigest
3f4970c20b1c049ca98a57630034da03017d6809cdc1d2fe8fb315114b8a4440;224tests/82selectors
unchanged; positive/tamper3PASS2.72s. Localmetadata only may change after freeze;
verify all139 source inputs against committed Git tree before normalpush.

Next: final cost/inventory review by Luna741, cheap strictOpenSpec/plannedIssueOps,
sole(#739) commit,139inputGitbinding, normalpushwithhooks. Keepissue739tree CLEAN
while pushlive, updateintegrationmemoryonly. Then fresh exact-head CI/scans/bots,
new15input scan reconciliation/render and all feedback/task/acceptance gates.
Normallymerge745/native739CLOSED before738. Goal remains active; RC1 unpublished.
