# Active context — #739, v0.5.1-rc1

## Goal and selected checkout

Finish and normally merge ONE issue at a time: #739 → #738 → #708 → #715,
then publish v0.5.1-rc1 as a non-draft prerelease, Latest=false, and independently
verify downloaded/installed artifacts. Stable0.5.0 stays Latest. Stable publication
and milestone closure require later user instruction. #731/PR737 and #741/PR743
are merged/CLOSED; four RC issues remain OPEN and RC1 is unpublished.

Exact checkout .worktrees/issue-739, branch fix/external-excludes-739, PR745
OPEN, main base6927ed0ca64788db6947b4d43eafce29aac5ed61, Refs #739.
Public/local head ddc90bf72fa11f131b603098f08d89e93b2e2f6c. Normal push29417
TERMINAL0 on2026-11-02T04:25:50Z;1792PASS/34suites/0FAIL/12ignored plus locked
Rust/docs/dependency/repository/Atlas gates. All140 artifact inputs match the
committed tree (/tmp/projectatlas-739-bounded-matcher-commit-source-binding.json).
No local push/build/index live. This memory update is metadata-only; final task/
continuity commit remains after full hosted acceptance. Do not
start738 before739 is normally merged and native CLOSED. No force/admin/hook
bypass, self-approval or database reset. Only task1.1 checked;2.1/2.2/3.1 and
all five acceptance rows remain open pending exact-head hosted/review acceptance.

## Accepted behavior and related fixes

Use gix-config0.62.2 defaults-off/sha1 and ignore0.4.34. Direct Git config reads
are bounded1MiB/file,4MiB aggregate; source/value/path precedence is explicit,
include/includeIf/full Git loading unsupported. Common info/exclude outranks
global, local rules outrank both. Preserve no-Git, inherited rules, ignored-parent
pruning, whitelists, regular-file symlinks and explicit operator root/config/DB
capabilities. RootScanPolicy rebases selected-root aliases without resolving
source targets. Static non-regular policy inputs refuse before parser opens at
root/ancestor/admitted-child owners, without inspecting pruned child rules.

Cold observation rebinds selected external parents and replaced recursive source
roots; queue1024/registrations256/convergence3 remain. Callback has no I/O.
Structural marker/event/sequence share ingress fence; original debounce retains
exact-root rename-gap uncertainty. Controlled watch distinguishes full-required
selection from failed incremental work, retaining complete reconciliation under
original cancellation/deadline and propagating a failed full scan only once.
Native regression edits a large single-file closure, compacts and replaces root;
synthetic100k-event controls do not claim native queue capacity above1024.

Document-only events invalidate warm epochs even when source paths are empty.
Read-only navigation refuses stale state; explicit refresh repairs reasons without
reparsing unchanged sources. SQLite event filtering preserves selected DB opens
and distinguishes selected/resolved runtime namespaces from distinct source
aliases, including Windows short names and retargets. Capturing native DB target
adds one canonicalization per observed request, including warm requests; no
isolated persistent-MCP warm-latency claim. DER25/semantic23, no graph schema change.

Document freshness pages unresolved inputs256 at a time. Shared native-prefix
admission protects cached, uncached and staged document consumers before policy
reads. Final publication discovers fresh policy from original ScanOptions before
transaction, preserving identity, approved purposes and last-valid publication.
#738 retains its complete separate platform/installed/upgrade acceptance.

## Security resource finding and current implementation

Older security P2 PRRT_kwDOQyFgJ86ygte3 confirmed: cold document rows rebuilt
hierarchical matchers per absent target. Page size bounded row memory, not repeated
policy work. Bounded CLI634 reproducer:1024persisted unresolved rows,2000rules,
50890policy bytes, median4.37718s over3cold summaries; init10.21149s. No30minute
denial/RCE/disclosure claim. /tmp/projectatlas-739-cold-policy-cost-before.json.

