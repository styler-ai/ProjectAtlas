## Context

See proposal.md for scope. Stable 0.5.0 is published and verified. The user has moved the complete large-repository performance topic forward from 0.6.0 to 0.5.1, including initial indexing and full reindexing. #708 owns implementation; #715 owns integrated release acceptance.

## Goals / Non-Goals

Use existing release and installed-product gates to verify the entire accepted product and the measured improvements. Keep performance algorithms, benchmark construction and their causal tests in #708. Release ownership adds no indexing/resolution implementation, schema or dependency. After child acceptance, #715 owns required RC version/packaging metadata, shipped workflow guidance and installed-test routing through the existing pipeline.

## Decisions

### Keep implementation and acceptance separate

The existing `v0.5.1-00` milestone has #715 as its sole release owner. #708 is its required direct child and blocker for the complete indexing/refresh performance work. Remove #708 from #493's 0.6.0 child/blocker set while preserving every other accepted child and task owner. The 0.6.0 features consume the accepted indexing baseline later.

#718 separately owns documentation of manual CI eligibility and the existing protected integration route. It changes no runtime or workflow definition.

Confirmed additional owners are #721 for bounded relation navigation and #722 for parenthesized Python imports. Preserve the deliberate relation default/legacy compatibility policy in complete public-surface acceptance. #722 advances the existing semantic contract without changing the storage schema: verify an unchanged installed 0.5.0 index gains resolved file edges and supported aliased callers while identity and approved purpose text/status survive upgrade and rollback. Keep the older 0.4.5 schema migration proof separate.

Additional confirmed fixes need their own accepted owners. #715 implements no product change and cannot start candidate acceptance until the complete #708 work and every other accepted child are implemented, reviewed and closed.

### Reuse the existing candidate and stable pipeline

Freeze accepted main after all children close. Use the existing clean parser handoff, native package and installer jobs, complete CLI/MCP inventory, real host readers and supported predecessor upgrade tests. Match the exact candidate revision, artifacts, runtime, plugin, skill, hook, host configuration, project and database identity throughout. Use isolated fixtures for mutations, administration and failure injection.

Run the existing RC-first process with independently verified tag/head, non-draft metadata, assets and SHA256SUMS. RCs preserve the preceding stable as Latest. Stable promotion repeats proof and verifies Latest and supported downstream pins. Preserve exact published tags; later documentation closeout commits do not move them. Scheduling alone grants no publication authority. RC1 publication has explicit user authorization; every tag and release write still requires the exact reviewed inputs and normal publication gates. Stable promotion requires a later instruction.

Commit identity records provenance; it is not by itself a reason to repeat expensive proof. Reuse passed evidence only when its behavior-relevant source, dependencies, lockfile, toolchain, shipping profile, workflow, configuration, platform and artifact identities still match. New candidate versions, packaging or guidance require their affected construction and validation, including exact final installed identities. Unknown input changes fail closed. Preserve the preregistered performance harness, targets, samples and original failures; do not repeat an unchanged numerical cohort merely to obtain a passing result.

### Preserve version-bound capability provenance

The registry deliberately binds ProjectAtlas-owned parser provenance to the Cargo package version. Follow its existing release policy: preserve all historical seals, advance the accepted capability-set version from 19 to 20 for `0.5.1-rc1`, and generate the current seal and language documentation from the registry. Regenerate the parser accepted-capability manifest and checksum through its existing generator. Retain grammar membership, capability strength, detector and semantic-provider policy, upstream grammar/license/source/ABI inputs and fixture corpus; derived catalog and manifest digests change with the exact registry identity. Verify normal unchanged-index reconciliation and authored-state preservation through the existing installed upgrade and lifecycle proof rather than rewriting an older seal or bypassing digest checks.

Stable promotion must repeat this version-bound generation for its own Cargo version and advance the accepted-set seal again; stable must never reuse the RC1 consumer manifest or provenance. That later transition remains outside current publication authority.

### Route the accepted owners through native packages

The existing source CI and release workflow share the native project-state cohort from `.github/scripts/native-project-state-contracts.sh`. Source CI retains its Cargo selectors; package acceptance selects the exact optimized unit and integration executables carried with the existing contract artifact, while lifecycle and worktree calls bind to the installed candidate through the existing runtime override. Preserve the Windows EFS/short-name/case controls, Linux ext4/f2fs sensitive and casefold/verity/label profiles, and APFS/APFSX/HFS+/HFSX nested-volume and ACL controls on both Mac architectures, including owned cleanup and refusal diagnostics.

