# Active context — #739, v0.5.1-rc1

## Goal and selected checkout

Finish and normally merge ONE issue at a time: #739 → #738 → #708 → #715,
then publish v0.5.1-rc1 as a non-draft prerelease, Latest=false, and independently
verify downloaded/installed artifacts. Stable0.5.0 stays Latest. Stable publication
and milestone closure require later user instruction. #731/PR737 and #741/PR743
are merged/CLOSED; four RC issues remain OPEN and RC1 is unpublished.

Exact checkout .worktrees/issue-739, branch fix/external-excludes-739, PR745
OPEN, main base6927ed0ca64788db6947b4d43eafce29aac5ed61, Refs #739. Last
uploaded baseline at this checkpoint:634cf7803977b8a392c77507c5a36de74190e152.
The bounded-matcher change below is ready for its next normal commit/push;
read Git and integration memory for the resulting live SHA/handle. Do not
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

## Hosted and security acceptance still pending

634source41379701198 TERMINAL: plan/Rust/Linux/MacARM/MacIntel/repository/
Windowsdiagnostics SUCCESS; Windows failure above. CodeQL41379697364 all7SUCCESS,
not a clean-scan claim. Current successor head needs fresh required checks/bots.
Do not restart completed634jobs or repeat old pushes because observation yielded.
Normal634push4931 TERMINAL0,1784PASS/34suites/12ignored; historical evidence only.

All15a2nativealert instances fetched; complete static triage prepared in
/tmp/projectatlas-739-a2ccd-complete-triage-prepared.json, NOT RENDERED.13olderRust
facts/fullflows andActions2flows unchanged vs parent; alert414 private cfg(test)
fs::copy TempDir database has4aggregate-taint flows, no hostile path assignment.
Copy reads content, not metadata-only. Keep exact source/test/operator boundaries;
no suppressions/dismissals. Fresh successor SARIF reconciliation/per-input triage
and rendering required, including any changed/new flows. Old public security
comment6563449602 describesbad7 and must be updated after complete current triage.

Open threads: olddocumentP2 PRRT_kwDOQyFgJ86yfjX1; controlledfallbackP2
PRRT_kwDOQyFgJ86ygrn7/comment4438294526 (reply4438359277); resourceP2
PRRT_kwDOQyFgJ86ygte3; AdvancedSecurity414 PRRT_kwDOQyFgJ86yg0JO. Prior7threads
resolved. Completed bot summary does not disposition every open thread. Resolve
only after source-specific proof/replies, current scans/platform and final review.

## Next and preserved work

Normally commit solely(#739), bind140inputs, pushwithhooks from CLEAN issue739.
While pushlive update integration memory only; retain same live handle on waits.
Then app readback, refreshed PR scope/costs, fresh exact-head CI/security/bot
comments, complete triage/render, task synchronization/five acceptance rows,
normalmerge745/native739CLOSED. Goal remains ACTIVE until actualRC1verification.

#738 parked8814d6d+dirtymemory. #708/PR728 publicc65dfed/local587ecbe+dirtymemory,
warm1201.127ms FAIL/focused1.031526ms partial/noSLO waiver. Reconcile live/main
2.3drift when708accepted; don't uncheck private progress. Integration v0.5.1 has
three private715proposal/design/spec edits plus memory; preserve all. Later715
evaluates PR744 outside RC milestone and queued CodexCIgap. Integration entry
.worktrees/v0.5.1/memory-bank/active-context.md; prior708checkpoint preserved.
