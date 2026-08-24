## ADDED Requirements

### Requirement: #492 is the feature-free v0.5 release hierarchy root
#492 SHALL have no native parent and SHALL be the sole direct parent of all twenty-nine other accepted `v0.5.0-00` issues, including dependency-free acceptance-contract owner #500. It SHALL be directly blocked by every child and SHALL implement no feature or bug. #500 SHALL NOT become a native blocker of product issues whose implementation does not consume a genuine #500 product boundary.

#### Scenario: Milestone progress is inspected
- **WHEN** the native hierarchy is read
- **THEN** #492 exposes every accepted child once while each issue's direct blocker list independently communicates execution order

#### Scenario: An implementation-bearing child is incomplete at RC
- **WHEN** any feature, bug, or maintenance child other than the declared release-governance campaign #499, or any of its mapped tasks, required proof, or review, remains open
- **THEN** #492 cannot freeze or publish an RC candidate

#### Scenario: Release-governance issues remain open at RC
- **WHEN** trusted release-version classification selects prerelease, every implementation-bearing milestone issue is closed and complete, the active release graph identifies #492 as its unparented release root and #499 as its sole campaign, and #499 has exact `candidate_ready` revision/inventory/config/audit evidence for the frozen candidate
- **THEN** the prerelease milestone gate MAY permit exactly #492 and #499 to remain open for RC publication
- **AND** both open packets and native roles remain exact without treating their later stable/closure work as complete

#### Scenario: Another issue is open at RC
- **WHEN** the milestone contains any other open issue, either governance role differs from the active graph, the campaign stage is not exact `candidate_ready` for the release input, or the graph/packet readback is stale or mismatched
- **THEN** the prerelease milestone gate fails closed without accepting a caller-supplied allowlist

#### Scenario: A child or prerequisite root task is incomplete at stable
- **WHEN** any child including #499 remains open, any child task, proof, review, or stage evidence is incomplete, or any #492 task other than exact finalization task 26.6 remains unchecked
- **THEN** the stable prepublication finalization gate fails before packaging

#### Scenario: Stable finalization is ready before packaging
- **WHEN** trusted release-version classification selects stable, #499 has exact `stable_ready` evidence and is closed so every child is closed, every child task and #492 task other than 26.6 is checked, the accepted RC plus repeated semantic, published-main, source/workspace, security, release-policy, and package-input preflight binds the exact stable input, and #492 is the sole open milestone issue with 26.6 the sole unchecked task
- **THEN** the stable prepublication finalization gate MAY permit only #492 to remain open and only exact task 26.6 to remain unchecked for stable packaging, installed-product proof, publication, exact hosted/Latest/downstream-pin readback, synchronization, and closure
- **AND** it SHALL NOT reuse the prerelease permission for #499 or admit another issue, task, role, revision, or caller-supplied allowlist

#### Scenario: Stable finalization has extra or stale residue
- **WHEN** another issue is open, another task is unchecked, the release-root role or exact stable input differs, or any required prepublication readback is stale or mismatched
- **THEN** the stable finalization gate fails closed without packaging

### Requirement: Lean CI and the dependency campaign remain release gates
Pull requests SHALL run the smallest contract-complete existing proof selected by the single #497 affected-contract planner, with human and Dependabot parity, ordinary additions/modifications eligible for known-impact union, and every rename/deletion plus unknown/shared/planner-owning input failing closed to complete proof. Default-branch, scheduled, candidate, and release boundaries SHALL run complete proof. Each release SHALL declare at most one active Dependabot campaign issue; v0.5.0 SHALL declare #499 and use its same body region for exact candidate-ready and stable-ready evidence.

#### Scenario: Pull-request proof is narrowed safely
- **WHEN** the exact current plan proves a closed subset of existing contracts affected
- **THEN** stable required contexts aggregate affected success or exact plan-bound not-applicable evidence
- **AND** missing, skipped, canceled, stale, malformed, or mismatched proof fails

#### Scenario: Shared or release proof runs
- **WHEN** proof runs on the default branch, a schedule, a candidate, or a release boundary
- **THEN** the complete repository proof executes without treating pull-request not-applicable state as release evidence

#### Scenario: Dependabot campaign is incomplete
- **WHEN** #499 has a pending or provisional PR/finding record, a failed/canceled/uncertain hosted audit run, no matching current final `clean|findings` campaign audit record, a missing exact campaign relationship, incomplete review/thread/protected-context/applicable-Sol-authorization readback, or stale/malformed stage evidence
- **THEN** candidate-ready or stable-ready fails closed

