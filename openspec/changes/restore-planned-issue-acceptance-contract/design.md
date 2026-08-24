## Context

ProjectAtlas currently has two issue-writing authorities that disagree. The bug, improvement, and chore forms require behavior-level acceptance criteria, and the bug/improvement forms also retain reproduction or affected-workflow context. The planned-issue checker introduced the concise #305 shape without `Acceptance Criteria`; it verifies headings, task mirroring, diagrams, pre-mortem mappings, and release relationships, but does not and cannot establish that the prose communicates product intent.

The audit of the twenty-six live v0.5.0 issue bodies plus candidate drafts #497, #498, and #499 found fourteen strong, eleven adequate, and four needing explanatory enrichment. Every audited packet lacked the canonical acceptance section, although the latest #497/#498/#499 drafts had already added ignored acceptance sections. #308 is the semantic benchmark because it carries actor, current state, consequence, intended outcome, ownership, compatibility, and causal delivery order continuously from prose through tasks; its length is not a target.

Sol owns specification authorship, issue prose, migration, semantic acceptance, and GitHub state. Primary Sol also owns the personal global `issue-spec-writing` skill and has already completed its cross-project semantic contract. Luna owns only the later repository checker, self-test, issue forms, repository guidance, and any version-matched ProjectAtlas plugin files proven necessary by an ownership review; Luna may verify compatibility with the global skill but never mutates it and owns no semantic judgment. The current lane writes only OpenSpec, issue-map, release-planning, architecture, a tracked exact-body publication manifest, and ignored non-authoritative issue-body convenience copies; it does not implement checker, workflow, Rust, or database behavior and does not mutate GitHub.

## Goals / Non-Goals

**Goals:**

- Restore one lean canonical planned-issue contract with observable acceptance between release scope and non-goals.
- Preserve enough applicable bug or improvement intake context for another contributor to understand and verify the issue without private discovery context.
- Mechanize only objective structure while assigning comprehension and reconciliation explicitly to Sol.
- Migrate all twenty-nine accepted/current-candidate v0.5.0 packets, map their OpenSpec task ownership, and stage #500 as the twenty-ninth non-release child without activating unpublished release-graph nodes or inventing product dependency edges.
- Make per-issue readiness, implementation handoff, RC acceptance, and stable acceptance depend on semantically reconciled published issue evidence.

**Non-Goals:**

- Word-count floors, required vocabulary, keyword or jargon rules, semantic scores, an LLM CI gate, evidence receipts, or another checklist.
- Requiring every raw issue-form field when it does not apply, or fabricating reproduction evidence for proof-gap hardening such as #390.
- Making #500 a native blocker of every v0.5.0 product issue or serializing otherwise independent work.
- Rust product, CLI, MCP, runtime, schema, migration, query, transaction, or SQLite changes.

## Decisions

### One canonical planned order restores the missing behavior contract

The planned order is `Why`, `What Changes`, `Capabilities`, `Architecture Diagrams`, `Release Scope`, `Acceptance Criteria`, `Non-Goals`, `Pre-Mortem`, then exactly one `OpenSpec Tasks` or `OpenSpec Task Checklist` section. `Acceptance Criteria` contains two to five non-checkbox top-level unordered bullets. The bullets state observable positive behavior, material negative or fail-closed behavior, compatibility or unchanged behavior, and a legitimate no-change outcome where the issue permits one. They contain no OpenSpec task IDs, per-body task identity, or proof receipt.

The alternative of placing acceptance inside OpenSpec tasks was rejected because tasks describe work and ownership, not the truths that make the outcome acceptable. A second checklist was rejected because the exact OpenSpec mirror already owns implementation progress.

### Applicable intake context is consolidated, not erased or copied mechanically

A planned bug retains enough sanitized trigger/environment/current/expected behavior to reproduce the failure, or explicitly says that it hardens a missing proof boundary without claiming a reproduced product failure. A planned improvement names the affected actor or surface and explains the before-to-after agent workflow. This context belongs in concise `Why` and `What Changes` prose; raw issue-form fields are inputs, not permanent extra planned sections.

Requiring every raw form field was rejected because it would add empty ceremony to maintenance and no-change work. Dropping type-specific context was rejected because a syntactically complete body can still be impossible to reproduce or estimate.

