# Design

## Context

See [proposal.md](proposal.md) for intent and release scope. #440 introduced classified documentation relations and #461 established canonical file anchoring, retained heading occurrences and explicit no-candidate coverage. Current source already contains a `documented_by` inbound view and query-time purpose projection. #751 verifies their real specification/source integration; it does not start from an assumed parser or storage defect.

The planning baseline is accepted `main` at `84a00144bc1f35ab2c6ff4770181ed0a0983d774`. Implementation must rebase onto the accepted RC1 release baseline under task 1.2 and recheck affected facts. A spot check against an indexing specification returned a truncated page with an unresolved reference and unavailable purpose; that is insufficient evidence of absence or a product bug.

## Goals / Non-Goals

**Goals:** expose the actual declared specifications and their exact implementation targets through a usable bounded agent recipe; project each endpoint's accepted purpose correctly; distinguish authoring gaps, unsupported references, stale projections and defects; preserve root, generation, compatibility and failure contracts.

**Non-goals:** semantic inference, automatic proof of conformance, a new graph engine, new schema, duplicate inverse storage, a mandatory new tool or direction enum, or unrelated agent-workflow redesign. General post-compaction and code-reflection guidance is a separate release/skill concern.

## Existing ownership and architecture

Reuse the existing views:

- [Canonical documentation relation projection](../../../docs/projectatlas-3-architecture.md#canonical-documentation-relation-projection).
- [Classified documentation and worktree navigation](../../../docs/projectatlas-3-architecture.md#classified-documentation-and-worktree-navigation).

No new runtime boundary is proposed, so these existing canonical views remain the architecture authority. Add or change a diagram only if a confirmed implementation fix changes that ownership or flow, and then render and visually review it.

| Responsibility | Existing owner to inspect | Required boundary |
| --- | --- | --- |
| Explicit Markdown destinations, headings and spans | `crates/projectatlas-symbols/src/markdown.rs` | Bounded parser-proven facts; no prose or name inference. |
| Target normalization, admission, resolution and affected publication | `crates/projectatlas-cli/src/runtime/graph_projection.rs` | Exact indexed identity, existing root/policy checks and atomic complete publication. |
| Canonical graph identities, adjacency, occurrences and coverage | `crates/projectatlas-db/src/repository_graph.rs` and the existing storage API | One `documents` fact; indexed outbound/inbound access; no stored inverse. |
| Detailed relation composition and purpose projection | `crates/projectatlas-service/src/relations.rs` | Typed inbound view, bounded traversal and batched authoritative endpoint purpose reads. |
| CLI/MCP rendering and continuation schema | Existing relation adapters and registries | Semantic JSON/TOON/MCP parity and exact root/generation/purpose binding. |
| Actual specifications and approved responsibilities | Repository OpenSpec files, accepted issue ownership and purpose APIs | Explicit reviewed association; document links do not prove semantic conformance. |
| Shipped graph-navigation guidance | `plugins/projectatlas/skills/projectatlas/SKILL.md` and its `references/short-cli.md` | Teach the bounded dependency/specification recipe and its purpose/coverage limits; do not edit an installed cache as a shipping change. |
| Installed/worktree acceptance | Existing CLI E2E owners, including `crates/projectatlas-cli/tests/e2e_worktrees.rs` | Real commands and MCP calls against isolated fixtures and actual repository pairs. |

These paths identify responsibilities, not a prohibition on fixing another confirmed manifestation at its shared owner. Trace current callers and registries before selecting edits.

## Decisions

### 1. Verify author intent before changing graph machinery

Inventory representative actual specification/source pairs from the repository's declared specification context and OpenSpec artifacts. Read the requirements and owning implementation responsibility to approve the pairing. A file existing at a referenced path is necessary but does not establish that it is the implementation described by the requirement.

Use supported explicit repository-local Markdown links or code references already accepted by the extractor. This change's delta spec includes ordinary relative links to the existing owners as an inspectable example. If a real specification lacks an authoritative reference, repair the actual document; do not insert a correct link only into a test fixture and claim the repository is integrated.

Keep multiple applicable specifications. A reference to historical or proposed work remains a reference, not proof of current normative authority. Use the repository's declared lifecycle context; do not manufacture status from a filename, modification time or purpose similarity. Follow the source and specification slices for semantic review.

This requires no new lifecycle classification field. Existing paths, occurrence evidence and the repository's declared specification context guide the agent; unavailable lifecycle authority remains unknown rather than an inferred wire value.

Alternative rejected: an inference layer matching document prose, names or purposes to code. It would turn uncertain ownership into false graph facts and is outside this issue.

### 2. Reuse canonical facts and purpose ownership

Keep document-file ownership of the canonical `documents` relation. Retain headings and exact occurrence spans as evidence. The inbound `documented_by` wire view is a projection of that same fact. File-first queries must not require a hidden heading lookup, and repeated headings must not multiply logical edges.

The current relation-purpose contract has `approved`, `unavailable` and `not_applicable` states. An unreviewed suggestion must not appear as approved. Resolve each local endpoint to its own accepted purpose at query time and retain its actual path, status and provenance. Editing a purpose changes authored-purpose revision, not the graph's logical identity; a new read must observe it without rebuilding the graph, while an old continuation remains revision-bound.

Alternative rejected: copying purpose strings into each graph row or persisting an inverse edge. Both create a second refresh authority and stale-data risk.

### 3. Define a usable recipe with existing directional surfaces

The inspected CLI supports `outbound` and `inbound`; it does not have a `both` direction. Reuse two bounded views rather than inventing a new enum or endpoint for this issue:

```bash
atlas symbols relations --view detailed --file <source-path> --direction outbound --content-selection source --limit 10
atlas symbols relations --view detailed --file <source-path> --direction inbound --relation documents --content-selection both --include-occurrences --limit 10
```

MCP uses equivalent `atlas_symbol_relations` arguments with the same explicit `worktree` or `project_path`, `view: "detailed"`, file and bounds. The specification branch uses `direction: "inbound"`, `relation: "documents"`, `content_selection: "both"` and bounded occurrences. Outbound traversal of the returned specification uses the same document family and exact root to confirm its implementation target.

The skill must explain when each view is needed, how to follow the returned document/source selectors, and why an outbound source-only result cannot prove that no specification exists. Preserve legacy omitted-selection behavior. Do not expand or change unrelated default output. If a future implementation already provides an equivalent composite view, verify and reuse it without requiring another API.

Do not silently merge results from different roots, generations or purpose revisions. Inspect each page's identities, relation-specific coverage and typed continuation. On staleness, perform the recommended bounded recovery and repeat only the affected view. A user-requested specification context must not end with a source-only page and no explanation or next step.

### 4. Classify failures at the owning boundary

| Observed state | Disposition and owner |
| --- | --- |
| No supported authored reference | Document-authoring gap; add a reviewed explicit reference if required by the actual specification. |
| Complete extraction with no candidates | Truthful document zero-coverage state, not proof that the requirement has no implementation. |
| Unsupported, dynamic or ambiguous reference | Preserve typed unsupported/unresolved evidence; document scope instead of guessing. |
| Stale index or purpose revision | Use existing refresh/restart guidance and preserve the prior publication. |
| Wrong normalization, target, occurrence or file anchoring | Reproduce and fix the parser/resolver/projection owner, covering equivalent callers and variants. |
| Edge stored but absent in the documented request | Repair selection, directional traversal, adapter projection or guidance at the confirmed owner. |
| Correct edge but wrong or stale purpose | Repair authoritative endpoint lookup/revision handling; preserve graph identity and approval semantics. |

If all current runtime behavior is correct, retain it. Missing authoring, documentation or coverage is sufficient work when it is the actual gap. Do not create a gratuitous production change to justify the issue.

### 5. Preserve data, resource and compatibility contracts

SQLite remains the project-local engine and normalized graph owner. Reuse current source-first/target-first indexes, prepared bounded adjacency, batch endpoint hydration and publication transactions. Do not materialize the full graph or add per-row purpose queries. A high-fan-out document and many documents pointing to one source must remain bounded with truthful continuation and work counters.

Keep approved purposes separately authored. Full/incremental refresh must publish canonical relations, occurrences, coverage and classification consistently, remove stale affected edges and preserve unrelated purposes. Source or target edit/rename/delete/ignore changes must converge with a complete rebuild. Cancellation, concurrent changes, errors and incompatible state must retain the last complete generation and an adequate retry path.

No migration or derivation bump is presumed. If a confirmed fix changes persisted derived semantics, use the existing contract-invalidation owner and prove supported upgrade, reopen, rollback/recovery and purpose retention before accepting the change. A schema or dependency proposal would require its own measured necessity and review before adoption.

### 6. Activate RC2 without changing RC1 admission

Keep this issue mapped but outside `v0.5.1-00` milestone/native membership while RC1 is in flight. The current RC1 release graph is unchanged by this planning packet.

The user's later explicit RC2 direction governs this specification/source relation visibility and purpose-integration topic, including confirmed defects within that verification scope. This narrowly overrides the general RC1 self-hosting rule for #751 only. Unrelated stable 0.5.0 defects remain governed by the RC1 rule; neither incidental discovery here nor a shared file grants them this exception.

Before implementation, task 1.2 must verify the published non-draft RC1, exact tag/candidate and installed acceptance, refresh this checkout onto that accepted baseline, and activate the next-candidate scope of the existing v0.5.1 release owner #715. Reopen/reconcile #715 for RC2, add #751 to the v0.5.1 milestone and declared/native child/blocker set, and pass the ordinary graph checks. Historical closed children remain historical. Do not add a reverse #751-blocked-by-#715 edge, which would cycle with release acceptance being blocked by #751.

This is a concrete deferred activation prerequisite, not permission to perform it now or to publish RC2/stable. Until activation, the fully specified issue remains backlog and implementation tasks remain unchecked apart from genuinely completed planning proof.

## Verification matrix

| Boundary | Positive proof | Negative/failure proof |
| --- | --- | --- |
| Actual repository pairs | Unmodified copies of reviewed committed OpenSpec/source references; exact targets and both directions. | A specification with no authored link remains a reported gap; unrelated same-named files are not substituted. |
| Headings and occurrences | File-first ownership, supported heading selectors and repeated occurrences. | Wrong/unsupported fragments and duplicate names retain typed outcomes; no convenient fallback. |
| Agent recipe | Source dependency view plus inbound specifications and outbound confirmation through JSON/TOON/MCP. | Source-only defaults or wrong filters are not reported as complete specification absence. |
| Purpose ownership | Distinct accepted source/document purposes and actual provenance; update via purpose API without graph rebuild. | Missing/unapproved/non-local states; wrong-endpoint copying; stale purpose-bound continuation. |
| Lifecycle and coverage | Multiple applicable specifications, declared lifecycle context, complete document zero. | Historical/unknown authority, failed/partial parsing and bounded/truncated pages never become absence or conformance claims. |
| Incremental publication | Add/edit/rename/delete/ignore changes on both endpoints converge with full rebuild. | Injected failure/cancellation/concurrent mutation preserves prior generation, identity and purposes; adequate retry succeeds. |
| Root and platform | Plain/Git/linked projects; Windows/Linux/macOS Intel/ARM; supported native paths and aliases. | Wrong/missing root, ignored/outside/indirect targets and sibling divergence; no implicit init/reset or private-path disclosure. |
| Upgrade and resources | Supported predecessor/reopen behavior and bounded high-fan-out traversal with indexed/batched reads. | Stale derived semantics, unsupported state, byte/deadline limits and accidental N+1/whole-graph work. |

One coherent test may cover several rows. Reuse verified evidence when its behavioral inputs are unchanged; do not require a unique test ID or repeated full campaign for each row. Actual named execution and meaningful assertions are required; zero matched tests, help/schema-only output and a feature declaration are insufficient.

## Gates and handoff

Strict OpenSpec and owner-scoped IssueOps must pass for this packet. Implementation follows the repository's affected-check planner and normal hooks. Rust changes retain locked formatting, all-target/all-feature check and Clippy, appropriate unit/integration/E2E, doctests and warning-clean rustdoc. Reuse the current holistic command when relevant:

```bash
cargo test --locked -p projectatlas-cli --test e2e_worktrees holistic_agent_worktree_flow_keeps_local_atlases_isolated_across_cli_watch_and_mcp -- --exact --include-ignored --nocapture
```

Extend/reuse the owning parser, real SQLite and service tests for the matrix, plus actual installed CLI/MCP proof and all four native platforms where relevant behavior differs. Registry-derived selectors must actually run. Changes to hot query/publication work require representative bounded measurements; a prose-only/reference fix does not justify a new numerical campaign.

Before closing, reconcile the issue, tasks, spec, actual source, architecture and graph-specific shipped guidance. Complete the five ordered independent acceptance reviews only after all implementation tasks pass. Record defect root causes, broader surfaces checked, reused evidence, deliberate unsupported scope and remaining uncertainty.

## Risks / Trade-offs

- Static links do not prove semantic conformance: preserve provenance and require source/spec follow-through.
- A purpose suggestion can look authoritative: expose only the current accepted owner through the relation contract.
- Unbounded document fan-out can hide a correct link: preserve limits and exact continuations; test complete bounded traversal.
- Stale references can survive a target move: prove affected closure/full convergence and supported upgrade repair.
- RC2 planning could block RC1 accidentally: retain no current milestone/native membership and gate activation under task 1.2.

## Open Questions

None. Runtime edits are conditional on reproduced defects; the required behavior, ownership, bounded interfaces, acceptance matrix and RC2 activation condition are specified.