#### Scenario: Dependabot campaign is candidate-ready
- **WHEN** the pre-RC audit completes successfully as `clean` or `findings`, every unlinked pre-PR finding is finally deferred, declined, or superseded, every linked finding points to a finally dispositioned real PR, `accepted` is used only for a finding linked to a finally accepted real PR, every candidate-snapshot PR record is final, and revision/inventory/config/audit/hosted-run identities match at publication preflight
- **THEN** #499 records `candidate_ready` and RC1 may publish while #499/#492 remain open

#### Scenario: Dependabot campaign is stable-ready
- **WHEN** RC1 is accepted, the later pre-stable audit completes successfully with a matching current final campaign audit record, every unlinked finding is finally deferred/declined/superseded, every linked finding points to a finally dispositioned real PR, `accepted` is used only for a finding linked to a finally accepted real PR, every newly observed/full-window PR record is final, and the exact stage/full-union readback matches
- **THEN** #499 records `stable_ready`, may close, and may unblock stable #492 acceptance without per-PR issues or weaker proof

### Requirement: Accepted issue evidence is published before implementation
Every issue assigned to `v0.5.0-00` with `status:ready` SHALL resolve its mapped OpenSpec task source, restored acceptance-oriented issue body, and every architecture URL, heading, and Mermaid block from an exact clean checkout of the live default-branch revision. Before readiness or implementation handoff, Sol SHALL reconcile the issue's actor/current state/consequence/outcome, observable behavior and owning boundary, capability truth, release role and genuine dependencies, two-to-five-bullet positive/negative/compatibility/no-change acceptance as applicable, non-goals and failures, diagram meaning and rendered truth, and exact task ownership/text/state. Candidate-local validation SHALL remain required for proposed artifacts but SHALL NOT authorize readiness, milestone assignment, native release relationships, implementation handoff, merge, or release.

#### Scenario: A planning slice publishes new evidence
- **WHEN** a planning pull request has no native closing issue, its candidate OpenSpec and Mermaid checks pass, and independent Sol plus a new hosted Codex review accept its corrected exact head
- **THEN** primary Sol validates the tracked body manifest's schema, unique exact issue set, normalized content, and SHA-256 values, publishes only its reconstructed thirty exact bodies, and reads back the same normalized bytes and hashes while the PR remains open, required task mappings remain ownership-only, and the authoritative/live graph remains at twenty-five children
- **AND** the temporary body-to-main architecture-link gap authorizes no readiness or implementation handoff

#### Scenario: Body bootstrap makes normal planning CI pass
- **WHEN** the tracked body manifest, ignored-copy comparison, exact task mirrors, write-time state, and normalized live-body hash readback all agree while the planning PR remains open
- **THEN** normal unfiltered IssueOps/CI passes before primary Sol authorizes the planning merge and reads back exact `main`

#### Scenario: Repository implementation precedes native graph activation
- **WHEN** the planning artifacts are exact on `main` but Luna's objective checker/forms/guidance implementation has not yet landed, received review, and been read back
- **THEN** primary Sol does not apply milestone/native relationships and no issue becomes ready

#### Scenario: Candidate graph is promoted after hosted bootstrap
- **WHEN** the accepted Luna implementation is on `main`, exact body and planning-main readback agree with the tracked body manifest, and primary Sol applies and reads the complete hosted milestone/native relationship bootstrap from the accepted graph manifest
- **THEN** a separate narrow PR atomically replaces only `openspec/issue-map.json.release_graphs.v0.5.0-00` with the exact #499 campaign declaration and replacement graph, removes both candidate manifests, and requires exact merged-main plus live IssueOps/hosted reconciliation before readiness

#### Scenario: Candidate and authoritative graph state differ
- **WHEN** a required review, manifest entry/hash/removal, body, planning/implementation revision, mapping, milestone, parent, blocker, graph node, or published revision is missing, stale, raced, or mismatched during bootstrap or promotion
- **THEN** IssueOps remains fail-closed and no readiness, handoff, merge authorization, or release transition proceeds

#### Scenario: IssueOps passes but the packet is not comprehensible
- **WHEN** an issue has the required sections, acceptance bullet shape, exact tasks, and valid diagrams but Sol cannot answer one or more semantic review questions from the packet
- **THEN** the issue remains unready and implementation handoff is forbidden without asking IssueOps to score prose

