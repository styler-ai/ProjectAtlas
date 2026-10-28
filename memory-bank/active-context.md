# Active context

## Objective and working agreement

Continue until v0.5.1-rc1 is published and independently verified; v0.5.0 stays
Latest. Stable promotion, #715 task3.2 and milestone closure need later instruction.
Primary implements; existing Luna Max reviewer is read-only and loads the same
custom Rust/database skills and accepted spec. No permission pending. Keep progress
visible, calls bounded and resume live sessions rather than restarting on timeout.

Worktree issue-731 / fix/static-bindings-731, PR737; committed/public HEAD dc3bd90
(contract10), accepted main last1b953f1d. Current source is dirty contract11/11.
Use installed stable0.5.0 Atlas plugin/CLI, exact private root. The MCP brief
returns policy_drift after candidate gates; preserve that DB, do not reset/downgrade
or fullrefresh merely for orientation. Manager has no atlas intentionally. Read
installed Atlas skill, memoryREADME and this file after compaction.

## Current implementation (2026-10-28)

Three review threads remain open: xSmDC body functions/var owner, xSmDH parameter
initializer/body isolation, xSpgU Rust frame accounting. Original4 threads were
answered and resolved against publicdc3bd90 after native proof. Local/live tasks
2.4/2.5/2.6/2.7 reopened;4/9 checked, task3.1+all5acceptance unchecked. Issue731
WhatChanges now includes these broader corrections and removes obsolete task2.3
reopening prose. Live exactreadback and scopedIssueOps9/9/4PASS; milestone37 retained.
/tmp/projectatlas-731-contract11-issue-readback.json /-body.md /-issueops.log.

ECMA lexical source now distinguishes function-body var ownership from parameter
evaluation through EcmascriptVarEnvironment enum. Body fn/var share owner; genuine
nestedblock functions remainseparate; classstatic andTSnamespace bodies ownvars.
Var reservation and actual lexical writes are separate, preserving valid catch
binding writes including destructuring/iteration, let/const ownscope unchanged.
No extra parse/ASTpass/scopes/publicschema. Existing perfile16k budgets retained.
Prior17ecma+TSnamespace focusedPASS; current allsymbols394PASS/1existingignore.
Causal native public10 failures body/default/static/namespace4cases retained at
/tmp/projectatlas-731-function-environment-native-proof.py/json and -native-negative.log.
Current11optimized upgrade/full/ALLpurposes/identity/noop/rollback not yet run.

Rust select reserves both querycopies+BTree bookkeeping BEFORE clone/visitedinsert,
releases exact checked reservation on cycle/success/error; no saturation masking.
Shared resolver private retained_limit defaults to existing2GiB staging cap; tests
inject perinstance, no newpubliclimit. Retained staging includes entities,
candidates, captured sources and resolver caches. Old alert512MiB was inaccurate.

Broader confirmed late-accounting repairs: shared ResolverScratch/CauseWorkspace
admit each uniquekey beforeclone/insertion, rowcap10k beforegrowth, duplicatefree,
cleanup on failure/cancellation and transfer once to completedcache. Rust root,
override, Slots and candidatecall indices reserve beforegrowth, outputrows before
insert. SourceBindingCache.nodes shares immutable Arc map, no secondinventorycopy.
Generic ECMAScript forwarding borrows cached modulekeys, reserves local+visited
querycopies, cleansvisited onErr; sole reexport recursion caller inserts cycle
suffix causes directly into parentworkspace (no uncachedresult charge leak).
module_file admits/dedups missingkeys beforeDBread and transfers once toloaded.
Accepted child/source caches survive parentrefusal. Single bounded request/path
and per-source derivation construction still use existing source/identity limits;
conservative byte model is not exact allocator/RSS/everytransientprecharge claim.

