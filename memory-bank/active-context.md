# Active context — #739, v0.5.1-rc1

## Goal and selected checkout

Finish/normal-merge/native-close ONE issue at a time: #739 → #738 → #708,
then #715 RC acceptance/publication and independently verify downloaded/installed
v0.5.1-rc1, non-draft prerelease Latest=false. Stable0.5.0 stays Latest. Stable
publication and milestone closure require later instruction; #715 stays open
through separately authorized stable verification. Goal ACTIVE. No force/admin/
noverify/selfapproval or DB reset. Primary implements; Luna Max reviewers read only.

Selected .worktrees/issue-739, fix/external-excludes-739, PR745OPEN/unmerged,
base6927ed0ca64788db6947b4d43eafce29aac5ed61, Refs739. Publicheadb703f544167e452310f36ea2870c433193b78906.
This source/continuity checkpoint is being committed as its follow-up; use Git HEAD
for exact localcommit. Only1.1 checked;2.1/2.2/3.1 and all5acceptance rows unchecked.
#731/PR737 and #741/PR743 merged/CLOSED; #739/#738/#708/#715 OPEN, RC1unpublished.

## Current follow-up: prefix-probe security fix and native fixtures

New Securitybot4440644526 onb703 identified repeated target_prefix_refusal
canonicalization outside matcher quotas. Cold CLI/MCP paged freshness, cached
projection and staged/final document verification share this DocumentResolutionIndex
owner. missing_or_escaping_ancestor had the same uncontrolled probe mechanism.
Primary used fix-finding workflow: real optimized beforefixture512missinglinks/
64existingdirs init5.320s/coldsummary2.600s, unchangedpublication. Separate strace
subset(readlink/readlinkat/newfstatat/statx/openat):1,302,683calls,1,300,153readlink.
Causal owning32dirs/128targets test BEFOREFAIL4352canonicalizations; AFTERPASS
<=384canonicalizations and <=8704metadata+canonicalprobes, independent native
counter equals ledger. High-level probe count is not a constant-time syscall.

Current fix keeps admission FRESH per distinct target: recanonicalize selected
root against captured canonical_root; lstat normal components, canonicalize each
symlink and every Windows reparse component immediately, reject first escape
including outside-then-reentry, stopfirstmissing, retain Unsupported uncertainty.
No cross-target positive admission cache. Ancestor fallback uses the same fresh
controlled walk. Scalar per-index ledger4Mprobes charges BEFORE every owned native
prefix/leaf/ancestor metadata/canonical operation across rows/pages. Cached states
stillcheckoriginalcontrol and terminalexhaustion. Constructor's control-checked
rootcapture is fixed separate setup, matcher keeps separate context/byte quotas.
Flatmissingtarget4probes; deeper targets spend samequota onextra checks.

CLI-private VerificationWorkLimit::Policy(PolicyMatchResource)|DocumentPrefixes
keeps existing serde-skipped policy_work_limit field. No publicFS/core errorenum,
CLI/MCP schema, graphidentity, SQL/schema or derivation/semanticcontract change.
Quota refuses through existing VerificationIncomplete/PolicyUnavailable and cannot
restart via incremental or observed-MCP full fallback.39documenttests PASS incl
page-spanning nativequota, cached/uncached stickyrefusal, fresh root/aliasretargets,
outside-back-inside, positivealiases/missing/regular targets, cancellation and
last-valid publication/identity/purpose controls. MCP no-retry/wire+normalrecovery
selector PASS for BOTH marker kinds. Affectedalltarget/allfeatureClippy, strict-
strings/fmt/strictOpenSpec/scopedIssueOps PASS. Luna731 plan+actualsource APPROVED.
Logs /tmp/projectatlas-739-prefix-{probes-before,probes-after,document-suite,
mcp-quota,clippy,strings,issueops}.log. No global/exhaustive security claim.

