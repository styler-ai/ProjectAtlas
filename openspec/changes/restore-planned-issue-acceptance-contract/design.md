## Context

ProjectAtlas currently has two issue-writing authorities that disagree. The bug, improvement, and chore forms require behavior-level acceptance criteria, and the bug/improvement forms also retain reproduction or affected-workflow context. The planned-issue checker introduced the concise #305 shape without `Acceptance Criteria`; it verifies headings, task mirroring, diagrams, pre-mortem mappings, and release relationships, but does not and cannot establish that the prose communicates product intent.

The audit of the twenty-six live v0.5.0 issue bodies plus candidate drafts #497, #498, and #499 found fourteen strong, eleven adequate, and four needing explanatory enrichment. Every audited packet lacked the canonical acceptance section, although the latest #497/#498/#499 drafts had already added ignored acceptance sections. #308 is the semantic benchmark because it carries actor, current state, consequence, intended outcome, ownership, compatibility, and causal delivery order continuously from prose through tasks; its length is not a target.

Sol owns specification authorship, issue prose, migration, semantic acceptance, and GitHub state. Primary Sol also owns the personal global `issue-spec-writing` skill and has already completed its cross-project semantic contract. Luna owns only the later repository checker, self-test, issue forms, repository guidance, and any version-matched ProjectAtlas plugin files proven necessary by an ownership review; Luna may verify compatibility with the global skill but never mutates it and owns no semantic judgment. The current lane writes only OpenSpec, issue-map, release-planning, architecture, and ignored issue-body drafts; it does not implement checker, workflow, Rust, or database behavior and does not mutate GitHub.

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

### Candidate graph data is not live authority

`openspec/issue-map.json.changes` continues to map #497-#500 to their candidate OpenSpec tasks because the repository schema requires every immediate `tasks.md` change to have one owning issue. That mapping declares task ownership only; it does not assign milestone membership, hierarchy, blockers, or release readiness. The planning pull request therefore leaves authoritative `release_graphs.v0.5.0-00` at the current hosted twenty-five children. The change-local `candidate-issue-map.json` is the single deterministic promotion source for #499's future campaign declaration and the complete twenty-nine-child replacement graph. The live IssueOps checker does not read that candidate artifact, and candidate validation cannot authorize readiness or relationships.

After the planning artifacts are accepted on `main`, Sol publishes and reads back all thirty exact bodies, then performs the authorized milestone and native relationship bootstrap from the accepted candidate manifest. During that bounded hosted transition the old active graph is expected to fail closed; no handoff, merge authorization, or release transition may proceed. Only after hosted state exactly matches the candidate does a narrow publication replace `openspec/issue-map.json.release_graphs.v0.5.0-00` with the manifest's #499 campaign declaration and v0.5 graph; exact published-`main` and hosted readback then remove the fail-closed condition. The manifest is removed as part of promotion so it never becomes a second active graph.

Publishing the future graph first was rejected because the existing checker correctly treats every active declaration as immediately authoritative. Weakening or teaching the live reconciler to overlook candidate nodes was rejected because it would make real graph drift indistinguishable from staging.

### Release acceptance repeats semantic reconciliation over the full set

#492 candidate and stable gates perform a fresh Sol reconciliation over every accepted issue's explanation, behavior/capability, acceptance, release role, non-goals/failures, diagram meaning, and task mirror. IssueOps independently proves objective structure and synchronization. Candidate acceptance still requires #499 `candidate_ready`; stable acceptance still requires #499 `stable_ready` and closure. The full hierarchy contains twenty-nine children after #500 is added.

### Guidance ownership stays at the narrow owning surface

The primary Sol-owned personal global `issue-spec-writing` skill now carries the cross-project semantic definitions: `Why` actor/current trigger/consequence/outcome, `What Changes` observable before-to-after behavior plus the smallest owner, conditional reproduced-bug facts versus truthful proof-gap hardening, the canonical acceptance order, and all eight Sol audit questions without a score, LLM gate, or ledger. This completed global source is read-only to Luna. Repository `AGENTS.md`, issue forms, and checker-facing guidance own ProjectAtlas's exact structural rules. If version-matched ProjectAtlas agent guidance must mention the workflow, Luna updates only `plugins/projectatlas/skills/projectatlas/SKILL.md` and its generated/package mirrors; repository-root guidance is not duplicated into another plugin framework or model-specific rule.

## Risks / Trade-offs

- [A mechanically valid issue remains semantically weak] -> Sol must answer the eight questions before each readiness, handoff, and release transition; IssueOps never claims comprehension.
- [Migration changes task text or checked state] -> Generate each draft from the exact live body or latest candidate draft, change only required explanation/acceptance/graph facts, and compare the final task slice byte-for-byte with its authoritative local OpenSpec owner.
- [Acceptance becomes another implementation checklist] -> Enforce ordinary bullets only and reject checkboxes, task IDs, and receipts.
- [Type-specific context makes planned bodies sprawl] -> Consolidate only applicable sanitized facts into `Why` and `What Changes`; do not retain empty raw fields.
- [Repository implementation overwrites the completed personal global semantic contract] -> Keep primary Sol as the only global skill mutation owner; Luna reads it for compatibility and changes only repository or proven necessary version-matched plugin surfaces.
- [#500 serializes the release] -> Keep `blocked_by: []`, add no #500-to-product edges, and use the existing per-issue readiness gate for the shared contract.
- [#500 completion depends on the shared readiness task that itself waits for #500] -> Keep #500 publication/readback and final architecture review inside #500, complete #500, and only then let primary Sol synchronize the independent shared task.
- [Candidate release-graph nodes activate live relationship checks before their native state exists] -> Keep task ownership mapped, stage only the future graph and campaign declaration in the one change-local promotion manifest, preserve the active release graph as exact hosted truth, and promote only after body and hosted graph readback match.
- [Release acceptance trusts an earlier semantic pass] -> Require a fresh full-set Sol reconciliation at candidate and stable readback in addition to objective IssueOps.

## Migration Plan

1. Publish this OpenSpec change, the non-authoritative candidate release-graph promotion manifest, the release-readiness semantic contract, the focused architecture views, and exact ignored drafts through a non-closing planning pull request while task ownership remains mapped and the authoritative release graph remains aligned with current live GitHub state; retain the completed primary Sol-owned personal global skill as read-only authority and keep the independent shared task unchecked.
2. Luna implements the smallest objective repository checker/self-test and necessary repository issue-form, repository-guidance, and conditionally required version-matched plugin integration; it verifies but does not mutate the personal global skill, and existing gates remain authoritative.
3. Sol semantically reconciles and publishes the exact thirty drafts (#500 plus twenty-nine migrated packets), applies and reads back the authorized milestone and native parent/blocker bootstrap from the accepted manifest, promotes that exact future graph and campaign declaration into the authoritative issue map through a narrow publication, and then rereads every body, task mirror, diagram, task mapping, release graph, milestone, and native relation from exact `main` and GitHub.
4. Review the final Luna implementation against the focused diagrams and update either side until they agree, completing #500's final task only after that review passes.
5. After every #500 task is complete and #500 itself completes, primary Sol independently synchronizes shared `complete-v050-release-readiness` task 1.4; no #500 task checks or depends on that shared task.

Any partial or raced body, hosted, or manifest-promotion mutation fails closed. The ignored drafts and candidate manifest remain the repair sources until authoritative map and hosted readback agree; no issue becomes ready and no implementation handoff proceeds from partial migration state.

## Open Questions

None. The structural, semantic, ownership, migration, and release-graph decisions are settled by issue #500.