The existing packaged contract dispatcher and inventory-checked regression loops also run every permanent static-binding selector from `docs/benchmarks/static-binding-cost.md`. Match names against actual compiled inventories before execution; a selector matching zero tests is not acceptance. This is test routing and reuse of accepted fixtures, without another product implementation, numerical campaign or release pipeline. Preserve the existing shipping-binary digest guards, isolated contract target and native timeout bounds.

### Keep installer fixture authority explicit

The shared installer test-command constructor requires an explicit fixture home and binds home/profile, Codex, AppData and XDG locations there. Test wrappers carry that authority through installation and repair; a missing home fails before a child can inherit the operator installation. Explicit fault and native-alias overrides remain available within each fixture. Check the same factory's callers rather than repairing only a reported PATH example: the product intentionally scans known shim locations under the selected home even when PATH excludes them.

The causal reproduction passed a configuration test while quarantining a different-version runtime in an outer fake home. Preserve that failed isolation proof, add the missing-authority and environment-boundary controls, and verify the corrected real installation/repair workflows plus retained outside runtime bytes. Keep the product's intended stale-runtime quarantine and supported operator paths; this correction belongs to task 2.3's test isolation, with no production installer restriction.

### Preserve native failure evidence without changing acceptance

Native MCP fixtures distinguish the observed child from a diagnostic profiler and record phase/operation timing around the preceding CLI half, flushed requests and received responses. A profiler exit or empty report cannot establish that the MCP child crashed. The shared completion boundary retains the typed primary cause alongside either clean child completion evidence or a secondary shutdown failure; healthy operations discard completion receipts silently. All six MCP stderr owners continuously drain their pipes while retaining at most a 4 KiB prefix with exact total and truncation metadata; every simple session releases its bounded stdout receiver before joining the reader, including after a killed or consumed child. Reuse the same receiver-disconnection mechanism in the existing delivery client rather than leaving another lifecycle implementation behind.

Capture bounded profile and profiler-log prefixes in explicitly selected runner host temp, outside the mounted native image; ordinary Mac invocation uses system temp instead of fixture-selected TMPDIR. Retain real child status, capped stderr and any bounded queued late frames while returning the original request failure. Preserve all request, observer, shutdown and native-step deadlines and the single attempt. The initial APFSX timeout remains failed evidence until affected native proof completes; improved diagnostics alone do not establish a product defect, an environmental cause or successful acceptance.