Windows b703 job125188004554 TERMINALFAILED earlyunits: FS47PASS; policy12PASS/
3FAIL, observer command not reached. Failures are test-fixture setup: display
forwardslashes make cmd mklink report Invalid switch Users; fsutil needs an empty
directory and returned0 despite printing directory-not-empty. Current test-only
repair uses cmd /D, Win32 backslash args; removes Rules before fsutil, requires
success+actualnativecaseflag, restores exact Rules/content and asserts lowercase
selectedalias missing. No skip/waiver. Luna741 approved fixture+analogouscall audit.

MacIntel b703 job125188004525 TERMINALFAILED automatic_state_admission_preserves_
cli_mcp_navigation under APFSX with generic MCP Timeout35.68s; previous volume
cohort same selector PASS16.50s. Underlying cause UNRESOLVED, not called flake or
fixed. Current E2E-only diagnostics print stage/suffix and requestid+tool, preserve
10s request deadline, distinguishTimeout/UnexpectedEof. On actual Mac timeout,
sample the still-owned/unreaped MCP PID: assert_cmd5s sampler timeout, structured
quoted shell positional args, stdout/stderrnull, coreoff, ulimitfile256blocks,
explicit NamedTempFile report, read<=32768bytes, automaticcleanup, best-effort
logging preserves primaryerror. Exact capture block typechecked with project
assert_cmd/tempfile on host; fullmac execution pending. Luna741 reviewed bounds,
lifecycle, cleanup, context and primaryerror: APPROVED. No timeoutincrease/retry.

## Current immutable artifact and local adapter proof

Final source folder ~/.cache/projectatlas-benchmarks/external-policy-739-prefix-admission-release:
CLIe89af7a49cca79336001fc8c395c4166bda1de367b06be94049e829d78b28d06,
69,962,536bytes; FShelper544eb8349e6803c427b11382b32b4297e071336f66b547c465ddc8a339d83fc9.
140-input manifest243a8385160eb477c1670b8ee595a590b6a5968841cacd836d8c7ebadd24bf4a.
Built from saved dirty source; receipt preserves that truth. NEXT: bind ALL140
saved hashes to the new committed tree and clean checkout before normalpush.
No further source edits without rebuilding/rebinding affected proof.

Fresh measured source manifestbcd077917dd9ae3ff814603f18908c8bbc167a83cb79dfd45648c9dc0187bbea
at intermediate external-policy-739-fresh-prefix-release has EXACT SAME CLI/helper
bytes. Only E2E failure-diagnostic source changed between those manifests. Final
rebuilt byteidentity verified, so reuse these fresh product measurements explicitly.
FShelper identical to older measured ddc component:18FSsamples reused with old
manifest/overlap limitation.18freshCLIcost/fourupgrade+rollback PASS at e89;
ordinaryinit308.87→306.85ms/noop48.29→49.70ms;10krulesnoop70.85→98.41ms. No SLOwaiver.
Root512/depth64 AFTER init.735s/coldsummary.322s, unchanged512missingrelations/
publication; tracing73,883calls/5,305readlink (separate traced timings, not compared).
Originalshallow1024link/2000rule fixture3coldreadsmedian.0684947/init.273917s;
historical exact-identity predecessor4.37718s retained, not a generalspeed claim.

Optimized e89 realE2Es PASS externalCLI/MCP1.24s, document-onlyMCP2.04s,
largeclosure/rootreplacement14.42s, automaticstateCLI/MCP Linux2.01s and primary-
failure/shutdownpreservation. Native Windows/APFSX and combinedrelease proof STILL
REQUIRED. /tmp/projectatlas-739-fresh-prefix-{cli-cost-proof,upgrade-proof,
cold-policy-cost,external-e2e,documents-e2e,replacement-e2e,state-e2e,shutdown-e2e}.*;
/tmp/projectatlas-739-document-prefix-cost-{before,after}.* and prefix-syscalls-*.
Cost docs bind current CLI+manifest and explicitly distinguish FS reuse and
E2E-diagnostics-only byte-identity reuse. Final cost/source binding review next.

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

## Public b703 hosted/security state and next steps

