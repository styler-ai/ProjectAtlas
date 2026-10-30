# Active context

## Goal and serial authority

Finish and normally merge one issue, verify BOTH issue and PR CLOSED, then start
another. Only #731 / PR #737 is active. No issue has closed in this execution.
Continue until v0.5.1-rc1 is a non-draft prerelease, Latest=false, independently
verified. Stable v0.5.0 stays Latest. Stable promotion and milestone closure need
later user authority. Primary implements; existing `review_731` is the
independent read-only GPT-6 Luna Max reviewer. Preserve all parked work.

## Current source and tracker

Checkout `.worktrees/issue-731`, branch `fix/static-bindings-731`.
Public HEAD: `5053ceffa814bea221af7af4979f63e24143bce0` (contract22 delivery).
Local production source: `232186edd902a7dd7538fc80c8d934bef7bae41d` (source22; includes runner repair).
Current reviewed production advances DER22/SEM22 for Go clause scopes and a private
forwarding module interner. Immutable contract22 build succeeded in54.52s; binary SHA74bd98229c2e1a306840ead43e09804dec549ec11c7a7f1009840f2ccb02c504.
Confirmed Cargo root fan-out reopens local/live tasks2.6/2.7:6/9.
Prior areas1–4 approval remains historical22; newresource fix/proof required. Task3.1 and all five acceptance rows
are OPEN. OpenSpec owner: `normalize-supported-import-bindings`.

Published Go/cache feedback replied and resolved; readback21/21resolved.
Latest addressed thread IDs:
- Go P2: `PRRT_kwDOQyFgJ86yIDN8`, comment4427993824.
- Cache P3 performance lead: `PRRT_kwDOQyFgJ86yILNt`, comment4428046956.
The earlier Python comprehension thread was verified on public50fc, replied and
resolved. No other issue/PR may start before731/737 are normally merged and closed.

## Current22 implementation and proof

Go-only scope predicate now includes `expression_case`, `type_case`, `default_case`
through existing scope admission. Switch/select siblings no longer share local
bindings; initializer, receive and type-switch alias visibility stay compatible.
The alias remains an inherited conservative shadow; do not claim per-clause
variable target identity. Existing other-language switch rules stay unchanged.

Actual original CLI comparison: stable050 has one correct package-call occurrence;
contract21 has two unresolved. Checksum-verified Go1.27.3 actually prints `package`
for the sibling case. The15-case corpus also compiles (not all bodies executed).
Before unit failed11 cases; after all15 pass. New near-cap/overflow scope test also
passes and preserves raw navigation. Receipts:
`/tmp/projectatlas-731-go-clauses-before.json`, `-go-clause-oracle-before.json`,
`-go-clause-matrix-oracle.json`, `-go-clauses-{unit-before,unit-after,final}.log`.
Go executable path: `/tmp/projectatlas-731-go-oracle-path.txt`.

The cache and recursive visited maps formerly compared long Arc strings by value.
A private per-BindingResolver ModuleInterner now has a pointer/length memo retaining
original Arcs and a value-keyed std HashMap assigning local ModuleIds. Both production
query constructors use the same resolver factory. Equality/order use short fields
plus ID; text stays available for resolution. IDs never escape or persist. Exact
value equality handles separate allocations and hash collisions. No public API,
provider, input restriction, module copy, global interner or new dependency.
The interner header is charged; new memo/value nodes reserve existing staging row
estimates before insertion, with budget/cancellation checks. Tests cover equality,
unequal long-prefix values, cache/visited hits, retained allocation witnesses,
refusal/cancellation and existing alias workspace/cycle cleanup.

Actual parsed two-statement debug key probe (1,902,910 payload bytes plus `./`):
80k shared-Arc hits1.8166→0.00756s; equal separate source allocations5.2878→0.00749s;
short literals ~0.0066→0.0075s. This is a focused debug measurement, not whole-CLI
speedup, optimized release proof or security-boundary claim. Temporary probes
restored exact source bytes: `/tmp/projectatlas-731-cache-order-{before,after}-probe.{py,log}`.

Independent Luna APPROVED the22 Go/interner source slice. Latest terminal gates:
-61323 PASS workspace Clippy13.82s,451 symbols+1ignored47.29s,374 all-feature CLI
 units36.32s (including prior21 repair/cancellation),3 inventory, forwarding E2E.
- Go CLI/MCP nine-fixture selector PASS20.17s: exact occurrence positions/targets,
 summary callers, valid shadow removal/restore, full rebuild, identity, no-op.
-44066 PASS final targeted nav Clippy8.18s,3inventory2.64s, strengthened forwarding
 CLI/MCP JS/TS/TSX82.83s. Both equal-module export statements have direct probes
 (`ns.e0`, `ns.e256`), each checked unresolved with one untruncated occurrence.
