# Active context — #739, v0.5.1-rc1

## Goal and selected checkout

Finish/normal-merge/native-close ONE issue at a time: #739 → #738 → #708,
then #715 RC acceptance/publication and independently verify downloaded/installed
v0.5.1-rc1, non-draft prerelease Latest=false. Stable0.5.0 stays Latest. Stable
publication and milestone closure require later instruction; #715 remains open
through stable verification. Goal ACTIVE. No force/admin/noverify/selfapproval
or database reset. Primary implements; independent Luna Max reviewers read only.

Selected checkout .worktrees/issue-739, branch fix/external-excludes-739, PR745
OPEN/unmerged, base6927ed0ca64788db6947b4d43eafce29aac5ed61, Refs #739.
Live GitHub reverified2026-11-02: #731/PR737 and #741/PR743 merged/CLOSED;
#739/#738/#708/#715 OPEN. Public head68cb2fd3e7d68c36a1e41414241098dc4ef71a32;
source repair committed ase20f7b5391bdd83669a16f63e5463ad2b372035e.
Subsequent cost/continuity changes do not change the140build inputs.
Only task1.1 checked;2.1/2.2/3.1 and five acceptance rows remain unchecked.

## Current repair and verified results

Public68cb source run41395102219 is TERMINAL: Linux, macOS Intel, macOS ARM,
Rust, repository and Windowsdiagnostics SUCCESS; Windows125114655655 FAILED,
verify FAILED. Do not poll/restart as live. All77 recursive-rebind traces name
Modify(Any) on strict containing Temp directory;42 drain refusals had rebind=true
and continuity_lost=false. Full trace /tmp/projectatlas-739-68cb-windows-ci-clean.log.
Root cause: callbacks unconditionally marked source-root ancestors for rebind;
policy matching likewise treated ordinary ancestor modifications as policy edits.

Current source preserves structural/rescan/error/root uncertainty, queues ordinary
ancestor events without callback I/O, then revalidates captured selected namespace
bindings in CLI/MCP drains under original control. Explicit non-timestamp metadata
remains conservative. Windows notify collapses modification flags: namespace
stability is not proof of which flag changed. Retain symlink/junction controls
including absent leaves; target-directory metadata is distinct from alias-entry
metadata. Native-entry and canonical-target scope indexes select related bindings;
resolve each candidate once and each related binding once across overlapping event
paths. Ordinary source-file namespace work is independent of input count/depth.
Directory work scales with related bindings; watch-directory cap does not bound
input count. Case-mode setup now requires fsutil success, no silent skipped proof.

17 policy tests/43 observer tests PASS; affected all-target/all-feature Clippy,
strict-strings, fmt, strictOpenSpec, planned and PR IssueOps, affected-ci-proof
self-test PASS. Dense resource test uses1,024 absent inputs/one external directory,
constant unrelated event resolution and exact P+eventPaths overlapping-directory
work, plus appearing-input positive. Initial ancestor regression causally failed
before fix and passed after (/tmp/projectatlas-739-ancestor-metadata-{before,after}.log).
New logs /tmp/projectatlas-739-indexed-namespace-{policy,observer,clippy,ci-proof,
openspec,issueops,pr-issueops,strings}.log. Luna731 APPROVES scope/resource/CI change;
Luna741 APPROVES correctness and inter-call transition fix after deterministic
removal/reparse regression. Earlier test gap corrected. Windows
cross-target check UNAVAILABLE: missing x86_64-w64-mingw32-gcc, not a source pass.
Fresh hosted Windows proof remains required. Intermediate95475 superseded; final
build63729 TERMINAL0 after repeated observer/Clippy/strings/OpenSpec PASS.
Final build38.31s; receipt under /tmp/
projectatlas-739-indexed-namespace-release-{artifacts.jsonl,build.log,build.time}.

Local64ce also fixes automated review4439249685: source-equal full-refresh no-op
must run existing bounded fresh document-policy probe, then recheck publication
generation. capturedNone/false never prove current policy. No new SQL/schema/source
scan/graph copy/transaction. drop(current) releases source-comparison ownership
before document maps. Owning causal test failed expectedMissing/gotIgnored before,
then passes full/source-overflow/document-overflow, ignored/missing/outside-root,
identity/purpose, parsed0/no-op, cancellation/deadline; nine related tests passed.
Current owning test rerun PASS1.43s. Luna731 source/spec approved. Generation fence
is statically checked; no deterministic inter-page concurrent-publisher test added.

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

## Current optimized artifact and local acceptance

Frozen ~/.cache/projectatlas-benchmarks/external-policy-739-indexed-namespace-release,
sourcee20f7b5.140buildinputs match committed tree; manifestbc24b111d18db378327751f9d7ab78e932ffcb0098aefc232a92982c691bd8cd.
CLI1dd76da9ec3df10686c2d92d8309256a170b02ca84d98ccf156a6b692579f28d,
69,971,552bytes. FShelper544eb8349e6803c427b11382b32b4297e071336f66b547c465ddc8a339d83fc9
byte-identical to historical measured helper, so18FSsamples reused explicitly;
not a claim of36new samples.18freshCLIcost/fourupgrade+rollback casesPASS.
CLIordinaryinit308.79→311.76ms/noop48.50→50.32ms;10krulesnoop71.39→98.87ms.
Actual optimized externalCLI/MCP1.29s,document-onlyMCP2.04s,largeclosure/root-
replacement14.30s PASS. Final logs /tmp/projectatlas-739-indexed-namespace-*
{cli-cost-proof,upgrade-proof,external-e2e,documents-e2e,replacement-e2e}.
Cold-currentfixture3newCLIprocesses on one newlyinitialized DB median.0685849s,
init.344581s;1024persisted missing document relations verified and publication
unchanged. Earlier exact-identity baseline4.37718s retained (~63.8x fixture only),
not a generalSLO/speedup claim. CurrentCLI/coldmeasurements had no overlapping
builds. PriorFSsamples retain documented workspacecheck/rustdoc overlap.
Cost docs bind currentCLI/source/helper identities; Luna731 APPROVED final
cost/proof reconciliation, including all three real-adapter logs.
All local sessions63729/39527/13827 TERMINAL0. No live build/index/push currently.

