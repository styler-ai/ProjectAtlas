## Context

ProjectAtlas currently has two issue-writing authorities that disagree. The bug, improvement, and chore forms require behavior-level acceptance criteria, and the bug/improvement forms also retain reproduction or affected-workflow context. The planned-issue checker introduced the concise #305 shape without `Acceptance Criteria`; it verifies headings, task mirroring, diagrams, pre-mortem mappings, and release relationships, but does not and cannot establish that the prose communicates product intent.

The audit of the twenty-six live v0.5.0 issue bodies plus candidate drafts #497, #498, and #499 found fourteen strong, eleven adequate, and four needing explanatory enrichment. Every audited packet lacked the canonical acceptance section, although the latest #497/#498/#499 drafts had already added ignored acceptance sections. #308 is the semantic benchmark because it carries actor, current state, consequence, intended outcome, ownership, compatibility, and causal delivery order continuously from prose through tasks; its length is not a target.

Sol owns specification authorship, issue prose, migration, semantic acceptance, and GitHub state. Luna owns the later objective checker, self-test, form and guidance integration, and no semantic judgment. The current lane writes only OpenSpec, issue-map, release-planning, architecture, and ignored issue-body drafts; it does not implement checker, workflow, Rust, or database behavior and does not mutate GitHub.

## Goals / Non-Goals

**Goals:**

- Restore one lean canonical planned-issue contract with observable acceptance between release scope and non-goals.
- Preserve enough applicable bug or improvement intake context for another contributor to understand and verify the issue without private discovery context.
- Mechanize only objective structure while assigning comprehension and reconciliation explicitly to Sol.
- Migrate all twenty-nine accepted/current-candidate v0.5.0 packets and add #500 as the twenty-ninth non-release child without inventing product dependency edges.
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

### Release acceptance repeats semantic reconciliation over the full set

#492 candidate and stable gates perform a fresh Sol reconciliation over every accepted issue's explanation, behavior/capability, acceptance, release role, non-goals/failures, diagram meaning, and task mirror. IssueOps independently proves objective structure and synchronization. Candidate acceptance still requires #499 `candidate_ready`; stable acceptance still requires #499 `stable_ready` and closure. The full hierarchy contains twenty-nine children after #500 is added.

### Guidance ownership stays at the narrow owning surface

The global `issue-spec-writing` skill owns the cross-project semantic definitions: `Why` actor/current/consequence/outcome, `What Changes` observable behavior plus ownership, truthful release role, diagram-boundary prose, and the eight-question Sol audit. Repository `AGENTS.md`, issue forms, and checker-facing guidance own ProjectAtlas's exact planned order and objective structural rules. If version-matched ProjectAtlas agent guidance must mention the workflow, Luna updates only `plugins/projectatlas/skills/projectatlas/SKILL.md` and its generated/package mirrors; repository-root guidance is not duplicated into another plugin framework or model-specific rule.

## Risks / Trade-offs

- [A mechanically valid issue remains semantically weak] -> Sol must answer the eight questions before each readiness, handoff, and release transition; IssueOps never claims comprehension.
- [Migration changes task text or checked state] -> Generate each draft from the exact live body or latest candidate draft, change only required explanation/acceptance/graph facts, and compare the final task slice byte-for-byte with its authoritative local OpenSpec owner.
- [Acceptance becomes another implementation checklist] -> Enforce ordinary bullets only and reject checkboxes, task IDs, and receipts.
- [Type-specific context makes planned bodies sprawl] -> Consolidate only applicable sanitized facts into `Why` and `What Changes`; do not retain empty raw fields.
- [#500 serializes the release] -> Keep `blocked_by: []`, add no #500-to-product edges, and use the existing per-issue readiness gate for the shared contract.
- [Release acceptance trusts an earlier semantic pass] -> Require a fresh full-set Sol reconciliation at candidate and stable readback in addition to objective IssueOps.

## Migration Plan

1. Publish this OpenSpec change, the twenty-nine-child map, the release-readiness semantic contract, the two architecture views, and exact ignored drafts through a non-closing planning pull request; keep shared task 1.4 unchecked.
2. Luna implements the smallest objective checker/self-test and necessary form, repository guidance, global skill source, and version-matched plugin guidance integration; existing gates remain authoritative.
3. Sol semantically reconciles and publishes the exact thirty drafts (#500 plus twenty-nine migrated packets), assigns #500 to `v0.5.0-00`, activates #492 parent/blocker truth, and reads every body, task mirror, diagram, milestone, and native relation back from exact `main`.
4. Only after the full migration, structural gate, Sol audit, exact published readback, and #500 completion succeed may shared `complete-v050-release-readiness` task 1.4 be checked. Then perform the final architecture review.

Any partial or raced hosted mutation fails closed. The ignored drafts remain the repair source until exact readback agrees; no issue becomes ready and no implementation handoff proceeds from partial migration state.

## Open Questions

None. The structural, semantic, ownership, migration, and release-graph decisions are settled by issue #500.