### IssueOps checks only objective acceptance structure

The later checker adds `acceptance criteria` to the visible required heading sequence, requires exactly one non-empty section, counts two to five top-level ordinary unordered bullets, and rejects task checkbox syntax or OpenSpec task-ID references in that section. Existing task-mirror, diagram, pre-mortem, graph, publication, and readiness behavior remains unchanged. Self-tests cover valid boundaries, missing/duplicate/misordered/empty sections, one/six bullets, nested or prose-only content, checkboxes, task IDs, hidden Markdown, and compatibility with all existing gates.

Word counts, semantic keywords, jargon lists, and model scoring were rejected because they would reward performative prose and still fail to establish comprehension. IssueOps reports structure; it never reports that the issue is well explained.

### Sol owns eight semantic audit questions

Before readiness, handoff, RC acceptance, and stable acceptance, Sol answers these eight questions from the issue body, OpenSpec, diagrams, and live release graph:

1. Does `Why` identify the affected actor, current state or trigger, consequence, and intended outcome?
2. Does `What Changes` describe the observable before-to-after behavior and the smallest owning boundary?
3. Do the named capabilities match the actual behavior and OpenSpec ownership without overstating support?
4. Does `Release Scope` explain the real release role, direct prerequisites, unlocked work, and legitimate parallelism?
5. Do two to five acceptance bullets cover observable success, important refusal/failure, material compatibility, and any legitimate no-change outcome without becoming tasks?
6. Do `Non-Goals` and `Pre-Mortem` bound scope and credible failures, with mitigation check states and task references synchronized to the one OpenSpec checklist?
7. Does each architecture link and its surrounding prose say what boundary the diagram proves, and does the rendered direction, ownership, failure termination, and density match the intended workflow?
8. Does the single task section exactly mirror the complete owning OpenSpec slice in text, order, ownership, and checked state, with no implementation gap hidden in prose?

The audit result is a Sol judgment recorded by the normal readiness/acceptance transition, not a score, ledger, generated receipt, or second review queue.

### Readiness, not fake graph edges, orders the shared contract

No implementation handoff occurs until that issue's body, OpenSpec, diagram meaning, acceptance criteria, and native release facts are reconciled and read back from exact published `main`. #500 has `blocked_by: []`; #492 is directly blocked by #500 like every child. #500 does not become a blocker of all product issues. The existing published-readiness gate owns the shared ordering.

#497 may proceed only after its own packet passes the restored contract and the shared checker/order mechanism required by #500 is accepted. This is an IssueOps readiness prerequisite, not a native product dependency, so #497 remains dependency-free in the v0.5 graph.

### Tracked body bytes are the sole publication source

The change-local `candidate-issue-bodies.json` is a one-file, fully reviewable publication input. Schema version 1 declares SHA-256 and UTF-8 with LF line endings plus one trailing LF, then stores exactly the thirty unique issue numbers and each complete Markdown body as `body_lines` with the digest of the bytes reconstructed by joining those lines with LF. The ignored `.tmp` bodies are generated convenience copies only and must match those reconstructed bytes exactly; they cannot authorize publication, repair, or review by themselves.

After independent Sol and hosted Codex review accept an exact planning-PR head, primary Sol validates the manifest schema, algorithm, normalization, exact issue set, uniqueness, complete content, and every digest before mutation. Publication writes only the exact body bytes reconstructed from the manifest. Immediate live readback normalizes the same UTF-8/LF boundary and must reproduce every byte and digest; a malformed manifest, wrong set, hash mismatch, concurrent body change, partial write, or stale readback fails closed. This is a temporary deterministic transport source, not a permanent body store or evidence ledger. It remains tracked through exact body and planning-`main` readback and is removed with the candidate graph manifest only at the later atomic graph-promotion cleanup after hosted relationship bootstrap also agrees.

### Candidate graph data is not live authority

`openspec/issue-map.json.changes` continues to map #497-#500 to their candidate OpenSpec tasks because the repository schema requires every immediate `tasks.md` change to have one owning issue. That mapping declares task ownership only; it does not assign milestone membership, hierarchy, blockers, or release readiness. The planning pull request therefore leaves authoritative `release_graphs.v0.5.0-00` at the current hosted twenty-five children. The separate change-local `candidate-issue-map.json` is the single deterministic promotion source for #499's future campaign declaration and the complete twenty-nine-child replacement graph. The live IssueOps checker does not read that candidate artifact, and candidate validation cannot authorize readiness or relationships.