New additive FS RootScanMatcher borrows captured policy and ORIGINAL control,
uses pinned IncrementalIgnore and one matcher per DocumentResolutionIndex/pass
across all probe pages. Prefix admission remains first. Each parent is matched,
pruned if ignored, otherwise validated/charged before its child policy can load.
Shared enabled-input enumeration retains existing non-regular and Git-only scope.
Warm root/ancestor seed is shared after child eviction; native child keys/Arcs are
released, not retained by the parent's weak map (pinned dir.rs195-259/289-309,
incremental.rs417-477 independently checked). No cross-operation stale cache.

Cumulative defaults PER MATCHER/PASS:1Mcontext loads,128MiB local policy input;
reloads charged again. Child input/native/wrapper keys have soft256context/4MiB
estimates; evict between targets before extending a full cache, allow one current
chain under cumulative bounds. Not exact allocator RSS. Staging/fresh publication
revalidation have separate pass accounting under one operation deadline. Trusted
global rules compiled once by policy discovery; local edits need a fresh matcher,
external edits fresh discovery. Existing single-path API remains fresh; existing
public FsError/IndexWorkResource enums unchanged. No new CLI/MCP/schema surface.

PolicyMatchError preserves filesystem/work failures; quota maps to existing
VerificationIncomplete/PolicyUnavailable, with non-serialized typed resource
marker. Incremental fallback AND observed-MCP incomplete-verification fallback
cannot restart exhausted policy work; unmarked recovery still refreshes. Any
failed matcher pass is terminal (Invalidated on reuse), preventing cached partial
native parse acceptance. Original cancel/deadline survives. Luna731 approved
plan, source/adapter and bounded native cache ownership. Its ignored-cache concern
was withdrawn after checking final-component call order. No open source finding
from that review; full exact-head automated/hosted acceptance still required.

Windows634 failure125067789317 was ingress_filter...structural-parent fixture:
ordinary Win32 normalizes intermediate/.. before lookup, unlike Unix traversal.
New Windows control removes intermediate and reads exact selected rules through
the raw ordinary path, then expects rules visible/sibling filtered. Unix retains
structural assertion; normalized/unsupported verbatim and actual missing-parent
controls retained. No production filtering change. Luna741 approved. Fresh
Windows hosted proof required. Mac synthetic fixtures use captured canonical
native registration namespace, retaining actual selected/canonical write controls.

## Frozen source and completed local proof

Immutable ~/.cache/projectatlas-benchmarks/external-policy-739-bounded-matcher-release.
Fresh locked default release56.12s; CLI SHA256
4307afd4ff50ca401b1c8c50d0d477729ffce6cfa7a98cb70d2b8e799cbc49f9,
69889168bytes.140-input manifest
bb9bf24ff9cec0522faf05df21edaa8648772e8138e7a6ef7c3d9d1b0a456d00,
including new policy_match.rs. Bind every input to committed tree before push.
FShelper544eb8349e6803c427b11382b32b4297e071336f66b547c465ddc8a339d83fc9,
8234512bytes: CHANGED, so all18FS samples rerun, no prior helper sample reuse.
Fresh18CLI cost samples+4upgrade/rollback cases pass; docs/benchmarks/
external-policy-cost.{md,json} bind source/artifacts/medians and limits. +432208
bytes/+0.62% vs acceptedmain. Ordinary init312.3→309.3ms/noop48.3→49.6ms;
10krulesnoop72.5→96.3ms. No #708 waiver. Cold fixture repeats same1024DBrows:
median0.0707262s/init0.264734s, publication unchanged; ~61.9x fixture speed ratio.
/tmp/projectatlas-739-cold-policy-cost-after.json. Sequential3+3warm samples,
different fresh roots, unisolated desktop; workspacecheck/rustdoc overlapped final
FS component trial, not the earlier cold CLI samples. No generalSLO claim.

