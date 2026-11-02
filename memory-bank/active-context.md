# Active RC1 checkpoint — 2026-11-02

Finish, normally merge and verify native closure of ONE issue before the next:
#739 → #738 → #708 → #715. Then publish v0.5.1-rc1 as a non-draft prerelease,
Latest=false, and independently download/install/verify it. Stable v0.5.0 stays
Latest. Stable publication and milestone closure need later user instruction.
Primary implements; independent Luna Max reviewers are read-only. Preserve all
parked work and private databases; no reset, force push, admin merge or hook bypass.

## Current source and tracker

Exact checkout: .worktrees/issue-739, branch fix/external-excludes-739.
Accepted main/base: 6927ed0ca64788db6947b4d43eafce29aac5ed61.
PR745 is OPEN/READY, sole owner #739, Refs rather than Closes. Public head is
16338ba089db32a086f8f2f410c61ae10191e1b9. Local HEAD is
03c7d4fd13eb0da2f7365e87390472846f4de1a5, with the inventory correction and its
source-binding documentation ready for the next normal commit/push. Read Git.

OpenSpec: reconcile-external-git-excludes, spec-driven. Only task1.1 is checked
(1/4); 2.1,2.2,3.1 and all five acceptance rows remain open. Issue739 scope and
PR745 description were updated/read back. Planned IssueOps and strict OpenSpec
pass. Do not close #739 before exact accepted-source feedback and platform proof.

## Current fix and causal coverage

The new Codex P2 thread PRRT_kwDOQyFgJ86yfjX1/comment4437820780 is confirmed.
SourceObservationEntry::drain_source_events discarded document_paths in its
changed predicate and acknowledged the event without invalidating a warm epoch.
A real persistent native MCP reproduction against CLI48a4 retained `ignored`
three times after an ignored target became an outside-root symlink; an explicit
refresh then produced `outside_root`. Native negative receipt:
/tmp/projectatlas-739-document-epoch-native-negative.json.

The production fix uses existing WatchChangeSet::has_changes(). It invalidates
read/mutation epochs under the receiver fence without requesting recursive rebind
for ordinary document-only events. The ordinary watch loop already used this
predicate; no analogous duplicated predicate remains. Access, database runtime
and unrelated outside event filters are unchanged. DER25/semantic23 remain correct:
this repairs process-local evidence, adds no schema/public route/query mutation.

New all-platform deterministic unit proves document-only classification, warm
acceptance refusal, cleared epoch and no recursive rebind for .ignore/.gitignore/
ignored assets. Old predicate:0PASS/1FAIL; fixed observer suite:39PASS/0FAIL.
New persistent MCP E2E covers both Ignored↔OutsideRoot directions under .ignore,
no-Git .gitignore and selected global rules. It requires typed refresh_required/
policy_drift before explicit refresh, unchanged publication on reads, zero source
parses on refresh, two correct warm successor queries, unchanged project identity,
all approved purposes/revision and no admission of ignored/outside source.
Windows creates an actual file symlink without a skip; hosted privilege proof is
still required. The selector is in the existing four-platform policy step.

The current permanent E2E fails against old CLI48a4 at the intended stale ignored
reason and passes against fixed CLIc41b. One initial verification assertion used
wrong quote escaping; negative-result.json records this without rerunning the test.
An initial positive assertion expected automatic reconciliation; the final test
correctly preserves existing typed read-only refusal followed by explicit refresh.
Logs: /tmp/projectatlas-739-document-epoch-{unit-negative,observer-positive,
current-e2e-negative,e2e-positive,optimized-e2e,external-policy-e2e,
root-replacement-e2e,clippy,strings,openspec,issueops}.log.

Luna731 approved the owning fix, broader consumer audit, tests and specification.
Luna741 approved cost documentation and the later inventory correction/reuse proof.
These are bounded approvals; full issue acceptance still requires current-head
hosted/platform/security results. The P2 thread has a confirmed-fix/local-proof
reply read back exactly and remains OPEN until the uploaded fix is verified.

## Inventory correction and final artifact

Normal push77584 TERMINAL1 before upload:636PASS/3FAIL. All three failures were
E2E inventory/tamper checks because the new navigation test lacked inventory
registration and source digest updates. This bookkeeping miss is corrected in
support/source_contract.rs and docs/v050-cli-e2e-inventory.json: actual navigation
digest17818350c4aa123003b00b04facea6d8b2934fe1b6d6ecb79cf755805cd6f4aa,
224 tests and82 selectors, with the new test and exact CI command registered.
Historical baselines/facets/symbols/fixtures/enforcement checks are preserved.
There is no repository generator; existing checker normalization/registration
rules were applied directly. Positive and tamper inventory checks:3PASS/0FAIL,
2.69s, /tmp/projectatlas-739-document-inventory-tests.log. Luna741 approved.

Fresh locked default release build33939 TERMINAL0,41.08s, is BYTE-IDENTICAL to
the measured production CLI and FS helper. Current immutable artifact directory:
~/.cache/projectatlas-benchmarks/external-policy-739-document-inventory-release.
CLI:c41b3bd19921bda4a5b11dcd1a81543aea059926948195ebbefa4152c6e2544d,
69,807,752bytes. Current139-input manifest:
097ead7d2827bb0130a2775be21d9bcba34996956507176ef266a68d4a5d6640.
Only source_contract.rs differs from the measured139-input manifest
 ebc7ab6a6da4e38aeb85bce592b166c66ea897b9ce790d4f7eb8849df284c536.
