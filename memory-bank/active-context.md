# Active context

## Sole active issue: #741

#731 and PR #737 BOTH CLOSED by normal merge a5b1bebcaa79c8386b68e9c247eff73aa11e5e15
on 2026-10-31. Finish #741, review, normal merge and BOTH CLOSED before next owner.
Draft PR743 open; public52ae571. Tasks 3/5 (1.1, 2.1, 2.3); all acceptance open. Release goal remains independently
verified v0.5.1-rc1 prerelease Latest=false; stable 0.5.0 stays Latest. No stable
promotion/milestone closure. Integration memory owns other parked release work.

Exact checkout issue-741 / fix/generated-state-741, HEAD 597a00e779a505f26241210d516c7b0e608bc138.
Normal origin/main integration includes accepted #731 DER24/SEM23, no source conflicts.
Inventory union221 tests; other owner mappings retained without parked production code.
Owned WIP restored from retained stash7cd644e2e5b8d8561385134e8290440d0ab6b4a1;
backup /tmp/projectatlas-741-resume-accepted731-backup preserves full source/memory/patch.

## Current confirmed mechanism and work

Automatic database admission and exact4-document atomic publication already implemented
in320652f/e1e633c. New findings: failed parent canonicalization must not mean external;
managed paths with missing/.. bypass JSON guard and truncate outside hardlinks. Legacy
explicit config init dangling aliases created outside before guard. Actual immutable
CLI/MCP debug b6aa4229f5bcae98f7b8941f61380b5e2cd823bc6011f8d696930e68de3a87e5:
/tmp/projectatlas-741-document-debug-path.txt; classifier-repro.json and
init-map-mcp-repro.json preserve causal evidence. Explicit config/DB parent variants
already refused; MCP dangling implicit legacy is deliberately filtered (counterevidence).

Merged source+legacy helper guard: exact legacy unit PASS, exact map reduction FAIL
(/tmp/projectatlas-741-resume-{legacy-after,map-before}.log; terminal3024).
Shared role10cases and Generated6cases FAIL before classifier fix (terminal6325),
/tmp/projectatlas-741-{shared-classifier,generated-capture}-before.log. Classifier patch now implemented. Luna confirmed Generated must also capture managed
legacy config before runtime identity repair can commit; causal existing-DB repair
unit FAIL101 before fix then PASS, and now includes supported retry/identity proof.

Implemented fallible managed classification: true owned / false proven external / Err
unresolved potentially managed. Preserve direct namespace proof before following .state,
physical existing symlink/.. behavior, external custom paths, Windows named() case rules,
root aliases, no-change/read aliases. Resolve deepest existing prefix before reducing
only unresolved suffix. All DB/Document/Generated callers propagate errors. Generated
captures private AND legacy config; init helper retains publication recheck. No schema,
DER/SEM bump or general path framework. Runtime test proves admission before identity repair.

## Verification and next steps

Read-only Luna Max reviewer review_741 found no remaining source issue after alias+
parent reduction supplement: original logical-ancestor proof distinguishes managed
namespace from explicit physical outside path. Paired missing/existing ×DB/config/JSON
unit FAIL101 then final16stateunitsPASS. WorkspacecheckPASS24.04s,ClippyPASS31.32s.
No additional unguarded SQLite opens found. Real CLI/MCP extended liveWAL/admission
PASS2.87s and writer missing/existing parent reduction PASS2.79s. Registered holistic
--ignored PASS1test8.39s (first ignored0 discarded). Final-source reruns pending.
Pipeline78384 terminal PASS: strings PASS after removing reviewer absolute path from memory;
403CLIunits PASS37.48s; lifecycle39PASS/2existingignored46.29s; registered explicit
--ignored PASS8.19s; inventory3 proof see log. No overlapping builds/index/timing.
Final compatibleupgrade/native/hosted/
independentacceptance remain. Design/spec/docs+inventory and715installedselectorhandoffcomplete;2.1/2.3checked.
Do not mark implementation complete from previous narrow writer E2E alone.
Permanent CLI/MCP selectors automatic_state_admission_preserves_cli_mcp_navigation and
managed_document_writers_preserve_cli_mcp_capabilities previously PASS; rerun affected
merged source. Reuse accepted #731 evidence rather than re-auditing closed history.

Atlas exact741 brief refresh_required80 paths, earlier closure10001 known708; preserve
private DB, no reset/scan/installer repair for orientation. Bounded direct owner reads.
Shared target free; source /tmp/projectatlas-rc1-test-env.sh before Cargo. No concurrent
build/index/timing. UNSET PROJECTATLAS_NO_TELEMETRY for native suites/push; set1 for
isolated read-only Atlas/probes. All exec login:false. Normal clean push hooks only.
Prior complete checkpoint archived in backup above; older evidence receipts retained.

Immutable final native debug SHA3b97df92f32e2b6f66814b049b29db3d9ac02ccea7e272a20463e302fccf197e,
/tmp/projectatlas-741-final-debug-path.txt; adjacent source-binding.json131checksums.
Stable050→corrected4layout upgrade/refusal/retry proof75899 terminalPASS: automatic,
legacy,external,root-alias; identity+ALL approved purposes+configpreserved through
refusal, no-scan retry, scan/init,overview,watchonce,rootverify. Receipt/log
/tmp/projectatlas-741-native-upgrade-proof.*. Initial95257 comparison incorrectly
included newly derived suggested legacy-config purpose; proof corrected to compare
all approved purposes, no product regression inferred. All4 finalfixturesPASS.
Wholeissue Luna areas1–4 review active, area5 after hosted. 2.2 and3.1open.

Normal nativepush64217 TERMINALPASS937.02s on52ae571:34suites/1713tests,zero failures;
check/Clippy/docs/doctests/dependency/repository/actualscan+lint allPASS. Exactreceipt
/tmp/projectatlas-741-native-push.json/log. Public draftPR743 opened/readback52ae571.
Only741/743 active. Task2.2/3.1/allacceptance remainopen. NoRCpublished.

Review found macOSIntel lacks runtimeownershipproof in existingCI; source reviewed
complete but2.2needs explicit all4platform execution. Current owned CI-only delta adds
16stateunits,legacyrepairunit,twoCLI/MCPselectors,roottransition onallcompile/CLIrows;
Intelonlyregisteredholistic supplement reuses existing three-platformworktreegate.
Placed after PHP coldcompile so15min proof budget owns only affected warmed work.
Luna reviewed the bounded plan; no new product source/schema/DER/SEM. Full source
native/immutable3b97+4nativeupgrades remain valid; normalhookrequiredagainonfinalCIhead.
NextreviewCIactualdelta/cheapworkflowgates,commit,normalpush,hostedall4feedback,
then2.2/3.1and orderedacceptance. No source mutation during active nativepush.
