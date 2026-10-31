# Active context

## Latest Cargo source checkpoint (supersedes index draft below)

16222 TERMINALPASS lazy-directory implementation: Clippy allCLIalltargets/features
6.72s, exact realcapture unit1/.11s, indexbudget/reuse unit1/.09s, expanded existing
CLI/MCP lifecycle1/62.14s (app-only edit≤45parsed; disabledhelper≤2).378CLIunits now.
Root path/cause buckets are SOURCE-FREE. Module maps are lazily recovered only for
queried ancestor directories and retained per manifest/directory. Independent
Luna accepted this repair; earlier eagerindex was rejected for unrelated recovery.
Causal68166 expectedFAIL actual eager optimized artifact:168parsed atapp-onlyedit.
Original22 causal68640 expectedFAIL12867dependencyrows atstep0. Keep both receipts.
Eager exploratory41426 PASS5cases: auto512 .614s,2048 2.432s; explicit512 .564s,
2048 2.217s. That artifact is historical/exploratory; final lazy artifact pending.
Current source reviewed, all targeted gates passed. Strictstrings initially caught
15 repeated fixture paths; centralized function-local constants, no gate waiver.
71945 TERMINALPASS strings/strictOpenSpec/scopedIssueOps6/9; fmtPASS.
Bootstrap final selftest commandfile isolation PASS, helperSHA18ebfa524a1455b2a9934a73f48b9ef2dbca2b41153b9629a7b41e926c95d820,
/tmp/projectatlas-731-managed-toolchain-command-files-final.json.
No active native build/index/timing lease. Preparing sourcecommit and default-
feature immutable final23 build. New outside proof drivers root-cohorts23:19native
baselines incl22, equal22targets+lowerdependencies, all16normalized projections,
36root timing samples6fixtures. Reviewer requested two integrity improvements:
convergence now binds upstream candidate hashes; reusable resource fixture now
checks authored file inventory/hashes, original+selected baseline hashes, and
exact resolved caller.js::caller→target.js::work, not mere row count. Readback
pending. No acceptance/task completion or new push yet.

## Serial release authority

Only #731 / PR #737 is active until normal merge and BOTH CLOSED readback; then
#741. No issue has closed in this execution. Active unbounded goal: continue
until v0.5.1-rc1 is published non-draft prerelease, Latest=false, independently
verified. Stable050 stays Latest. Stable promotion/milestone closure need later
authority. Primary implements; existing review_731 is read-only Luna Max.

## Source and task state