FS helper:68663dbeaf98788375eb33213c0b472b4e0fcc3c3bd5da176089e79113aebb72.
Receipt and /tmp/projectatlas-739-document-inventory-proof-reuse.json prove reuse.

Reuse exact c41b proof from completed87423:4 upgrade/rollback cases,18 CLI samples,
optimized document-transition1PASS1.71s, external-policy1PASS1.28s and source-root
replacement1PASS8.09s. Only18 FS component samples reuse the byte-identical helper
measured after the root-alias repair. All source/receipt/byte checks were actual.
Cost docs use the new manifest and explicitly record exact-binary reuse:
+350,792bytes/+0.505049%=0.51%; ordinary init311.3→312.4ms/noop50.3→49.7ms;
10,000-rule unchanged refresh adds26.3ms. No general speedup or #708 SLO waiver.

## Live/terminal processes and hosted feedback

No local build/index/push is live at this checkpoint. Next normal push has not
started.77584 is a terminal failed push, not restartable. Process/log receipt:
/tmp/projectatlas-739-document-epoch-push-process.json and -push.log.
Prior11389 was deliberately SIGTERM143 before upload for the newly confirmed P2,
not timeout; all9 captured processes are gone. Prior96803 TERMINAL0 pushed16338
with1778PASS/0FAIL/12ignored across34 suites. Preserve these distinctions.

Hosted source16338 run41362359349 is still live: Rust125015293207 and Linux
125015293306 whole PASS. Linux owner proof49FS/10policy/38observer/1external/
1replacement is downloaded. MacARM125015293213, MacIntel125015293232 and Windows
125015293177 were running at last authoritative poll. Fresh successor proof is
required for the document-only fix, native test and inventory correction.

CodeQL16338 run41362357386 SUCCESS all7 languages: Rust2073368431 has13 results,
Actions2073317753 has1; other5 zero. Codex code/security completed; code reported
this P2. Old7 inline threads resolved, new thread open. Eleven Rust complete
facts/flows equal the reviewed bad7 scan after exact unchanged-line relocation.
Alerts412/413 were separately reassessed:4 flows each, private cfg(test) CWD and
three fixed fixture writes at lib.rs2503-2509 → fixed TempDir child targets →
metadata exists644/is_dir674. No shipped untrusted boundary/content-read claim.
Actions full result/two flows match bad7→16338 with unchanged workflow bytes.
The successor adds one fixed CI command: inspect its fresh scan, not a blind copy.
Prepared14-input static dispositions:
/tmp/projectatlas-739-16338-complete-triage-prepared.json;
-16338-{rust-reconciliation,actions-reconciliation,test-flow-reassessment}.json.
Final new-head scan/feedback remains mandatory. No suppression/dismissal or
clean-scan claim. Public security comment6563449602 still describes bad7.

## Established shared scope and preserved work

Policy: gix-config0.62.2 defaults-off/sha1; ignore0.4.34; direct config bounds
1MiB/file4MiB aggregate, precedence, no implicit includes. No-Git hierarchy,
pruning, native lexical root spelling, operator paths and FIFO/regular-symlink
controls are covered. Queue1024/registrations256/convergence3; cold parent/root
coverage and ingress fence; no unconditional warm rewatch. Policy-only document
probe pages256, unchanged parse reuse, fresh publication policy/original options.
Shared #738 native-prefix guard covers cached/uncached/staged document consumers;
#738 still owns separate complete acceptance. CLI debounce root-gap and Mac/Win
fixture fixes are reviewed. Prior root/ingress/publication/document causal proof
paths remain under /tmp/projectatlas-739-*. No database reset.

Stable050 MCP index-4 completed after policy_drift. Later navigation returned the
correct bounded dependency_closure_limit10001 refresh guidance; no reset or
concurrent build/index was used. Follow typed refresh guidance if broad Atlas
navigation is needed again. Preserve this checkout's private approved purposes.

#731/PR737 and #741/PR743 are merged/closed; main6927ed0 includes them. #741 merge
tree49893e53b41eb98bd3e641a200dab26d9981e941 matches reviewed source; do not reopen.
#738 parked8814d6d plus dirty memory is preserved. #708/PR728 publicc65dfed/local
587ecbe plus dirty memory is preserved: warm1201.127ms FAIL, focused1.031526ms
partial, no SLO waiver. Its task2.3 live/main drift reconciles when #708 is accepted;
do not uncheck private progress. Integration release/0.5.1 retains three private
#715 design/proposal/spec edits. Queued Codex CI gap belongs to its later owner;
PR744 Dependabot is outside the RC milestone. Four open RC issues; RC1 unpublished.

Next: commit this reviewed inventory packet with sole(#739), bind all139 Git-tree
inputs, normal push with hooks, keep selected tree CLEAN while active and update
integration memory only. Inspect every current-head finding, earn task/acceptance
checks, normal merge745 and verify native739 CLOSED before starting738.