The executable bootstrap starts from a corrected exact planning-PR head accepted by an independent Sol review and a new hosted Codex review for that head, including every public body byte in `candidate-issue-bodies.json`. While the reviewed PR remains open and authoritative `release_graphs.v0.5.0-00` still matches the hosted twenty-five-child graph, primary Sol validates that body manifest, publishes only the exact body bytes reconstructed from it, and reads back the same normalized bytes and hashes. Those bodies intentionally link to architecture that is not yet on `main`; that temporary body-to-`main` gap exists only to let normal unfiltered IssueOps/CI validate the PR and authorizes no readiness, milestone, relationship, handoff, merge authorization for implementation, or release transition. After normal unfiltered IssueOps/CI is green, primary Sol first uses the normal trusted-default-branch repository-dispatch authorizer and accepts only its exact merged-main readback.

One non-reusable exception exists only for the proven self-hosting repair in this planning PR, where repository dispatch necessarily runs the old default-branch authorizer that the PR itself corrects. Before that exception, primary Sol seals one immutable preflight: accepted reviewed head; base branch and exact current `main`; expected squash-result tree; complete branch-protection state; exact required status-context-to-GitHub-Actions-app bindings; all ordinary required proof green; no unresolved review finding; and failed `issueops-merge-authorized` as the sole authorization blocker, with its failure traced to the old authorizer defect. Primary Sol then temporarily removes only administrator enforcement and performs an admin squash guarded by the pinned head. A mandatory finally path restores administrator enforcement regardless of command outcome. Required checks remain configured with the same app bindings throughout; no check is removed, rebound, waived by a forged status, or replaced with another normal route.

Downstream work remains blocked unless the merge command succeeds and readback proves the complete protection state equals the preflight with administrator enforcement active, the squash commit's sole parent is the pinned base/`main`, its tree is the pinned expected result tree, and `main` points to that exact squash commit. Primary Sol must then dispatch the repaired trusted-default-branch authorizer for the already merged PR and receive the exact idempotent `already-satisfied` outcome without protection or tree drift. Any missing precondition, command failure, restoration failure, or protection/parent/tree/`main`/dispatch mismatch terminates at restoration and recovery; it never falls through to Luna or hosted graph activation. This exception cannot be reused for another PR or ordinary merge.

After exact planning-main plus repaired-authorizer readback, independent shared `complete-v050-release-readiness` task 1.4 verifies the checker-intended classification across all candidate changes. #497, the shared #498/#499 change, and #500 each contain at least one accepted checked section 1 contract/specification task; impact data, implementation, tests, campaign/GitHub synchronization, and final implementation-versus-diagram reviews live in section 2 or later and remain unchecked until substantively complete and accepted. #500 publication/bootstrap task 2.1 remains unchecked while the shared task accepts and reads back its handoff; its later-section state does not block activation, and its tasks 2.2-2.3 remain unchecked for post-activation delivery. Exact issue-map owner slices, truthful task states, and published body task mirrors must agree. This order is deliberate: `planned_issue_failures()` applies `openspec_readiness_failures()` to every open milestoned issue and treats every `1.*` task as pre-milestone contract/specification work, while later-numbered delivery tasks may remain unchecked without blocking readiness.

Only after that exact published-main classification readback does shared task 1.4 apply and read back #497-#500's v0.5 milestone/status plus the full twenty-nine-child native parent/blocker bootstrap from accepted `candidate-issue-map.json`. Partial hosted state remains fail-closed. After exact body, planning-`main`, task classification, and hosted relationship readback agree, a separate narrow graph-promotion PR atomically replaces only `openspec/issue-map.json.release_graphs.v0.5.0-00` with that graph manifest's #499 campaign declaration and v0.5 graph and removes both candidate manifests in the same change. Its exact published-`main`, live IssueOps, hosted ownership, task, dependency, and semantic readback must pass before synchronizing the shared task or handing any delivery to Luna. Neither manifest becomes a second active authority, and neither Luna nor a candidate checkout owns any GitHub/specification/global-skill mutation.