b703 normalpush99029 TERMINAL0 at09:10:01Z:1801PASS/34suites/0FAIL/12intentional
ignores plus normalRust/docs/dependency/repository/Atlas gates. Do not repoll.
Source41418445583 terminaljobresults: plan/repository/Rust/Linux/Windowsdiagnostics/
MacARM SUCCESS; MacIntel and Windows FAILED as above; inspect run aggregate if
needed. Never treat those failures as LIVE or restart merely for observation.
pr-state41418441850 and41418496481 SUCCESS; metadata41418495571 follows source.
Code review6562604562 onb703 COMPLETED false/false; separateSecurity COMPLETED
true/true with originalpolicyresourceP2 and newprefixP2. This does not resolve
native threads. PRbody last readbackexact still describes b703; rewrite for new
followup/freshsource before/after nextpush, preserving Refs739 untilaccepted.

CodeQL41418440583 all7analyses COMPLETE: Rust2075694918has14, Actions2075580323
has2, other5zero. Currentraw metadata /tmp/projectatlas-739-b703-analyses.json;
ActionsSARIF /tmp/projectatlas-739-b703-actions.sarif.json. New Actions row is
ci.yml472–483 earlyunits; retainedrow1216–1226 lateE2Es. Both claim poisoned
needs.plan.outputs.head on schedule/dispatch. NOcurrentverdict assigned; trace
currentplanner/eventhead/cache controls and keep one result per nativeinput.
Fetch full exactPRref-scoped openalerts+instances and RustSARIF, normalize WHOLE
collection incl2genuinebotresourceclaims before finalinline/statictriage/render.
REST only findingintake; explicit ref=refs%2Fpull%2F745%2Fhead is REQUIRED. Do not
repeat unscoped391defaultmain intake mistake. Prior16input ddc renderedtriage
/tmp/projectatlas-739-ddc90-complete-triage.json is HISTORICAL, reuse verified
reportedflow facts only with exactcurrentmapping. Statictriage phase: no agents,
source edits/builds/tests; leave phase explicitly before dynamicfix work.

Now5genuineopen threads expected (refresh live): document-onlyP2 PRRT_kwDOQyFgJ86yfjX1,
controlledfallbackP2 PRRT_kwDOQyFgJ86ygrn7, policyresourceP2 PRRT_kwDOQyFgJ86ygte3,
fullnoop4439249685, prefix4440644526. FP414/415 previously dispositioned/replied/
resolved; do not bulkresolve realfixthreads before complete acceptance. No native
alertdismissal/suppression. Live739WhatChanges expanded/readbackexact for native
prefix work; localtasktext unchanged/scopedIssueOps PASS. Allacceptanceunchecked.

Next: finalcheapgates/source+cost commit/all140Gitbinding, finalcostreview, normal
cleanpush withhooks, currenthead nativeWindows+APFSX/platform proof, automated
code+separateSecurityfeedback, wholecurrentsecuritytriage+render, examinedthreads/
implementation/fiveacceptancesync, normalmerge745/native739CLOSED BEFORE738.
Goal remains ACTIVE until actual RC1 downloaded/installed independentlyverified.

## Parked work and tools

#738 parked8814d6d+dirtymemory. #708/PR728 publicc65dfed/local587ecbe+dirtymemory;
warm1201.127ms FAIL/focused1.031526ms partial, noSLO waiver; reconcile live/main
2.3drift onlywhen708accepted, preserve privateprogress. Integration .worktrees/v0.5.1
has3private715proposal/design/spec edits plusmemory; never revert. Later715
evaluatesPR744outsideRCmilestone andCodexCIgap. Source /tmp/projectatlas-rc1-test-env.sh
forcargo+gh (PATHremovesrg). Exact installedstable050 Atlas DB preserved, most
recent onepass refresh index-8 TERMINALcomplete; no reset. Source edits can stale
index, do not claim stalegraphfacts. All local builds/proofs/exec sessions in this
checkpoint are TERMINAL, no livepush currently; inspect newhandles when launched.