Tests:6newFS behavior tests; fullFS55PASS; fullCLI460PASS/2ignored38.47s.
Real paged-probe lowquota/state/identity/authored-revision and MCP dispatch/no
retry+ordinary recovery+wire-schema controls pass. Existing prefix/pruning/FIFO,
alias/no-Git/Git controls pass. Locked affected Clippy, workspacecheck, strict
strings, fmt, strictOpenSpec/plannedIssueOps and rustdoc-Dwarnings PASS.
Frozen optimized real E2Es: externalCLI/MCP1PASS1.32s; document-onlyMCP1PASS2.06s;
largeclosure+source-replacementCLI/MCP1PASS14.30s. Inventory source unchanged,
224E2Es/82selectors; normal push selects required repository/inventory gates.
Luna741 approved exact fresh cost packet; receipts' tracked-only dirty_paths omit
new untracked module but hashed140-input manifest explicitly includes it.

## Current hosted and security acceptance

Source41388423972 exactbase6927/head ddc: plan125094298262, Windowsdiagnostics
125094349945, repository125094349962, Rust125094349967, Linux125094350014 and
MacARM125094350053 SUCCESS. MacIntel125094349957 and Windows125094350002 LIVE
at last check; poll those same jobs, do not restart because observation yields.
Linux terminal log confirms externalCLI/MCP, document-onlyMCP, largeclosure/
rootreplacement, ingressfilter and DBaliasretarget regressions PASS. MacIntel
progressed beyond release/resource proof to installed-reader at05:01:12Z, not
stalled. pr-state41388421527 SUCCESS. Old634Windows failure is historical.

CodeQL41388419794 all7SUCCESS: Rust2074364500 has14results, Actions2074291446
has1, other5zero. No clean-scan claim.13retainedRust result facts/fullflows match
634 after exactly unchanged source-line relocation. Former409result absent;
new415 separately inspected, not deduplicated. All15native exactddc instances
fetched viaREST, ordered415/414/413/412/411/410/408/407/406/405/404/372/316/237/236.
/tmp/projectatlas-739-ddc90-{rust,actions}-reconciliation.json and
/tmp/projectatlas-739-ddc90-instance-reconciliation.json retain source evidence.

415 has4completeflows/27relatedlocations: leaseFileopen and Node kind/language/
content_hash aggregate taint is propagated to batch.root, but full3262 and
incremental8878/9067 explicitly assign immutable plan.root. Prefix admission at
graph_projection4082 precedes batch matching; metadata241 only classifies dirs.
Not actionable for supplied flow.414 privatecfgtest copy reads owned fixture DB
content;4aggregateflows do not assign its PathBuf. No universal TempDir/race/
metadata-containment claim; trusted test environment overrides for other alerts
can select non-TempDir fixture paths. No source restriction/suppression/dismissal.

Complete16input current static triage (15native alerts plus the genuine older
resource finding now repaired) is /tmp/projectatlas-739-ddc90-complete-triage.json,
RENDERED through CodexSecurityapp, one row per input. Current secondary citations
come from exactSARIF, not guessed offsets: runtimecfgtest9859-9860/test15463-15485;
FScfgtest1575-1576/rootfixture2527/2528/2529/2533. No runtime-triage claim; separate
fix-finding runtime proof remains above. Current static dispositions need no new
source fixes. Keep required platform/release acceptance distinct.

Code and separate security reviews COMPLETED forddc; codefalse/false, security
flagsnull and older resource advisory still listed until thread resolution.
TechnicalFP414reply4439040686 and415reply4439040822 read back EXACT; matching
threads PRRT_kwDOQyFgJ86yg0JO and PRRT_kwDOQyFgJ86yiVaS RESOLVED. Publicsecurity
summary6563449602 updated/readbackEXACT for this head, replacing oldbad7summary.
ResourceP2reply4438902496 also exact. PR745scope/costbody readbackEXACT.