After promoted-state readback accepts #500's task 2.1 handoff, that task may be checked through normal truthful synchronization; tasks 2.2-2.3 still own objective checker/forms/guidance implementation and final implementation-versus-diagram review. #497, #498, and #499 likewise retain their incomplete owner-specific delivery tasks and real graph blockers. Each issue closes only after normal implementation validation proves its delivery complete; activation and shared-task synchronization never claim incomplete work complete.

Publishing the future graph first was rejected because the existing checker correctly treats every active declaration as immediately authoritative. Weakening or teaching the live reconciler to overlook candidate nodes was rejected because it would make real graph drift indistinguishable from staging.

### Release acceptance repeats semantic reconciliation over the full set

#492 candidate and stable gates perform a fresh Sol reconciliation over every accepted issue's explanation, behavior/capability, acceptance, release role, non-goals/failures, diagram meaning, and task mirror. IssueOps independently proves objective structure and synchronization. Candidate acceptance still requires #499 `candidate_ready`; stable acceptance still requires #499 `stable_ready` and closure. The full hierarchy contains twenty-nine children after #500 is added.

### Guidance ownership stays at the narrow owning surface

The primary Sol-owned personal global `issue-spec-writing` skill now carries the cross-project semantic definitions: `Why` actor/current trigger/consequence/outcome, `What Changes` observable before-to-after behavior plus the smallest owner, conditional reproduced-bug facts versus truthful proof-gap hardening, the canonical acceptance order, and all eight Sol audit questions without a score, LLM gate, or ledger. This completed global source is read-only to Luna. Repository `AGENTS.md`, issue forms, and checker-facing guidance own ProjectAtlas's exact structural rules. If version-matched ProjectAtlas agent guidance must mention the workflow, Luna updates only `plugins/projectatlas/skills/projectatlas/SKILL.md` and its generated/package mirrors; repository-root guidance is not duplicated into another plugin framework or model-specific rule.

## Dependencies / Cross-Issue Impact

#500 has no product blocker and adds no blocker edge to #497 or another implementation issue. Its checked section 1 specification contract, like the checked section 1 contracts for #497 and #498/#499, must be exact on published `main` before independent shared `complete-v050-release-readiness` task 1.4 activates all four issues and promotes/reads the graph. Their incomplete section 2-or-later delivery remains unchecked and executable afterward under the real blocker topology: #497 unlocks #498, while #482 and #498 unlock #499. #492 remains the sole release root and closes last.

## Risks / Trade-offs

