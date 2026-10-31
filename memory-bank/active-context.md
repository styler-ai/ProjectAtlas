# Active context

## Serial RC1 authority

Only #731 / PR #737 is active. Finish normal merge and verify BOTH CLOSED before
#741. No issue closed in this execution; no RC published. Active goal remains through
v0.5.1-rc1 non-draft prerelease, Latest=false, independently verified; stable v0.5.0
stays Latest. Stable promotion/milestone closure require later user instruction.
Exact checkout `.worktrees/issue-731`, branch `fix/static-bindings-731`.
Local/public HEAD c802a5d29efc1dbf8b874ddb6426c20bed54c1a0, unmerged.
Local/live implementation 5/9: 2.4/2.6/2.7 reopened, 3.1 + all acceptance OPEN.
Scoped IssueOps + strict OpenSpec passed at that state; no current24 acceptance yet.

## New24 implementation in progress

Current DER24/SEM23 source/spec independently APPROVED for freeze (areas1–3). No current24 optimized runtime/proof yet. Source23
receipts below remain BASELINE, not readiness. Primary writes all; review_731 read-only
Luna Max is reused. Use login:false on exec_command and verify pwd/root: reviewer
admitted earlier login shell reads reset to integration despite workdir. Wrong-root
observations are discarded; exact731 current source review is underway. Do not assume
old independent23 approval establishes current24 acceptance.

Kotlin fix: lexical_bindings::kotlin_statement_binding_start uses whole local property
end (val/var/destructuring/delegation) and actual for-container expression end. File/
classifier reservation remains zero. 13-case source matrix had 11 causal failures
before, all13 PASS after; existing native local-values matrix + symbols Clippy PASS.
Logs /tmp/projectatlas-731-kotlin-boundary-{unit-before,unit-after,compat,clippy}.log.
Actual23/stable050 before probes: /tmp/projectatlas-731-kotlin-boundary-variants-before.json.
Exact native AST /tmp/projectatlas-731-kotlin-ast.{rs,txt}, Kotlin-ng1.1.0. Official scope/
for specs checked; no compiler oracle claimed. Still needs permanent CLI/MCP case,
refresh/full/noop, current20-native-baseline upgrades and independent final review.
Old23SEM22 digest preserved before edits:77ce9ef63aa9da217f35e41d59470395e7be08d45f167c14b2477eb29309d2ca.
Runtime prior_projection fixture now23; separate22 and older seeds retained.

Configured aggregate resource root cause causally reproduced:5files x16side-effect
imports x64~1KB valid target paths retained5,603,545bytes under injected2MiB. Before
log /tmp/projectatlas-731-configured-aggregate-unit-before2.log expectedFAIL. Current
causal unit PASS0.11s, including admitted64-target prefix, split-admission reuse,
nonzero identity base and cancellation. No2.5GB/OOM execution needed.

Current implementation (not final proof):
- Core CanonicalResolutionKey::retained_heap_bytes actual String capacity, no layout/
  serde change. Symbols Context stays Copy, adds with_retained_bytes_limit; typed
  MemoryLimit maps to existing CLI OutputBytes refusal. Private ProjectionBudget
  checks keys/vector envelopes/source-call/span/toolchain/import/cache/index owners.
- ResolutionKeyProjection::retained_bytes counts vector capacities/canonical text/
  sidecars. ConfiguredModuleResolution::retained_bytes counts config/mapping/target
  capacities. Existing64targets/module length/provider/schema/2GiB cap unchanged.
- Scalar operation remainder enters all3 runtime graphstage calls from existing
  retained_before_symbols. Original source identity is now part of SymbolBuildStage
  retained_bytes; source admission receives remaining symbol allowance. Existing
  identity/spill512MiB sublimit preserved.
- Full/incremental direct+affected phases subtract coexisting symbols/cache/maps/
  identity/docs/config. Per-file projection generation is bounded before admission;
  valid partial Contract projections + typed rejection state share the same allowance.
- GraphIdentityAdmission seeds operation cap for raw admission/hydration/merges;
  for_paths clone pre-admitted; relation_fact_indices sidecar reserved/charged at
  creation, clone and merge. Merge admits self+2*incoming envelope before shared-path
  clones while incoming maps remain live.
- Hydration reserve_identity_hydration_scratch admits all-chunk path/rejection/
  coverage/drop/fact-count/parser maps before retention and lowers report allowance
  by scratch. Outer closure restores original Option allowance on ALL exits, including
  legacy incomplete-publication successful early returns. Single DB batch retains its
  existing10k-row / coverage32MiB decoded envelope, not an unbounded aggregate.
- SourceBindingCache captures/Cargo cohorts use phase remainder. PackageIndex owned
  vector/strings are reserved before clones; construction drops temporary needed set
  and releases only its own package reservations on error, preserving completed cache.
- Entity builder moves map projections before ownership growth; node/project rows
  checked per insertion; export key/owner clones admitted first and final estimate
  includes them. Fallback producer bounded. Parent qualification has checked reserve
  callback covering vectors and scope rows. Coverage marker remains nonentity but its
  actual ordinal metadata is counted.