#### Scenario: The shared acceptance mechanism is not accepted
- **WHEN** an issue such as #497 has a semantically reconciled candidate packet but the #500 objective mechanism and section order it requires are not accepted on published `main`
- **THEN** the existing readiness gate blocks handoff without adding a native #500 product dependency edge

#### Scenario: #500 completes before the shared readiness task is synchronized
- **WHEN** #500 finishes exact publication/readback, semantic reconciliation, and its final implementation-versus-diagram review
- **THEN** #500 may complete without checking or depending on shared release-readiness task 1.4, after which primary Sol independently synchronizes that shared task

#### Scenario: Candidate-only evidence is presented as published
- **WHEN** a planned, implementation, merge-authorization, milestone, or release check resolves an artifact only from a candidate, stale, or dirty checkout
- **THEN** readiness fails until a planning pull request lands and the exact live default-branch artifact is read back successfully

#### Scenario: The local checkout is not the exact published root
- **WHEN** Git reports another top-level root, a malformed local HEAD, a well-formed but different HEAD, or tracked modifications
- **THEN** published readiness fails before the checkout's OpenSpec or architecture artifacts authorize state

#### Scenario: Only ignored untracked notes exist
- **WHEN** the exact live default-branch checkout differs only by untracked files excluded from tracked publication identity
- **THEN** published readiness may continue without treating those notes as repository evidence

#### Scenario: Published identity cannot be established
- **WHEN** Git inspection times out or raises an OS/process error, or GitHub returns a malformed default-branch identity, ref, or SHA
- **THEN** the check fails closed with no fallback to candidate or cached identity

#### Scenario: The default branch moves during merge authorization
- **WHEN** the live default-branch SHA differs between published-snapshot admission, merge preflight, or final authorization reread
- **THEN** authorization fails and cannot arm or preserve a merge decision based on the earlier snapshot

### Requirement: Native relationship changes are prevalidated and reverse drift is repaired
IssueOps SHALL derive a bounded transition plan from the declared release graph and current native state before any relationship mutation or zero-mutation success. The requested relation kind, orientation, issue, related issue, and operation SHALL match exactly one graph-owned missing-or-extra transition toward the declaration. For `blocked_by`, the source SHALL belong to exactly one release graph; for `sub_issue`, the source SHALL be exactly one graph's release issue. Post-mutation readback and complete graph reconciliation SHALL remain mandatory. Issue events SHALL repair invalid closed state in both blocker directions within the declared graph and SHALL validate a declared issue even when a `demilestoned` event removed its live milestone.

#### Scenario: A relationship request does not match the declared transition
- **WHEN** the requested tuple is unknown, ambiguous, reversed, graph-widening, or does not repair one exact missing-or-extra relation
- **THEN** IssueOps rejects it before any GitHub mutation and does not rely on rollback after reconciliation failure

#### Scenario: An unowned relationship appears already satisfied
- **WHEN** an add or removal addresses an existing or absent native relation but its `blocked_by` source belongs to no graph or multiple graphs, or its `sub_issue` source is not exactly one graph's release issue
- **THEN** IssueOps rejects the request before mutation instead of admitting a zero-mutation result from native state alone

#### Scenario: A blocker reopens
- **WHEN** a declared blocker becomes open while one or more graph-bounded reverse dependents are closed
- **THEN** IssueOps derives reverse direct-blocker adjacency, reopens or fails every invalid closed dependent through a bounded queue, and invalidates every exactly reread affected pull request by prioritizing protected merge-authorization revocation and auto-merge safety before attempting the implementation context, continuing across both contexts and all targets while aggregating failures

#### Scenario: An affected pull request reference or head is unsafe
- **WHEN** a closing reference names another repository despite reusing an issue number, or the pull request's exact head or complete closing references change between selection and a status or auto-merge operation
- **THEN** IssueOps rejects the foreign reference, rereads before mutation, skips or retries the raced target within bounds, and fails closed without mutating stale state

#### Scenario: Invalidation calls fail
- **WHEN** one status or auto-merge safety call fails while later exact targets or the other status context remain attemptable
- **THEN** IssueOps continues bounded invalidation attempts and reports aggregated failures; complete API failure remains fail-closed and does not claim successful remote revocation

#### Scenario: A declared issue is demilestoned
- **WHEN** an issue event removes the live milestone from an issue still owned by a release graph
- **THEN** IssueOps selects the graph from the issue map, reports targeted milestone drift, and does not skip validation because the event payload milestone is null