- [A mechanically valid issue remains semantically weak] -> Sol must answer the eight questions before each readiness, handoff, and release transition; IssueOps never claims comprehension.
- [Migration changes task text or checked state] -> Generate each candidate body entry from the exact live body or latest candidate draft, change only required explanation/acceptance/graph facts, and compare the final task slice byte-for-byte with its authoritative local OpenSpec owner.
- [Ignored convenience drafts drift from the reviewed publication input] -> Treat only the tracked body manifest as authority, validate every stored digest and exact issue number, and require each ignored `.tmp` copy to match the body bytes reconstructed from its manifest entry.
- [Acceptance becomes another implementation checklist] -> Enforce ordinary bullets only and reject checkboxes, task IDs, and receipts.
- [Type-specific context makes planned bodies sprawl] -> Consolidate only applicable sanitized facts into `Why` and `What Changes`; do not retain empty raw fields.
- [Repository implementation overwrites the completed personal global semantic contract] -> Keep primary Sol as the only global skill mutation owner; Luna reads it for compatibility and changes only repository or proven necessary version-matched plugin surfaces.
- [#500 serializes the release] -> Keep `blocked_by: []`, add no #500-to-product edges, and use the existing per-issue readiness gate for the shared contract.
- [A candidate implementation or final-review task remains numbered `1.*`, so planned-issue readiness rejects milestone activation] -> Keep section 1 limited to accepted checked specification work for #497, #498/#499, and #500; move all delivery to section 2 or later, verify exact owner slices/body mirrors and truthful task states, and activate only after unchanged checker probes admit every open issue with incomplete delivery still unchecked.
- [Candidate release-graph nodes activate live relationship checks before their native state exists] -> Keep task ownership mapped, stage only the future graph and campaign declaration in the separate graph-promotion manifest, preserve the active release graph as exact hosted truth, and promote only after body and hosted graph readback match.
- [The planning PR waits for body publication while body publication waits for the planning merge] -> Require exact-head independent Sol and hosted Codex review of the tracked body manifest first, publish/read back only its exact bytes while the PR is still open, and keep the temporary body-to-main link gap fail-closed for readiness until the green PR merges and exact main is read back.
- [The trusted default-branch authorizer cannot consume its own repair while administrator enforcement correctly blocks an unchanged-protection admin merge] -> Keep repository dispatch as the sole normal route and admit only this one proven self-repair after the immutable head/base/main/tree/protection/check-app preflight; remove only administrator enforcement, restore it in a mandatory finally path, and require exact protection/parent/tree/main plus repaired-main idempotent-dispatch readback before downstream work.
- [Release acceptance trusts an earlier semantic pass] -> Require a fresh full-set Sol reconciliation at candidate and stable readback in addition to objective IssueOps.

## Migration Plan

1. Publish this OpenSpec change, the tracked exact-body publication manifest, the separate non-authoritative candidate release-graph promotion manifest, the release-readiness semantic contract, and focused architecture views to the non-closing planning PR while task ownership remains mapped and the authoritative release graph remains aligned with current live GitHub state; verify the ignored convenience bodies exactly match the tracked body manifest, retain the completed primary Sol-owned personal global skill as read-only authority, and keep the independent shared task unchecked.
2. Obtain independent Sol review and a new hosted Codex review for the corrected exact PR head. No body or graph mutation starts from an unreviewed head.
3. While that reviewed PR remains open and the authoritative/live graph remains at twenty-five children, primary Sol validates the tracked body manifest and publishes only the exact body bytes reconstructed from it, then reads back every normalized body and digest. The temporary body-to-main architecture-link gap remains fail-closed for readiness.
4. Rerun normal unfiltered IssueOps/CI and require the planning PR to be green. Attempt the trusted-default-branch repository-dispatch merge first. Only for this one proven authorizer self-repair, pin the reviewed immutable head, base/`main`, expected result tree, complete protection state, required check/app bindings, and sole authorization blocker; temporarily remove only administrator enforcement, admin-squash only that head, restore enforcement in a mandatory finally path, and require exact protection/parent/tree/`main` plus repaired-main idempotent-dispatch readback. Any precondition, merge, restoration, or readback failure stops in recovery, with required checks never removed, rebound, or forged.
5. From exact planning `main`, verify that #497, #498/#499, and #500 each expose only checked specification work in section 1, every delivery task is in section 2 or later, every incomplete or not-yet-accepted delivery task remains unchecked, and every issue-map owner slice/body task mirror agrees. #500 task 2.1 remains unchecked while shared task 1.4 accepts its planning-main/repaired-authorizer handoff; tasks 2.2-2.3 and every other incomplete delivery task likewise remain unchecked. Any misclassified, unchecked section 1, or falsely checked delivery task blocks activation.
6. Independent shared `complete-v050-release-readiness` task 1.4 assigns #497-#500 their v0.5 milestone/status and applies/reads the complete twenty-nine-child native parent/blocker bootstrap from the accepted graph manifest. Any partial, raced, prematurely activated, falsely checked, or mismatched hosted state remains fail-closed.
7. After exact body, planning-main, task-classification, and hosted relationship readback agree, use a separate narrow PR to atomically replace only active `release_graphs.v0.5.0-00` with the exact future graph and #499 campaign declaration and remove both candidate manifests; require exact merged-`main`, live IssueOps, hosted ownership, dependency, and fresh semantic readback before checking the independent shared task.
8. Hand off each issue only when its promoted graph blockers permit. #500's Luna repository implementation and final diagram review then complete normally after its preactivation publication/bootstrap task; #497, #498, and #499 likewise retain and complete their own unchecked delivery tasks before closure.

Any unreviewed-head, malformed, partial, raced, stale, or mismatched body, PR, implementation, hosted, or manifest-promotion state fails closed. The tracked body and graph manifests remain the deterministic repair sources until exact bodies, planning `main`, authoritative map, and hosted readback agree; ignored `.tmp` copies never become authority. No issue becomes ready and no implementation handoff proceeds from the temporary pre-merge body/link gap or any partial migration state.

## Open Questions

None.