Logs: `/tmp/projectatlas-731-clause-cache22-{clippy,symbols,cli-units,inventory,forward-boundary}.log`,
`-clause-cache22-nav-clippy.log`, `-go-clauses-boundary.log`.
No active local build/index/timing lease.

New accurate selector: `go_clause_bindings_agree_across_cli_mcp_and_refresh`.
Current inventory test_count219 is derived from its test list; historical218
selectors/facets and440 symbols remain frozen. Inventory validation passes.
Current navigation SHA:
`cb492cca455dedfb3f3a87cbea726eb7d2be17f3093a813bc6d5ddf29157cebc`.
Delivery SHA: `8b6cfc1b705f558effc8aac49aa66785de8db6b73b3edb66839a5c183d09751c`.

Captured semantic21 digest BEFORE production edits:
`e0bc8aa6d30d159c0e1eae3b80773fb0f4031398608b01e1bba31b6b8d6cbd04`.
`/tmp/projectatlas-731-semantic21-digest.{rs,txt}`. Do not recapture from22 rlibs.
Current prior-repair seed is21;20 and older controls remain. Final715 DER must
exceed22 and retain SEM22 after this fix is accepted.

## Publication and hosted gate continuity

Normal push75836 public50fc PASSED34 suites,373 CLI units,449 symbols+1ignored,
53 navigation218.18s, docs/security/scan/lint. Fourteen existing untracked asset
advisories are exit0, with zero source/purpose findings. That is historical21 proof.

Runner push83002 was deliberately terminated143 for the fresh Go defect, not an
observation timeout. Owned PGID890246 and children are gone; remote remains50fc.
Receipt `/tmp/projectatlas-731-runner-image-native-push-session.json`. Local ea86
runner commit is preserved. Do not restart that old push before22 is ready.

Source CI41120100367 at50fc had ARM job124284418474 canceled before allocation:
macos14 scheduled brownout, retirementNov2,2026. Official retirement notice and
runner-images13518 verified it. Local ea86 fixes all active ARM pins to macos15,
retains Intel15-intel, and provisions CI `macos-arm64` on15 even when the trusted
accepted-base planner still emits14. Contracts and trusted selection are preserved.
Causal old-planner and missing-adapter controls failed; exact runner/inventory/
Clippy/fmt/spec gates and independent review passed. Actual old-base→hosted ARM15
verification remains an OPEN3.1 gate. Preserve this fix in22 publication.

CodeQL41120094258 at50fc last had Rust pending and six other language jobs PASS.
Open IDs282/278/277/276/244/243/242/241/221/220/216 match prior reviewed leads.
Main graph_projection.rs cleanup owner is byte-identical f7→ea86; retain prior
counterevidence,244 is confirmed owner738. No alerts were dismissed. Require fresh
analysis/head and feedback on the final published candidate; no stale waiver.

## Current21 baseline and next work

Immutable21 binary:
`~/.cache/projectatlas-benchmarks/binding-731-comprehension-contract21-20261030/projectatlas`
SHA `4bca2b1b2ea623d946183c5a511e563873a8d2bd592b17572048c2841da62339`, source3e81518.
Its129 bound source/build/test files include20 tests/fixtures and109 non-test inputs;
do not call all129 production files. Current22 production differs, so21 is a baseline.
Prior campaign9970 PASS11 tasks:17 unique native stable050/contracts5..20 upgrades,
84 initialization samples/42pairs/14fixtures. Supplement23846 compared16 semantic
stored tables including ALL purposes/summaries through incremental/full for every
baseline, preserving identity and final no-op. Receipts:
`/tmp/projectatlas-731-comprehension21-{proof-session,proof-manifest,convergence-proof}.json`.
Current benchmark docs still describe21 and must become a truthful22 handoff.
Earlier immutable artifacts and source20/19 evidence remain in their cache and
`/tmp/projectatlas-731-qualified20-*` / `admission19-*` receipts; reuse verified controls.