The next Intel run passed the first APFS product checks, with MCP initialization completing in 191 ms, but then `hdiutil create` reported `Resource busy` while constructing the APFSX image after both prior images were verified detached. The original APFSX timeout was therefore not retested in that run; its cause remains unproven. This is a separate fixture-construction failure. A runner-maintainer [report](https://github.com/actions/runner-images/issues/7522) documents host services temporarily locking newly created images; the specific locker in this run is unproven.

The same candidate reached Apple Silicon's warning gate and exposed an idiom hidden by the timeout reporter's Mac-only method declaration. Keep its portable implementation type- and lint-checked on every host while retaining Mac-only invocation of the native profiler. Use the existing Rust warning gate rather than suppressing the lint or adding a second formatter implementation.

Keep setup recovery separate from product acceptance: allow at most three image constructions only for that specific create failure, use a fresh private output each time without overwrite, retain attempt diagnostics, and verify that every earlier attempt is unattached before retrying. Other failures, existing outputs and unknown or active mappings stop immediately. Cleanup also checks for any remaining image mapping beneath the fixture, including partial creation that never produced the explicit attach receipt. Do not disable host security services, force-detach devices, change the four-filesystem matrix or deadlines, or retry product selectors or numerical measurements. Reuse the shared source/package helper and prove these boundaries through causal fixture controls and the corrected real native routes.

### Explain when to use the accepted commands

The packaged ProjectAtlas skill must describe command purpose and call triggers, not only list functionality. After every compaction, an agent restores the repository's continuity context, selects its own exact checkout and uses the existing Atlas startup funnel to recover the source ownership and architecture needed for the active task. It does not initialize or fully scan an already indexed repository merely because the session resumed.

At meaningful task boundaries, after feedback and before completion, the guide prompts bounded reflection on obsolete paths, duplication, simpler designs, requirements and stale assumptions. Current source, callers, exports and registries remain authoritative. A generated purpose, resolved document link, partial graph or zero inbound rows cannot prove conformance or dead code. Missing specification authority is reported rather than invented; ordinary changed inputs use incremental refresh and wider rebuilds follow the matching typed recovery.

This changes guidance for existing CLI/MCP operations, without a new runtime owner, graph relation, background service or automatic whole-repository audit. Review the guidance semantically against the supported commands and preserve its exact reviewed bytes through packaging and installed-skill verification. The separate complete specification/source relation and purpose-integration verification remains #751's RC2 scope.

### Consume measured performance without weakening correctness

#708 preregisters numerical latency/resource targets and supplies representative large-project fixtures and work counts. Release acceptance runs those cases against the optimized installed candidate with normal telemetry: initial/full indexing, cold CLI, warm MCP, unchanged projects, mixed sparse/dense changes and required exact reconciliation. Report distributions and work separately for each path. A warm result cannot conceal slow fresh CLI or full reindexing. The accepted RC1 scope uses exact reconciliation for explicit refresh; zero-source-work warm admission belongs to unscheduled #725. Measure current-contract opens separately from required first semantic-policy reconciliation. Consume the existing explicit installed harness on the exact RC packages, including its predecessor Python reproduction and corrected targets/callers/purpose status, rather than creating a second campaign or skipping policy invalidation.

The integrated scenario performs an initial index, approves purposes, applies mixed changes, refreshes through CLI/MCP, verifies current source and affected graph results, exercises unchanged refresh and explicit full reindexing, injects cancellation/failure and concurrent changes, and retries safely. Include offline/same-metadata edits, lost events, ignore/parser/compiler changes, worktree isolation and a plain non-Git project. Preserve typed uncertainty and compatible rollback; do not reset user databases for acceptance.

## Risks / Trade-offs

- Partial optimization could be called complete: require separate targets and correctness results for every indexing path before freeze.
- A larger default budget could mask expensive work: inspect visited/read/hashed/parsed/written work alongside elapsed time and explicit-limit behavior.
- Release-only changes could bypass owning review: return confirmed defects to an implementation issue and restart affected proof after its fix lands.
- Planning could be mistaken for a release: leave all future acceptance/publication tasks unchecked and retain 0.5.0 identities until implementation and explicit publication work occur.
- Navigation evidence could be mistaken for implementation truth: preserve the guide's source, specification-authority, freshness and partial-coverage distinctions; keep reflection bounded to the affected task.

## Migration Plan

This owner changes release tracking, candidate version/packaging metadata, shipped guidance and installed-proof routing. Preserve 0.5.0 artifacts, native project databases and authored state. All accepted implementation children must be closed before candidate acceptance. Any required runtime/storage migration must be specified and proved by its implementation owner before this release consumes it. Close #715 only after later-authorized stable publication and successful stable readback; close the milestone afterward.

## Architecture Disposition

N/A: release and runtime ownership are unchanged. Reuse the existing pipeline; #708 owns the indexing architecture and its reviewed diagrams. This release owner composes installed proof rather than introducing another system.

## Dependencies / Cross-Issue Impact

#708 is the required implementation owner, with no new dependency on 0.6.0 features. #708 moves from #493 to #715 as a direct release blocker. #715 does not start candidate acceptance until that complete work and every later accepted child are closed. Preserve the separate 0.6.0 language, agent-continuity and distribution plans. #718, #721 and #722 are additional direct children/blockers with their own accepted specifications; #725 is unscheduled and excluded from RC1.

#731, #732, #738 and #739 are additional required direct RC1 children/blockers. #731 owns cross-language binding and resource corrections, #732 owns the accepted dependency remediation, and #738 owns derived document-target admission before policy reads. #739 owns consistent quoted global/shared-worktree exclusion inputs across full/targeted selection and policy observation. The release's existing dependency/security task 2.2 owns current per-finding scanner and automated-review assessment, including findings reported after #732 closed. Preserve its accepted history rather than reopening it for every new scanner lead. Record reachability, safeguards, legitimate positive workflows and remaining proof gaps; confirmed product defects return to an implementation child and block RC1. A raw alert count is not a confirmed-vulnerability count, and no blanket restriction or blind dismissal substitutes for evidence.

#741 is an additional required direct RC1 child/blocker. It owns early admission of automatic project-state directories, default databases and WAL/SHM sidecars before root discovery, SQLite opens, identity repair or bootstrap writes, plus atomic generated-file publication. Final installed acceptance must combine its refusal and preserved-state proof with positive navigation, source-root aliases and explicit external database/configuration workflows. It does not replace #738's separate document-target policy boundary or authorize a blanket ban on operator-selected external storage.

## Open Questions

None.

## Historical inherited CodeQL disposition (2026-10-28)

The PR737 check reports 11 high-severity path alerts, but independent source/alert comparison confirms that all 11 IDs are also open on accepted main1b953f1d, with the same sinks and shifted line numbers. Successful analyzer jobs and an inherited-alert summary are not security acceptance. No alert was dismissed.

Ten staging-cleanup leads (282,241,278,220,216,243,242,276,221,277) concern unchanged code confined to the explicit selected root's .projectatlas staging area. Direct-child names are checked, lstat rejects symlink stages/databases, recursive payload cleanup requires the staging database to verify exact root/project ownership, and symlink children are unlinked as leaves. When a staging database is absent, cleanup only attempts remove_dir on the direct-child stage, which succeeds only for an empty directory. The reviewed flows provide no evidence that untrusted repository content can delete outside that owned area. Preserve supported operator-selected paths and cleanup; do not blanket-disable them to clear taint alerts. This is a bounded disposition of those exact inherited flows, not repository-wide security clearance or proof against every filesystem race.

Alert244 is also inherited but confirms a real boundary defect: document target policy evaluation precedes prefix admission and can inspect ignore metadata beneath an escaping symlink. Existing required child738 owns the fix and causal syscall/CLI/MCP proof. Its8814d6d source adds target_prefix_refusal before excludes_path, but is not yet accepted on main or present in731. Keep this alert open until the owning fix is integrated, positive linked/external-policy compatibility passes with739, and a fresh combined CodeQL analysis is inspected. Do not classify it as a false positive merely because it predates731.

Task2.2 remains open for the complete current dependency/CodeQL/automated-review inventory and unresolved proof gaps. Task2.1 cannot freeze until all accepted children and reviews close. These dispositions add no unowned bug and do not weaken release gates or supported capabilities.

### Completed static snapshot assessment (2026-10-29)

The normalized scanner snapshot has now been assessed per finding, with the original
test-boundary and individually reviewed dispositions reconciled against eight bounded
review batches. The union covers all 368 normalized IDs exactly once, with no remaining
or overlapping IDs. Seven reported alerts correspond to two confirmed owning defects:
244 remains with #738, while 196, 197, 274, 292, 294 and 295 belong to #741's automatic
state-admission boundary. The other 361 have documented test-only or not-actionable
dispositions against their actual supported paths and safeguards. These are alert
counts, not seven independent vulnerabilities or repository-wide security clearance.

#741's broader causal investigation also reproduces read-only SQLite SHM writes before
root discovery, so an init-only correction is insufficient. Preserve explicit external
database/configuration bindings and ordinary navigation while applying its managed
state checks. Keep both required fixes open until source, positive and failure proof,
independent review and combined installed acceptance pass. No alert was dismissed;
inspect fresh final-source analysis and new findings before task2.2 can complete.

### Accepted implementation status (2026-11-07)

The dated snapshots above preserve their original findings and uncertainty. Their two confirmed owners, #738 and #741, have since been independently reviewed, normally merged and closed, alongside #708, #718, #721, #722, #731, #732 and #739. Their formerly pending status is historical; it is not a current open implementation blocker.

The accepted combined source and its four-platform performance and native compatibility proof are now available for candidate preparation. Task 2.2 still binds the current dependency, CodeQL and automated-review collection to the exact final candidate, reusing earlier per-finding assessment only when the complete claims and relevant inputs match. No historical alert count is a claim of current repository-wide security clearance, and no alert is dismissed merely because its owner closed.