Exact checkout .worktrees/issue-731, branch fix/static-bindings-731.
Public5053ceffa814bea221af7af4979f63e24143bce0, production22=232186e,
immutable22 SHA74bd98229c2e1a306840ead43e09804dec549ec11c7a7f1009840f2ccb02c504
at ~/.cache/projectatlas-benchmarks/binding-731-clause-cache-contract22-20261030/projectatlas.
Local ab14270 commits reviewed native Homebrew bootstrap (#731), not pushed.
Current UNCOMMITTED DER23/SEM22 Cargo cohort + candidate-index repair and tests.
Local/live2.6/2.7 reopened:6/9. 3.1/allfiveacceptance OPEN. OpenSpec owner
normalize-supported-import-bindings. Sole PR closing reference Fixes #731.
22 review threads:21resolved, Cargo N² open PRRT_kwDOQyFgJ86yKe9j/comment4429046208.
Resolve only after published verified repair. Do not begin another issue.

## Current Cargo fix

Actual22 CLI auto src/bin roots:128→34176dependencyrows/1.413s/51716KiB;
512→529920rows/21.657s/463260KiB. Explicit custom app roots auto*false also
quadratic:16→624rows,32→2272. Receipts /tmp/projectatlas-731-flat-targets-
{before,large-before}.json and -explicit-flat-targets-before.json.
Root cause: every source retains every compatible sibling file cause, then
membership rescans every root. Fix both retained cause fan-out and traversal.

New private runtime/graph_projection/cargo_roots.rs owns per-manifest captured
CargoRootCohortIndex and repository_join (moved exactly). PackageIndex::from_sources
hydrates only Rust-participating package metadata from captured/hashchecked facts.
SourceBindingCache retains/coarsely charges cohorts before growth; failures leave
no partial cache. Exact fallback for unclassified/foreign/conventional/singleton
roots. Multiple admitted same-parent actual roots share cohort cause; actual
module paths retain exact file causes. No AST-fact or schema change intended.
Symbols cargo_automatic_target_directory reuses existing placement policy; new
ResolutionProjectionContext.rust_root_cohort enables only exact-listed-root keys.
Automatic discovery retains existing Imports inventory keys. Explicit listed
roots use existing Extended(Configures) discriminator via cargo_root_cohort_key.
This distinction is REQUIRED: with autobins=false, an unrelated unlisted helper
must not emit a consumer's explicit cohort key. First draft failed causal unit;
current Configures fix passes unit and CLI/MCP lifecycle. Luna accepted design.
Old/new stored entity-export keys union drives edit/delete/rename invalidation.

Current RustResolver root_memberships cache indexes root physical directories and
top-level module-name→root ordinals once. Membership still calls roots() for each
source to retain automatic inventory/missing nested-main causes. Visits only
ancestor directories, copies shared negatives once, binary-searches direct root,
follows matching declarations, deduplicates direct ordinal, preserves certainty/
ambiguity/module-file causes. Scratch admission before nodes/causes/ordinal growth;
index publishes only complete state. This new index is NOT independently reviewed
yet; no final artifact/proof yet. Check current logs before more edits.

Prior retention-only exploratory23 build40617 PASS56.03s:
~/.cache/projectatlas-benchmarks/binding-731-root-cohorts-exploratory23/projectatlas
SHA9fe89a334138ecbfaebcb057a84b06425a5f39b39ac51cc3e48b611e9f83d5f9.
Five-case22223 PASS, receipt /tmp/projectatlas-731-root-cohorts-exploratory-cost.json:
auto128 .313s28088KiB1664deps;512 .753s40768KiB6656;2048 5.069s98216KiB26624;
explicit512 .692s39924KiB5632;2048 4.843s92312KiB22528. Exact first/last targets
and no-op pass. 4×input gave6.7–7×time: confirmed remaining CPU cost, motivating
current directory/module index. Exploratory uncommitted proof is NOT finalSLO.

Current gates before index: cargo checkPASS, Clippy36037PASS (before latesttests),
private cargo_roots unit7807PASS2/.13s; expanded CLI/MCP existing selector
rust_root_aliases_follow_manifest_and_module_changes19593PASS1/62.51s.
Lifecycle covers all4auto dirs+custom app+package-rootdot+single/nested controls,
11 edit/delete/add/rename/manifest/helper phases, ambiguity, all16normalized
incremental/full snapshots, identity/all-approved purposes/no-op, disabled-auto
helper no-key-intersection and parsed≤2. New index current check52537PASS6.04s.
Historical49235: eager Clippy failure (test helper formatting), --lib matched0
NOT proof, lifecyclePASS63.26s. Corrected83080 actual --bin units/ClippyPASS. Logs
/tmp/projectatlas-731-root-membership-{clippy,unit,lifecycle}.log. No active49235 lease. Latest16222 supersedes it.

Captured old22 SEM digest77ce9ef63aa9da217f35e41d59470395e7be08d45f167c14b2477eb29309d2ca
BEFORE edits: /tmp/projectatlas-731-semantic22-digest.{rs,txt,json}.
DER23 prior_projection is22 withthatdigest; prior21/e0bc8… andolder preserved.
Compare new semantic digest after build; SEM22 only if unchanged. Final715 DER>23.

## Latest accepted22 baseline evidence

Independent areas1–4 APPROVED22 source/driver/proof/docs; current23 needs delta
review. Go clause scopes and private forwarding module interner source at232186e.
451symbols+1ignored,374CLIunits,219installedtestinventory,54navigation selectors.
22 campaign2469 terminalPASS15tasks:4optimizedCLI/MCP,18unique native baselines
stable050/contracts5..21 upgrades, resource21→22 compatibility, all16normalized
semantic tables incremental/full, identity/approved/rollback/deadline/noop.
96init samples48pairs16fixtures. Forward21→22 median1.99451→1.87129s (local6.18%);
ordinaryPython .13420→.13471, no general speedup/SLO waiver. Docs benchmark22 now
baseline after23 edits. Driver receipts /tmp/projectatlas-731-clause-cache22-
{proof-session,proof-manifest,upgrade-proof,stage-upgrade-proof,convergence-proof}.json.
Two failed upgrade-oracle attempts retained*.attempt1/2; corrected exact occurrence
spans/status/targets, not targetsets; four passed unchanged E2Es safely reused.

Normal CLEAN nativepush6414 terminalPASS34suites:374CLIunits451symbols+1ignored,
54navigation220.74s, docs/security/fullscan/lint.14existing asset advisoriesexit0,
zero source/purpose findings. Public5053 verified. Receipt
/tmp/projectatlas-731-clause-cache22-native-push-session.json. Native22 historical.

## Hosted and Homebrew bootstrap

5053 source41138283363 plan/repositoryPASS, ARM124346111967 failed preflight0.08s
before install/build. Actual accepted-base macos14→macos15 adapter allocated
macos-15-arm64 image20261028.0489.1. Other jobs continue; don't cancel/retry merely
for observation timeout. Logs /tmp/projectatlas-731-clause-cache22-arm-{job.json,job.log}.
Homebrew publicbin aliases are wrappers; native hardlinks are libexec/bin.
Shared verify-rust-toolchain.py fix ab14270 independentlyAPPROVED, customfallback
caseincluded. Pureprovenance retains exactsamefile/size/SHA before execution.
Darwin--install fallback bounded brew-prefix, verifies all5 selected wrappers
belong same formula, selects/verifies native bundle, preserves existing fallback
or formula globalsettings. Restoresprocessenvfinally; only successful real GHA
install publishes GITHUB_PATH/ENV. Selftests have isolatedcommandfiles/noeffects.
Offline native/wrapper/foreign/mixed/missing/digest/prefix/override/negative
controls+selftest+realLinuxreadonlyPASS, actual delivery selector99578PASS1/.27s.
Old commandfile isolation receipt SHA stale after test-only customfallbackcase;
refresh. Corrected actual hosted ARM gate remains REQUIRED after publication.

CodeQL41138279073 at5053 lastpending; metadata41138429431 waits source;
pr-state41138280569/41138430109PASS. Fresh finalhead reconciliation required.
11 existing leads282/278/277/276/244/243/242/241/221/220/216:10counterevidence,
244confirmed owner738. No alerts dismissed. Preserve owning-range evidence.
Branch protection strictadmin, verify/pr-state/conversationresolution; no bypass.

## Next steps

Finish index gates, causal old22 rowbound failure, optimized512/2048 scaling,
source/spec Luna review. Commit reviewed source23 then immutable final23 artifact.
Rebind measured inputs dynamically (newsourcefile), compareSEM22digest.
Clone verified22 proof drivers, add native22→23 =>19unique historical baselines,
all16normalized tables, root lifecycle/new cost fixtures, identity/purposes/noop/
deadline/rollback. Reuse immutable historic artifacts. Finalcostdocs/JSON+review,
check/mirror2.6/2.7 immediately once done, strictspec/strings/IssueOps/fmt.
CLEAN normal nativepush hooks enabled, actual hosted4OS incl corrected ARM,
freshCodeQL/reviews, independentarea5,3.1/allacceptance, normal737merge expectedhead.
Verify BOTH731/737CLOSED before741. Continue serial throughRC1; no stablepromotion.

## Operating discipline and parked work

Read complete installed050Atlas skill+short guide after compaction; exact731root,
preserve its DB. Known Atlas brief policy_drift/full/changed0 reproduced again,
tracked739. Noorientation reset/scan; reuse verified owning slices. Hook installer
warning previously misleading; do not mutate install for warning alone.
Source /tmp/projectatlas-rc1-test-env.sh before Cargo (shared v0.5.1 target,
homecacheTMPDIR); cleanPATH lacksrg. UNSET PROJECTATLAS_NO_TELEMETRY native suites;
only isolated probes/read-only use1. Never edit731 while nativepush/timing lease.
Cleanpush memory: save outside, restore HEAD ifdirty, restorelatest onlyterminal.
All commit subjects exactlyone (#731), PR sole Fixes #731. No force/hookbypass.

Parked untouched:741e1e633c+owned atlas_map/config dirt missing-parentfixunretested;
7396eb0dfd+ownedwatch/mapbudgetdirt;7388814d6d compose739;708PR728publicc65/local587ecbe
closureCLI/MCP+combined4OSperf (120warm1.127FAIL/focused1.031526partial preserved,
725deferred);715integration ownedproposal/design/specdirt. RC1 blockers715 children
708/718/721/722/731/732/738/739/741 milestone37;closed718/721/722/732.
Future41/root493 accepted740/main3ef exact310/314/388/490/563. Prior Codexfeedback
upload DONE ~/.local/state/projectatlas/feedback/20261029-defensive-hardening-*.
Earlier detailed22memory archived /tmp/projectatlas-731-memory-before-root-index.md.
