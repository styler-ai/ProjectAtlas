# Active context

Continue until v0.5.1-rc1 is released and independently verified. Stable Latest must remain unchanged; stable promotion requires later user instruction. Primary writes all code; independent GPT-6 Luna max reviews use custom Rust and applicable database/SQLite skills. Use stable 0.5.0 official Atlas plugin/CLI with private worktree databases, incremental refresh only. Keep bounded calls and progress visible.

## Python #722

Checkout `.worktrees/issue-722`, branch `fix/python-imports-722`, base9845364. User requested analysis, English translation and RC1 inclusion if real. Confirmed installed0.5.0 reproduction: parenthesized token_contract import unresolved; adjacent redaction import resolves. Fixture root recorded in `/tmp/projectatlas-issue722-repro-root`; original issue backed up `/tmp/projectatlas-issue722-original.json`, actual stable CLI results `/tmp/projectatlas-issue722-stable-relations.json`.

Issue translated to English and normalized to repository shape, native parent/blocker #715 added. Milestone not yet assigned: plan review/checked contract gate first. OpenSpec `resolve-parenthesized-python-imports`, schema spec-driven,4 unchecked tasks. All apply context files written/read; strict validation passed. Luna plan review pending. No product source changes yet.

Python parse_import rejects all parentheses; resolution_keys incorrectly lists valid parenthesized form unsupported. Plan: one borrowed outerpair, flat comma/alias normalization, conservative identifiers/delimiter rejection, contract version3->4 through existing fingerprint. Real CLI file/caller/incremental/full/priorcontract and installed0.5.0 upgrade proof required. No schema/API/Pythonexecution. Global semantic version can force one-time non-Python reconciliation: coordinate #708 upgrade benchmark honestly rather than skip invalidation or compare to a cheap steady-state open.

Atlas alias issue-722 hydrated private state safely: parsed17/reused655 of672, identity8b1090f2bc522fad2a84be036348ff06. MCP controlalias main is integration checkout, not rootprimary. CLI next/slices used from this exact checkout. Do not init again.

## Other RC lanes

#723 merged main9845364. #724 relation implementation/source and final acceptance independently approved; headf54a712 is task-only acceptance, normalhook/pushpassed, currentheadhostedgates running. PRbody now Closes #721. Merge normally only after required checks pass, reuse unchanged source proof.

#708 budget checkout retains owned mixed edits/untracked work; do not lose them. User explicitly accepted shipping exact reconciliation and measured gains while deferring zero-work warm optimization. Respec #708/OpenSpec/docs, reset Open Questions to None after documenting decision, track deferred causal-native-history work in unscheduled backlog, then independently review scope. Latest detailed budget/harness evidence in issue708 memory; no RC exists yet.

Tests use filesystem TMPDIR=/home/utopien/.cache/projectatlas-test-tmp and shared CARGO_TARGET_DIR=/home/utopien/projects/projectAtlas/.worktrees/v0.5.1/target. Clean PATH rustup+ghonlyhelper+Pythonvenv; never compile sibling CLI during CLI E2E. gh authenticated. Mandatory native push hook and trusted hosted gates, no bypass. Commits use public Shaun Tyler/noreply identity per-command. Goal remains active.

## Verified implementation checkpoint

Luna approved narrow plan and current Python source after fixing hardkeyword rejection and adding softkeyword positive. Scope explicitly excludes comment-bearing lists (collapsed extractorline boundaries); regression safely abstains. Plan1.1, implementation2.1/2.2 checked; final3.1 and allacceptanceunchecked. LiveEnglish/milestonev0.5.1-00/soleparent715/blocker715 verified; scopedIssueOps4/4/1passed beforeimplementation.

Code borrows oneouterpair, residualdelimiter/comment rejection, existingflatcomma/alias normalization and conservative Pythonidentifier/hardkeyword guard. Semanticcontract3->4. Newunit failed beforefix; focused20tests pass, fullsymbols267pass/1existingignored. RealCLI E2E single/multiline/alias filetargets/callers +incremental/fullconvergence +ID/purpose/noopgeneration passes. Crosslanguage aliascaller E2E passes. Clippy initially found panic-in-Result in newtest; replacedassertmacros with localResultclosure, finalrerunpassed before lasthelperreuse (normalhookwillrecheck).

Realinstalledstable0.5.0->localdevelopmentfix upgrade9checksPASS (/tmp/projectatlas-issue722-installed-upgrade.json): unchangedsources repaired bothimports/callers, onepublication, stableidentity/purposes, secondnoopnopub andstable rollbackpreservation. Not exactRCacceptance. #715 must run thisagainstexactinstalledRC; add parenthesized_python_imports_resolve_and_converge_after_refresh to packagedrunner routing/installedselectors. NewE2E uses existing installed-executable override; assert_summary_called_by nowreusesexistingjson_summary_command for same override. Sourceinventoryupdated afteredits. Required nativepushhook/hostedgates stillpending.

#708 respec acceptedzero-workdeferral725/livebodyupdated; newhonestupgrade/currentopen harnesscases/unit3pass, finalLunascopecheckpending. #724 currentheadRustpassed, fourinstalledplatformjobsstillrunning.