Ten focused realresolver component testsPASS .55s: byte/rowrefusal, dedup,
cancellation, framecycle/errorcleanup, cachedchildpreservation, exactselfreexport
onecompletedowner charge+sameArc retry, missing-source cleanup, loadedkey refusal
beforeDB/read and positive/reuse. Initial synthetic direct-cycle test was replaced
by actual native selfreexport facts, registryentity and cache. Helpers return typed
errors ratherthanpanic in Result; explicit reborrow fixes satisfy Clippy.
/tmp/projectatlas-731-shared-workspace-unit.log.
Causal pairs restored by try/finally:
- /tmp/projectatlas-731-causal-scratch.json/log: originalunreservedframe, missing
cause reservation, missingrowcap, missingroot reservation allFAIL; fixed7PASS.
- /tmp/projectatlas-731-causal-shared.json/log: missing sharedcause reservation,
onequerycopy, HEADoldlate-loaded-copy allFAIL; injected old-equivalent uncached
cycle charge FAILS exactaccount test; fixed10PASS. Generalaliasrefusal test alone
also passes lateadmission (externallysamefailure), so do notcallit allocationcausal;
sharedhelper/loaded/frame/exactcounter controls establish narrower causal proof.

Luna approved the Rust-specific slice, final shared-alias scratch fixes and the
separate ECMAScript environment delta. These source approvals are component
reviews; final #731 acceptance still needs current upgrade, cost and hosted proof.

## Current validation and next action

Actual expanded bare_var_redeclarations_preserve_cli_mcp_calls_and_export_refresh
PASS20.10s: JS/TS bodynegative, default/static/TSnamespacepositives plus existing
aliases, summaries, write/restore, unchangedimporter, incremental/full/identity/noop.
Existing selector expanded (no testcountchange), source digests updated in inventory
and support/source_contract.rs. It needs rerun after latestgeneric changes (active).
StrictOpenSpecPASS currentdesign/spec/docs11; scopedIssueOpsPASS9/9/4.

Affected gate runner session50449 completed successfully. Current fmt, workspace
all-target/all-feature check, Clippy and strict strings pass; symbols394PASS with
one existing ignored test, runtime371PASS, four Rust CLI/MCP testsPASS3.95s, expanded
ECMA CLI/MCP testPASS and three inventory checksPASS. Progress JSON:
/tmp/projectatlas-731-contract11-affected-gates.json; per-stage logs
/tmp/projectatlas-731-contract11-{stage}.log. The earlier47772 test reborrow lint
failure is superseded. Current doctests and warning-strict workspace docs also PASS.
No live build remains at this checkpoint.

Next: finish currentgates, act on finalECMA source review, meaningfulcleancommit,
immutable optimized11 artifact, actualpublic10->11 native upgrade/full/ALLpurposes/
identity/noop/compatiblerollback, retained earlier9/8/7/050repairtests, representative
cleanpairedcost measurement. Then proofLuna review, tasks appropriatecurrentproof,
mandatorynativepush+exacthosted/currentfeedback beforeclosure. 738/739 finalcombined
DER strictlygreater than FINAL73111, semantic11 retained. No current performancewaiver.

## Verified earlier proof to reuse

Typed AST source sidecar preserves import/export/call identity, module namespaces,
exact occurrence ranges including257sameline sites, canonical summarycallers,
Python directives/annotations/readwrite/exports and declaration-only recovery,
deferred stable enclosingtimeline/captured writes, barevar and malformedinitializer
recovery. Native ASTdepth256, lexical names/scopes independently16k. Python directive
readcatalogue conservativelyoverapproximates filewide undercap; documented accepted,
not fullcompiler semantics. No public schema/wire/provider/querytimeparse added.

Publicdc3bd90 complete mandatory nativepushPASS session10413, logcontract10-native-push;
source83b937c cleanoptimized10:
~/.cache/projectatlas-benchmarks/binding-731-83b937c-contract10-20261028/projectatlas.
Actualold8->10 threevarcases+old7->10 fivePythoncases withfull/identity/ALLpurposes/
noop/rollbackPASS. Fresh18pairedcost samples no competingwork: roots4 .281221122
->.344702295s/RSS27952->33204KiB; roots64 .326954193->.393456090/28616->35140;
dense .164662759->.163240066/26648->29328. Root20-23%slower/RSS19-23%higher,
dense<1%faster/RSS~10%higher. Historical10proof doesnotcertifydirty11. Publicreport
still10 untilrebound. Allrawretained. Contract10semanticdigest a1d3ff14480c0efd0dc36ab9abe78ea6ac9423b62ef2ce52b36996436e06b421,
/tmp/projectatlas-731-contract10-semantic-digest.txt; currentrepairunitadds10seed.