### Requirement: Release input is exact and complete
#492 SHALL freeze one exact RC `main` revision only after the complete twenty-nine-child hierarchy is exact; every implementation-bearing child other than #499 is closed successfully; every applicable task, owning proof, document/diagram, dependency, release note, and actionable human/automated review finding is complete; the fresh full-set Sol semantic reconciliation agrees; and #499 has exact `candidate_ready` evidence. The release workflow SHALL select its milestone gate from trusted release-version classification. For a prerelease, the gate SHALL derive #492/#499 from the active graph's release-root/campaign roles, permit exactly those two issues open, require every other milestone issue closed and complete, and bind the campaign's exact candidate revision/inventory/config/audit/hosted-run record to the release input. It SHALL NOT accept an arbitrary open-issue allowlist or represent the two governance issues' later work as complete. For stable prepublication, a distinct finalization gate SHALL derive #492 as the sole release root, require #499 exact `stable_ready` and closed, require every child closed and every mapped task except #492 task 26.6 checked, permit only #492 open and 26.6 unchecked, and bind the accepted RC plus repeated semantic, published-main, source/workspace, security, release-policy, and package-input preflight to the exact stable input. After publication, a separate final gate SHALL require exact stable hosted/Latest/downstream-pin readback, full OpenSpec/issue/review synchronization, all mapped issues closed, all mapped tasks checked, #492 closed last, and the milestone closed. Technical disposition MAY satisfy only reproducible no-change work or a genuinely non-actionable observation; it SHALL NOT convert partial accepted work into readiness.

#### Scenario: Evidence-led no-change issue
- **WHEN** a measurement or reproduction task proves existing behavior already satisfies its contract
- **THEN** the issue may close with reproducible no-product-change evidence and its required review/gates

#### Scenario: Revision or artifact changes
- **WHEN** any candidate input changes after proof
- **THEN** the complete public-surface and holistic proof restarts for the new exact input

#### Scenario: Published issue evidence drifts
- **WHEN** a mapped task source, acceptance-oriented issue body, architecture document, heading, Mermaid block, issue mirror, semantic audit answer, or default-branch identity is missing or inconsistent
- **THEN** #492 stops acceptance, returns the gap to its specification owner, and performs no feature or bug repair

### Requirement: Every installed CLI and MCP route executes
The release gate SHALL derive and reconcile the complete installed CLI command/nested-command and MCP tool inventory, including unchanged routes, and SHALL safely execute each supported route on every supported platform.

#### Scenario: Read-only route
- **WHEN** a navigation, source-evidence, format, freshness, health, status, or settings route is selected
- **THEN** the installed candidate proves root/worktree identity, output/error schema, bounds, compatibility, and actual behavior rather than help/schema presence

#### Scenario: Mutation or administration
- **WHEN** purpose, worktree, task, repair, resolve, reset, strip, or another mutating/administrative route executes
- **THEN** it uses isolated disposable fixtures, proves confirmation/refusal/cleanup, and leaves unrelated state unchanged

### Requirement: Holistic proof uses packaged installed products
One clean E2E SHALL compose binary/npm/plugin/host installation, init/database, scan, purpose-led navigation, graph/source evidence, PHP, PDF/DOCX, analysis, worktree/watcher/telemetry, parser capability, update/repair/uninstall, concurrency, cancellation, failure recovery, and compatible rollback using exact candidate artifacts.

#### Scenario: Supported installed workflow
- **WHEN** the candidate is installed into isolated homes/config/cache/repositories/databases/host roots
- **THEN** every composed boundary returns consistent runtime/plugin/skill/MCP/CLI/host identity and exact source evidence

#### Scenario: Ambient checkout or database is present
- **WHEN** developer state could satisfy a route accidentally
- **THEN** the harness proves the packaged path and isolated selected database are used or fails

### Requirement: Updating from v0.4.5 is a publication hard gate
Before RC or stable publication, the release gate SHALL install `v0.4.5`, create and exercise a real project database with durable authored and runtime state, update that same installation and database to the exact candidate on every supported platform, and prove schema/runtime/plugin/skill/MCP/CLI/host convergence. Publication SHALL fail when migration, state preservation, interrupted-update recovery, safe retry, or compatible rollback/refusal behavior is incomplete.