Current22 campaign2469 TERMINALPASS all15tasks. Four optimized CLI/MCP E2Es,
18 unique native stable050/contracts5..21 upgrades, resource21→22 compatibility,
all16 normalized semantic tables incremental/full equal for every baseline,
identity/all-approved purposes, deadline/rollback/reupgrade and no-op PASS.
96 initialization samples/48pairs/16fixtures, immutable source/binary bound before
and after. Two-source forwarding21→22 median1.99451→1.87129s (6.18%local decrease),
maxRSS128424→129448KiB; ordinaryPython.13420→.13471s, no general speedup/SLOwaiver.
Current docs/benchmarks/static-binding-cost.{md,json} updated; independent whole
areas1–4 delta/readback APPROVED. Failed first upgrade oracle used unique target
sets and masked the Go sibling difference; corrected all exact occurrence spans,
status and targets. Second attempt had an offset1 column mismatch; independently
caught/corrected before approved retry. Both failed receipts retained as*.attempt1/2.
Source/permanent tests unchanged; four passed E2Es safely reused with exact arg,
name, exit and input guards. Receipts /tmp/projectatlas-731-clause-cache22-
{proof-session,proof-manifest,upgrade-proof,stage-upgrade-proof,convergence-proof}.json.
Native6414 TERMINALPASS normal clean push5053cef;34 suites,374CLIunits,451symbols
+1ignored,54navigation220.74s, docs/security/scan/lint PASS. Fourteen existing asset
advisories remain exit0 with zero source/purpose findings. Remote head verified.
Receipt /tmp/projectatlas-731-clause-cache22-native-push-session.json. No activelease.
Current hosted source41138283363 at5053cef: plan/repositoryPASS; ARMjob124346111967
FAILED preflight beforeinstall/build in0.08s; otherjobscontinue, do notcancel/retry.
ActualARMadapter VERIFIED: labels[macos-15], image macos-15-arm64 20261028.0489.1.
CodeQL41138279073 live; pr-state41138280569/41138430109PASS; metadata41138429431waits.
All21threads replied/resolved afterpublication; PRbody soleFixes731 updated/readback.

New3.1 owning gate root cause: .github/scripts/verify-rust-toolchain.py assumes
CARGO_HOME/bin for all selected proxies. ARM selects /opt/homebrew/opt/rustup/bin.
Read actual job/log /tmp/projectatlas-731-clause-cache22-arm-{job.json,job.log}.
Earlier log errors are intentional selftests; actualfailure startsline465.
Offline native-co-location test failsbefore/passesafter with exactbacking checks
retained. Draft genericalternate selection+~90lines tests UNCOMMITTED in shared
helper, plusaccepted design/spec scenario. Reviewer approved native-directory
plan, NOT finalfix yet. All129 measured source/build/test inputs unchanged.
DER22/SEM22 andcost/upgradeproof valid. Never weaken samefile/size/SHA backing,
override/version/commit or manager-before-proxy guards.
Homebrew wrapper fix nowIMPLEMENTED UNCOMMITTED insharedhelper, not yetpublished.
Pureproxy_provenance extracted(noexec) retainscanonical/nativeco-location/exact
backingchecks. homebrew_native_environment Darwin--install invalidoriginalbundle
queriesbrew--prefixrustup, requiresall5selectedpaths samefile exactformula/bin
wrappers, validatesnative libexec/bin backingbeforeanyRustup/proxycall, preserves
existingfallbackenv orbrewglobal/etc/rustup/settings.toml. verify_toolchain restores
processenvfinally; persist_ci defaultsFalse; mainreal--install+GHAtrueoptin publishes
nativeGITHUB_PATH/fallbackGITHUB_ENV ONLYafterallpreflightPASS. Rawpreflightselftests
cannotpersist; package tests useisolatedcommandfiles, distinctneverexecutedwrappers,
actualhardlinkednativefiles, failure/mixed/missing/digest/prefix/override controls.
Independent sourceapproval received; suggestedcustomfallbacktestadded/selftestPASS,
readbackqueued. Existingcommand-fileisolationreceipt olderhelperSHA mustrefreshafter
test-onlychange. RuntimehelperSHAatoldreceiptfed845...; latesthashnotcapturedyet.
Gates89300PASS selftest+realLinuxreadonly+strings/spec/IssueOps8. Initialdelivery
selectorwrongtargetmatched0 (notproof). Correctactualallfeaturesdeliveryselector
issueops_and_workflows_use_behavior_focused_quality_gates PASS1/.27s99578.
Officialformula andexactimage supportnative vswrappers distinction. Design/spec and
docs/workflow updated totruthfulbootstrap; noRustproduction/fingerprintchangesyet.
Currentall129 measuredsource/build/test bytes still232186e;22costproof valid.
NoactualhostedcorrectedARM proofyet; willpublishafternewCargo leadfixed too.