## Immutable historical artifacts and next proof

Previous ddc90bf artifact ~/.cache/projectatlas-benchmarks/
external-policy-739-bounded-matcher-release: CLI4307afd4ff50ca401b1c8c50d0d477729ffce6cfa7a98cb70d2b8e799cbc49f9;
140-input manifestbb9bf24ff9cec0522faf05df21edaa8648772e8138e7a6ef7c3d9d1b0a456d00.
FShelper544eb8349e6803c427b11382b32b4297e071336f66b547c465ddc8a339d83fc9.
All18FS+18CLIcost/fourupgrade cases passed then; optimized externalCLI/MCP,
document-onlyMCP and largeclosure/source-root replacement passed. Bounded cold
1,024-link/2,000-rule fixture4.37718→.0707262s (~61.9x) is fixture-only, no general
speed/SLO claim. Cost docs currently describe this historical packet.

Intermediate clean64ce artifact external-policy-739-document-noop-release:
CLI50c99150c8a2cce53f7bd6be8a1be99faae5b4a9823cb9cc9beaa81a0adbe6c6,
69,872,736bytes;140-input manifeste561c712e673b12b1db40512458b394b315122ce62a4270207a90b981aa76261.
FShelper byte-identical to ddc.18freshCLIcost/fourupgrade casesPASS; quiet final
CLIcost rerun ordinaryinit308.96→309.84ms/noop49.44→48.85ms;10krulesnoop70.26→97.31ms.
These two historical artifacts are superseded by the current e20source-bound
artifact above; no equivalence claim for their CLI binaries. Current cost docs
were updated after new CLIcost/fourupgrade/real-adapter proof. /tmp/projectatlas-739-freeze-document-noop.py and document-noop proof scripts
are reusable only after adapting immutable output paths and checking assumptions.

## Hosted/security feedback still requiring final acceptance

Public68cb CodeQL41395098627 all7SUCCESS: Rust2074629854 has14results,
Actions2074560814has1, other5zero. Complete facts/fullflows matchddc after exact
unchanged Git line relocation; /tmp/projectatlas-739-68cb-{rust,actions}-reconciliation.json.
Correct PR-ref-scoped15native inputs:415/414/413/412/411/410/408/407/406/405/404/
372/316/237/236, exact68cb instances fetched+validated. Intake needs explicit
ref=refs%2Fpull%2F745%2Fhead. Earlier unscoped request fetched391defaultmainalerts:
preserved *unscoped-main* is an intake error, NOT this PR's current collection.

Prior ddc complete16input static triage was rendered through Securityapp:
/tmp/projectatlas-739-ddc90-complete-triage.json,15native+genuine resourceP2 nowfixed.
415 aggregate taint does not assign immutable plan.root; native prefix guard
precedes matcher metadata.414 privatecfgtest DB copy reads owned fixture content;
other privatecfgtest helpers may intentionally use trusted outside paths. No
blanket TempDir/root-jail/race claim, suppression or native alert dismissal.
Reuse verified flow facts with exact current mapping; final current-source full
static collection assessment/render remains pending. Static triage is INLINE,
no agents/builds/tests/source edits while in that phase; read skill+SECURITY first.
Both code and separate Security reviews completed for68cb; code true/true from
new document-noop finding; security flagsnull. No clean-scan claim.

13threads/9resolved/4genuineOPEN before latest refetch: document-onlyP2
PRRT_kwDOQyFgJ86yfjX1, controlledfallbackP2 PRRT_kwDOQyFgJ86ygrn7,
resourceP2 PRRT_kwDOQyFgJ86ygte3, new4439249685 (reply4439288172 readbackexact).
FP414/415 replies4439040686/4439040822 and threads were readbackexact/resolved.
Publicsecurity summary6563449602 is historicalddc. Final PRbody/securitysummary,
feedback reconciliation, task/fiveacceptance synchronization, normal cleanpush,
exacthead hosted proof and required review/thread closure remain. Do not mark
Closes739, resolve genuine threads or merge until complete acceptance.

## Parked work and tooling

#738 parked8814d6d+dirtymemory. #708/PR728 publicc65dfed/local587ecbe+dirtymemory;
warm1201.127ms FAIL/focused1.031526ms partial, noSLO waiver. Reconcile live/main
2.3 drift when708 accepted, preserve private progress. Integration .worktrees/v0.5.1
has three private715 proposal/design/spec edits plus memory; never revert. Later715
evaluates PR744 outside RC milestone and CodexCIgap. Do not start738 before739
normally merges and GitHub confirms CLOSED.

Source /tmp/projectatlas-rc1-test-env.sh for cargo/gh (itsPATH removes rg).
Installedstable050 exactissue739 DB preserved; MCP brief returned policy_drift,
requested one watch_once task index-7 now TERMINALcomplete. No DB reset. New source
edits may stale index again; reuse exact current source slices, no stale graphclaims.
Local sessions56944/83379/48466/67916/61561/61269/95475 are terminal; crosscheck48466 failed only
missingcompiler. Old normalpush63564 TERMINAL0 at05:37:17Z:1794PASS/34suites/12ignored.
No repeated poll of terminal jobs. During normalpush hooks do not edit selected
checkout; update integration memory only. Progressvisible≤60s, no wait>60s.