#### Scenario: Supported in-place update
- **WHEN** an exercised `v0.4.5` installation and database update to the exact candidate
- **THEN** project identity, authored purposes, telemetry, registered worktrees, selected roots, current generation, and source evidence remain correct without destructive reinitialization

#### Scenario: Update or migration is interrupted
- **WHEN** installer activation or database migration fails at an injected boundary
- **THEN** no partial candidate becomes active, the prior state remains usable or fails closed without corruption, repair/retry succeeds, and unrelated host or project state is unchanged

### Requirement: Confirmed defects return to owning issues
#492 SHALL classify each candidate observation. A confirmed defect SHALL return to an existing or new sanitized v0.5 IssueOps/OpenSpec owner for implementation, tests, and review; #492 SHALL NOT patch it.

#### Scenario: Candidate blocker
- **WHEN** installed or hosted proof finds a contract failure
- **THEN** publication/promotion stops, the owning fix lands on `main`, and complete proof restarts

#### Scenario: Non-defect observation
- **WHEN** evidence proves expected, unsupported-by-contract, duplicate, already-correct, or non-actionable behavior
- **THEN** it is recorded without weakening an accepted task

### Requirement: v0.5.0 begins with an independently read-back prerelease
With explicit authorization, `v0.5.0-rc1` SHALL publish as a non-draft prerelease from the exact accepted revision only after every implementation-bearing child is closed and #499's exact candidate-ready record matches publication preflight. Independent readback SHALL verify tag/revision, metadata, assets, checksums/integrity records, installers, npm, runtime/plugin/skill/MCP/CLI/host identity, and acceptance results. #499 and #492 SHALL remain open for the stable window, and v0.4.5 SHALL remain Latest.

The RC release workflow SHALL invoke the prerelease-aware milestone gate above. Stable publication SHALL invoke the distinct stable prepublication finalization gate, never the prerelease exception; after exact stable publication/readback and synchronization, final acceptance SHALL invoke the postpublication all-issues-closed/all-tasks-checked gate.

#### Scenario: Missing or mismatched release artifact
- **WHEN** any required tuple/asset/digest/version/readback is absent or inconsistent
- **THEN** RC acceptance fails and stable promotion is blocked

### Requirement: Stable promotion repeats complete proof and closes last
After an accepted candidate and explicit authorization, stable v0.5.0 SHALL require the post-accepted-RC pre-stable audit, exact stable-ready full-window reconciliation, #499 closure, and therefore every child closed before repeating the full-set Sol semantic reconciliation, objective published-main IssueOps readback, installed proof, and hosted proof. Before packaging, the stable prepublication finalization gate SHALL require #492 as the sole open milestone issue and task 26.6 as its sole unchecked mapped task; all other issue, task, role, revision, review, and proof state SHALL be exact. #492 SHALL remain open and task 26.6 SHALL remain unchecked only while the exact stable release publishes, independently reads back, becomes Latest, reconciles downstream pins, and synchronizes final OpenSpec/issue/review state. Primary Sol SHALL then check 26.6, close #492 last, and close the milestone. A postpublication final gate SHALL reread the exact stable tag, release, assets, checksums, runtime/plugin/MCP/CLI/host identity, Latest selection, downstream pins, issue/task/review/OpenSpec state, #492 closure, and milestone closure; any missing, stale, partial, raced, or mismatched state SHALL fail closed.

#### Scenario: Stable publication begins from exact finalization state
- **WHEN** #499 is closed from exact stable-ready evidence, no child blocker remains, all tasks except #492 task 26.6 are checked, #492 is the sole open issue, and the exact stable input passes the repeated prepublication proof
- **THEN** stable packaging and publication may proceed without treating 26.6 or #492 as complete

#### Scenario: Stable hosted state is synchronized and closed
- **WHEN** exact readback proves v0.5.0 is the intended stable release and Latest, downstream pins agree, and final OpenSpec/issue/review state is synchronized
- **THEN** primary Sol checks task 26.6, closes #492 last, closes the milestone, and the postpublication gate succeeds only after rereading all mapped tasks and issues closed in that exact hosted state

#### Scenario: Stable finalization or postpublication readback drifts
- **WHEN** an extra issue/task remains, a required closure happens early, or any release, Latest, pin, synchronization, root, task, review, OpenSpec, or milestone fact is missing, stale, partial, raced, or mismatched
- **THEN** publication or final acceptance fails at the applicable gate without reusing the RC exception or claiming complete closure