- Candidate registries receive current remainder and reserve before distinct entity/
  key/digest/forwarding insertion; duplicate witnesses do not charge. Incremental and
  recovery append directly into current registry, removing a second map/merge peak.
- BindingResolver captured derivation gets remaining allowance; slots/module/key/
  parent indexes reserve while captured projection is live. Existing temporary vector
  transitions charged before retaining.

## Current validation / immediate remaining work

Source is restored after a controlled causal mutant. Receipt
/tmp/projectatlas-731-page-transfer-causal.json records original/restored graph SHA
5ff246c19d347112f1e2a6b69d3a802ac8ab219c615acc6093654d2108cb1699,
mutant exit101 and exact assertion that registry ignored its live page; restored
positivePASS/.13s. A later tiny source refinement makes snapshot Vec capacity explicit
and includes excess returned candidate Vec slots in its receipt; targeted rerun90986
TERMINAL PASS page/query/DB controls + alltargetClippy11.99s. No nativepush/index/timing lease.

All source97390 gates had PASS: Clippy;380CLI/.36.42s,452symbols+1ignored/42.42s,
242DB/29.59s, permanent Kotlin CLI/MCP lifecycle with7newcases/88.72s. After page
ownership + path inventory fixes,55623 PASS causal negative/restored positive,
381CLI/36.01s,242DB/29.95s and the Kotlin lifecycle (inspect terminal log for duration).
Symbols later added a causal boolean-storage control and fixed the new producer's
incorrect bit-packed assumption. Vec<bool> uses size_of<bool>() per element; the
actual-capacity test failed before and passed after. All crates were searched for
that packing assumption; this was the only occurrence. Current40527 PASS453symbols
+1ignored/47.38s,381CLI/36.17s,242DB/29.87s, alltargetClippy8.58s. Current90986 reruns page/query
workspace/DB long-witness controls and alltargetClippy after final capacity refinement.
Logs /tmp/projectatlas-731-projection-budget-{page-unit,workspace-unit,db-unit,clippy,
cli-units,symbols-units,db-units,kotlin-boundary}.log.

Implementation now also fixes the earlier remaining items:
- ResolutionKeyWorkspace reserves distinct keys before clone/move at row +byte caps,
  snapshots return remaining DB allowance, changed workspaces drop before affected
  recovery, dependency set moves into resolver rather than loaded.cloned().
- Actual Contract partial fixture uses source `export {work as ''} from 'pkg'` beside
  valid64-targetimports. It retains a partial valid projection + rejection and refuses
  later sources under2MiB. NUL module was a rejected test attempt: scope normalization
  safely omitted it before Contract, so no causal claim from that variant.
- DB adds source-compatible bounded overloads for export/affected/candidate reads,
  pre-admits normalized input clones and borrowed-row decode, and returns a typed
  RepositoryResolutionReadPage with retained-owner envelope. Runtime keeps page
  ownership charged through registry append and restores scope on all exits. Existing
  unbounded library wrappers retain ordinary page outputs. Read-only long3000-byte
  witness + low1KiB refusals/normal output/publication checksPASS.
- Graph path inventories are pre-admitted at stage entry using ten node/string
  envelopes for direct/current/manifest/package/affected sets and loader inventories,
  including deleted direct paths. Inbound paths transfer into that reserved owner.
- Permanent Kotlin lifecycle now includes val/var/destructured/delegated/for-input
  positives + post-property and loop-body local shadows. Unit lambda proof remains
  separate without a fabricated named-caller claim.

Current validation complete:90986 PASS page/queryworkspace/DBlongwitness controls
and alltargetClippy11.99s;28583 PASS3inventory2.63s/strictOpenSpec/scopedIssueOps5/9;
27697 PASS strictstrings/workspace doctests/rustdoc39.55s. Current navigation checksum
82821d6136374241a25358d9fbdc45267662b36cfeea13bf9239131da9ef5f87 matches both inventories.
Exact-root Luna Max reviewed all current source/spec areas1–3 and APPROVED freeze;
no concrete unresolved implementation defect. Review did not claim current24 runtime,
upgrades/cost/native/hosted acceptance; staging estimates are not exact RSS guarantees.

Next: sourcecheckpoint commit and immutable defaultoptimized24;20native baseline
upgrades stable050/contracts5..23; affected current CLI/MCP/convergence/resource/
performance proof +docs. Normal CLEAN nativepush/hosted4OS/security/freshfeedback/
acceptance remain;737merge+BOTHCLOSED before741. Local/live5/9/allacceptanceOPEN.
External proofdrivers projection-budget24-{upgrade,stage-upgrade,convergence}-proof.py
prepared (8Kotlinfixtures,20unique hashguarded baselines) but not executed yet.
No current24 runtime artifact. No work on other issues.

## Verified public23 baseline (reuse honestly)