## Milestones and sibling lanes

Live37 v0.5.1-00 targetsRC1first. Owner715 no parent; children AND directblockers
EXACT708/718/721/722/731/732/738/739, all37. Closed718/721/722/732, open708/731/738/739;
PR728/737 also37. Actual37membership7open(5issues+2PRs)/13closed; APIcounter6open
is stale. Closed42counter1open hasnoactualopenmember. Do notchurnmembership or
inventscope to forcecounts. Audit/tmp/projectatlas-milestone-live-audit-20261028.json.
725 userdeferredunscheduled;720 historicallyprotecteddev reportvalidbutcurrentmain
soleintegration supportedboundary explicitlydispositioned. Routine734/735 proposals
assessed under715task2.2, not automaticallyvulns. AllconfirmedcurrentproductbugsRC1.
Eachcandidate map remains acceptedbase+ownowner; finalintegration mustunion accepted
731/732/738/739 maps/tasks, notclaimoldintegration maprelease-ready.

738 source8814d6d optimizedstandalone5 in~/.cache/projectatlas-benchmarks/document-738-8814d6d-contract5-20261028/projectatlas.
OptimizedCLI/MCP+3native stable050upgrade/full/state/ALLpurposes/noop/rollback and
strace no outsideignorecontentread PASS, /tmp/projectatlas-738-document-upgrade-proof.json/log.
Luna approvescomponent but7382.1+acceptance wait739sharedlinkedpositive. Doc5projection
contractunit/combinedcounter>FINAL73111/currenthosted/platformproof remain. NoPR/pushyet.
739 plan c411202 localready/high/high, Luna+strict+IssueOps4/4/1PASS; nocodeyet.
Owns stable050 quotedglobalpathwithspaces andlinkedcommoninfo/exclude failures,
provedGitcheck-ignore+explicitfullrescan. CanonicalGitconfigparser requirement,
existingFS/policyowner, scopedconfig/precedence/capture/cancel/cost guards inaccepted
reconcile-external-git-excludes plan. MustimplementforRC1, neededfor738positive.
708 publicPR728 c65dfed local587ecbe correctedCFL guidance; actualCLI/MCPclosureproof
stillpending. Seed/readsamebinary toavoidpolicy_drift fixtureerror. Thencombined
accepted source andfresh4platformperformance; failed120samplewarm1.127 retained,
focused1.031526partial notSLOwaiver. 715 integrationold e54939ec +4ownedplanning/memory
filespreservebeforeupdate. ExactRC1pins/package/parser/installedallCLI/MCP/plugin/
hooks/hosts/upgradesincl045schema/isolation/recovery/concurrency/cost/release/download/
checksums/Latestreadback remain. NoRCversion/tag/releaseexists. 715securitytask2.2
owns currentCodeQL/Dependabot/online-review triage, no blinddisabling/dismissal.

## Execution

source /tmp/projectatlas-rc1-test-env.sh supplies cleanPATH, filesystemTMPDIR and
shared .worktrees/v0.5.1/target. No siblingCLIbuilds duringE2Es; no concurrentbuild/
index/bench duringmeasurements. Default/tmp tmpfs is correctlyrefusedbyWALprofile,
notproductbug. Runtime tests --bin projectatlas, notzero-test --lib --exact. Normal
mandatoryhooks/protectedchecks, noforce/bypass. gh authenticated; explicitShaunTyler
noreplycommitidentity percommand. Longerpriorhistory archivedoutsidecheckout at
/tmp/projectatlas-731-memory-before-contract11-workspace.md.