12reviewthreads/9resolved. Only3 genuine fix threads remain OPEN pending platform
acceptance: document-onlyP2 PRRT_kwDOQyFgJ86yfjX1; controlledfallbackP2
PRRT_kwDOQyFgJ86ygrn7/comment4438294526; resourceP2 PRRT_kwDOQyFgJ86ygte3.
No current new code-review finding. Do not bulk-resolve or infer thread closure
from a completed review summary. Live739scope explicitly includes no observed-
MCP quota retry; PRscopedIssueOps PASS (candidate4/live4/checked1), allpostbase
commit subjects solely(#739), no unrelated mapped task file changed.

## Next and preserved work

Finish current Windows/MacIntel proof and inspect terminal #739 selectors, then
resolve the three examined fix threads, synchronize implementation/five acceptance
rows/liveissue and final metadata-only continuity/tasks commit. Reuse expensive
source proof only with verified unchanged140inputs/artifact identities; normal
protected gates remain mandatory. Normalmerge745/native739CLOSED before738.
Goal remains ACTIVE until actualRC1verification. Source739 worktree must be clean
while any subsequent normal push runs; update integration memory only then.

#738 parked8814d6d+dirtymemory. #708/PR728 publicc65dfed/local587ecbe+dirtymemory,
warm1201.127ms FAIL/focused1.031526ms partial/noSLO waiver. Reconcile live/main
2.3drift when708accepted; don't uncheck private progress. Integration v0.5.1 has
three private715proposal/design/spec edits plus memory; preserve all. Later715
evaluates PR744 outside RC milestone and queued CodexCIgap. Integration entry
.worktrees/v0.5.1/memory-bank/active-context.md; prior708checkpoint preserved.


### Latest verified CI blocker — 2026-11-02

Exact public head ddc90bf / source run 41388423972: Windows job
125094350002 FAILED; macOS Intel job125094349957 remains in progress.
Rust, repository, Linux, macOS ARM and Windows diagnostics passed.
The corrected Windows parent-component policy fixture passes. Twelve later
source_observation tests fail (27 pass), involving warm verification, injected
observer-event filtering and the non-directory configuration-parent refusal.
Full terminal log: /tmp/projectatlas-739-ddc90-windows-ci.log. Root cause remains
under investigation; ordinary/display versus native Windows root representations
is a hypothesis, not an established fixture-only disposition. No source fix for
these failures has been made. Independent read-only review_741 is running.
PR745 remains OPEN/BLOCKED; three genuine fix review threads stay open pending
platform acceptance. Four native milestone37 issues remain OPEN:739,738,708,715.
731 and741 are natively CLOSED. Continue739 only; RC1 remains unpublished.


### Windows CI investigation slice prepared

Confirmed product defect: PolicyWatchInputs::add_path treated Windows NotFound
for a child beneath a regular file as a genuinely missing parent. New owning
guard rejects only confirmed non-directory proper ancestors before any closer
directory resolves, preserving ordinary Win32 canceled file/.. components,
regular/symlink leaves and genuinely absent parent chains. Added shared blocker
depths/positive controls, Unix symlink-target control and Windows actual-read
file/.. positive. Independent Luna731 approved source/spec slice.

Two Windows synthetic-event fixtures now canonicalize root before constructing
DB/events, matching notify's exact registered-root producer rather than the
caller's possible RUNNER~1 temp spelling. Both prove directory case mode.
Added test-build-only Windows event/rebind/continuity diagnostics; no speculative
production event-filter broadening. Luna741 approved fixture/diagnostic slice
(the subsequent insensitive-mode assertion follows its recommendation).

Local42observer+12policy tests PASS; affected all-target/all-feature Clippy,
strict-strings, fmt, strict OpenSpec and planned IssueOps PASS. New Windows
native branches and remaining epoch failures are UNVERIFIED; next ordinary
commit/push must obtain fresh native trace. Prior ddc performance/artifact proofs
remain historical; do not claim new-head artifact/cost equivalence without proof.
All acceptance rows remain unchecked and three real fix threads stay open.