Fresh automated22nd thread OPEN PRRT_kwDOQyFgJ86yKe9j/comment4429046208 at
runtime/graph_projection/binding_resolution/rust.rs: automaticCargo flat src/bin
roots allegedly allmatch everysibling, per-source cause(root) retention N² (2kroots
512MiB,4k2GiB). Lead now hasactual128target CLI proof:34176 persistedrelationdependencyrows,267perowner,51MiB/1.41s; exactsourceallrootscause retentionmechanism confirmed. NoRustfixyet.512case now529920rows,463260KiB,21.65s vs128case34176rows,51716KiB,1.41s. Need traceowningroot
membership/causecopy/nativefixture to distinguish performancevswrongtarget facts,
thenboundedspec/fix/regression (possiblyDER23SEM23onlyiffactschange). Keepthreadopen,
other21resolved. Finishcurrentpreflightdesign/fixthen thislead; bothsameissue731.
35287 TERMINALPASS optimized22 CLI512target scalingprobe; noactivelease. Receipt /tmp/projectatlas-731-flat-targets-large-before.json.128case /tmp/projectatlas-731-flat-targets-before.json.
Potentialsharedfixunderreadonlyreview: existingcargo_target_inventory_key isalready
emittedEVERYflatRustfile src/bin/examples/tests/benches; incrementalchangedkeys
includeold/newsourcekeys onedit/deletion. Replaceper-flat-filecause copies with
existingsharedinventorycause, preservingnongrouped/conventional/nested causes.
Broaderexplicitcustomflatcase CONFIRMED too: auto*false, [[bin]]app/tN.rs;
16targets624deps39/owner;32targets2272deps71/owner. /tmp/projectatlas-731-explicit-flat-targets-before.json.
DoNOTshipautodir-onlygrouping and leavecustomsame-mechanismfailure. Reviewerasked
minimalsharedroot-cohort/keyproducer design viaadmittedCargoTargetRegistry; noRust
fixyet. PackageIndex holdsroot/name/manifestonly; publicResolutionProjectionContext
hasrust_package(manifest,root)tuple, nocargotargets. Needrootonlysourcegroupkey or
incrementaldirty-rootgroupescalation withcurrentcapturedmanifestfacts, withoutall-
Rust-file/whole-crate overinvalidation. Potentialfirstmodule→candidateRootindex for
remainingCPU. ExactdesignNOTDECIDED; wait/readreviewerrecommendation, primaryimplements.
BootstrapcustomfallbacktestreadbackAPPROVED; actualcorrecteddelivery1PASS, strings/
spec/IssueOps6PASS. Localbootstrapcommitnext, no pushuntilCargo fixes/proofready.
Needverifykeyproducercoverage andavoidduplicateplacementlogic (reuseowningsymbols
helper). RemainingrootloopCPU mustbemeasuredafterfix, notsilentlyleft. Factsseem
unchanged; likelyDER23forprojectionrepair/SEM22unchanged, notdecidedfinalyet.
Need finishbootstrapreadback, confirmedresourceplan/spec/taskreopen2.6/2.7, causal
Rustfix/coverage/currentartifactupgrade/costproof/review,
applicablecheap/nativepushgates, actualhosted4OS/CodeQL/freshfeedback, finalarea5,
task3.1/allfiveacceptance and normal737merge. Verify BOTH731/737CLOSED before741.
Goal ACTIVE; noissueclosed/noRCpublished. Preserve allparked owners.

## Operating discipline and parked owners

Read complete installed stable050 Atlas skill and short guide after compaction.
Select exact731 root; preserve its DB. Known brief refusal:policy_drift/full/changed0,
tracked739. No orientation scan/reset; use verified owning slices. Required native
pre-push scan is authorized. Installer-readiness warning was previously misleading;
installed050 works. Do not mutate installer merely for that warning.
Source `/tmp/projectatlas-rc1-test-env.sh` before Cargo. Shared target is v0.5.1/target;
use its cache TMPDIR (system /tmp is tmpfs and not supported for DB fixtures). The
clean PATH lacks rg; search before sourcing or use an absolute rg. Never globally
set PROJECTATLAS_NO_TELEMETRY for native suites: unset it; two telemetry units need
normal environment. Disable only isolated read-only/probe CLI/MCP telemetry.
Preserve latest owned memory outside checkout before a clean push; restore only
HEAD memory if dirty, then restore latest AFTER terminal. Never bypass hooks,
force push/merge, reset private DBs or run sibling builds/indexes during leases.

Parked and untouched:741 e1e633c+owned atlas_map/config dirt (missing-parent fix not
retested);7396eb0dfd+owned watch/map-budget dirt;7388814d6d must compose739;708PR728
publicc65/local587ecbe needs actual closure path and combined four-platform performance
proof. Preserve120-sample warm1.127FAIL and focused1.031526 partial;725 is deferred.
715 integration is not an RC; preserve its owned proposal/design/spec edits and
consume accepted union only after children close. Root715 exact children/blockers:
708/718/721/722/731/732/738/739/741, milestone37. Closed718/721/722/732. Future41/root493
exact310/314/388/490/563 was accepted via740/main3ef603aad39c435cf130545a555f8d746ec745ba.
No stable promotion or milestone closure is authorized. Prior Codex feedback upload
is DONE in ~/.local/state/projectatlas/feedback/20261029-defensive-hardening-*; do not duplicate.