Production ff50d228b30b858d03a8f656d68fdb2f1d980c46 DER23/SEM22, bootstrapab14270,
metadata deliveryc802. Immutable executable
~/.cache/projectatlas-benchmarks/binding-731-root-cohorts-contract23-20261031/projectatlas
SHA e963f7fac2ceacbf3ea9caf38df46895fb863d825705da5afb31798606770eec.
Native clean push62972 PASS34Rust suites/378CLI/451symbols+1ignored/54navigation227.25s,
docs/security/scan/lint (14existing asset advisories exit0, sourcepurpose0). 807.77s.
Receipt /tmp/projectatlas-731-root-cohorts23-native-push-session.json +log.
Hosted source41153614591 ALL8jobsSUCCESS, including actual corrected macOS ARM
preflight/E2E; alsoIntel/Windows/Linux/rust/repository. CodeQL41153611803ALL7SUCCESS.

Proof campaign52390 all17tasksPASS:5CLI/MCP,19nativebaselines stable050/contracts5..22,
all16normalized semantic tables incremental/full,identity/allapproved/deadline/refusal/
rollback/reupgrade/noop, resource22->23,132init samples66pairs22fixtures. Receipts
/tmp/projectatlas-731-root-cohorts23-{proof-session,proof-manifest,upgrade-proof,
stage-upgrade-proof,convergence-proof,root-cost-proof,compat}.json.
Auto51222->23 median21.806599->.6021117s,464260->40528KiB,529920->6656deps.
Explicit51221.304527->.5634474s,456956->39432KiB,527872->5632deps.
2048 comparison baseline9fe89a was exploratory retention-only23, NOT accepted runtime:
auto5.036286->2.407838s, explicit4.831136->2.237061s, same dependency counts.
Supplement22851:4automatic layouts13phases root/module add/edit/delete/rename/ambiguity,
all16tables/identity/approved/noop; ...-auto-lifecycle-{proof,session}.json.
Causal old22 root-selector12867deps FAIL; eager-index artifact168parsed app-only edit
FAIL; final lazy<=45,disabledhelper<=2. Cargo root cohorts/key producer keep automatic
Imports vs explicit Configures domains and relevant ancestor lazy module index.
Delivery metadata delta129/130 boundinputs unchanged: navigation checksum fa981d6f...
updated only tests/support/source_contract.rs +docs/v050-cli-e2e-inventory.json; helper
only imported e2e_delivery;3inventoryPASS. This is historical23 reuse, not current24.
Renderer ...-publish-costs.py SHA d37b98db238b5bd5aa3349690fd895808708d3f284f1425fdbb5acd1cfa028fb,
exact reproducibility ...-renderer-repro.json; docs baseline sourceff50/e963.

## Live feedback / security

26threads24resolved; OPEN KotlinPRRT_kwDOQyFgJ86yMGKd/comment4429741442 and aggregate
PRRT_kwDOQyFgJ86yMH47/comment4429752684. Resolve only after public corrected proof.
Source23newCodeQL370/371 trusted GITHUB_PATH/ENV files: counterevidence+officialdocs+
independentNOT_ACTIONABLE, replied/resolved threads, alerts NOT dismissed. Old17Rust
alert owner bodies byte-identical5053->c802;10prodcleanupcounterevidence,244confirmed
owner738;6testhelpers +2oldPython same_directory operator leads. Final current-head
all-alert reconciliation remains required. Receipts ...-toolchain-alert-triage.json,
...-alert-owner-equivalence.json. No security alert dismissal.

## Navigation / environment / parked work

Read complete installed stable050Atlas skill after compaction. Exact731 privateDB;
MCP brief policy_drift/full/changed0 is tracked739; no orientation reset/scan/installer
repair. Native hook scan when publishing is authorized. Source /tmp/projectatlas-rc1-test-env.sh beforeCargo; shared .worktrees/v0.5.1/target and homecache TMPDIR. CleanPATH
omitsrg; use before sourcing or absolute. UNSETNO_TELEMETRY for native suites/push.
Normal push hooks enabled/CLEAN; exactlyone (#731) percommit and sole Fixes#731 PRbody.
StrictOpenSpec binary $HOME/.npm/_npx/21a1280729add0a7/node_modules/.bin/openspec.
Scopedchecker python3 .github/scripts/issue-checklists.py --repo styler-ai/ProjectAtlas
--root . --issue-map openspec/issue-map.json --planned-issue 731.

Parked untouched:741e1e633c +ownedatlas_map/configdirt;7396eb0dfd +ownedwatch/mapbudget;
7388814d6d composes739;708PR728publicc65/local587ecbe closureCLI/MCP+combined4OSperf,
120warm1.127FAIL/focused1.031526partial,725deferred.715integration ownedproposal/design/
specdirt, notRC. RC1root715children708/718/721/722/731/732/738/739/741,milestone37;
closed718/721/722/732. Future41/root493 accepted740/main3ef, exact310/314/388/490/563.
Previousfeedback upload DONE; do not duplicate. Older selected memory retained in Git
HEAD and /tmp/projectatlas-731-active-context-before-budget24-concise.md.
