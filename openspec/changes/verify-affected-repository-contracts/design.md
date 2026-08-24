## Context

ProjectAtlas currently runs one broad `verify` job and four platform E2E jobs for every pull request. That proves the repository, but it also repeats unrelated builds and checks. The repository already has two narrower foundations that this design composes instead of replacing: #341 supplies digest-addressed dependency-layer reuse, and #366 supplies fail-closed input-based proof reuse. #372 remains the separate timeout owner.

The change is CI policy, not product architecture. It must preserve the seven owned Rust crates, every existing proof family, stable protected-branch contexts, and parity between human and Dependabot pull requests. Default-branch, scheduled, candidate, and release boundaries remain complete-repository proof because they establish or consume shared truth.

## Goals / Non-Goals

**Goals:**

- Compute the smallest contract-complete set of existing checks affected by a pull-request diff.
- Make the plan deterministic, reviewable, bound to its exact inputs, and fail closed.
- Keep every required context present with either affected success or a trusted plan-bound not-applicable result.
- Reuse Cargo's dependency graph and the repository's existing workflow/classifier mechanisms.
- Reduce redundant runner, compiler, and test work without reducing proof.

**Non-Goals:**

- No new Rust crate, database, schema, service, build system, generalized dependency-graph framework, provider/plugin system, third-party Python package, or proof ledger.
- No removal, dilution, or bot exemption for unit, integration, E2E, platform, security, dependency, packaging, installer, OpenSpec, IssueOps, Mermaid, or release checks.
- No reuse of a plan across a different base, head, event, workflow, contract revision, toolchain, or platform.
- No cancellation of IssueOps, release, deployment, publication, or other mutation work.

## Decisions

### One closed impact contract and one standard-library planner

One checked-in data file owns a closed set of repository contract identifiers, path classifiers, and only the cross-contract edges Cargo cannot express. One Python-standard-library planner reads that file, the pull-request diff, and one `cargo metadata` result. It emits a bounded deterministic plan consumed by the existing workflows.

The declared cross-contract edges cover database/schema, CLI/MCP, Python, Node/Mermaid, documentation/OpenSpec/IssueOps, installers/packages, fixtures/generated inputs, and platform gates. Rust crate reverse dependencies come from `cargo metadata`; they are not duplicated in the data file. Existing repository classifiers and helpers are reused where their current contract fits.

The alternative of a provider/plugin framework or a second dependency graph was rejected because the variant set is repository-owned and closed. The alternative of independent workflow-local classifiers was rejected because two planners could disagree. A concrete data contract plus one script is the smallest mechanism that keeps the policy inspectable.

### A plan is evidence only for its exact identity

The planner records the base and head commit, pull-request event class, impact-contract digest, planner/workflow identity, toolchain identity, platform when platform-specific, selected contracts, and one digest over the canonical plan. Downstream jobs validate that identity before trusting either work or not-applicable state.

Changed planner code, its data contract, aggregation/workflow ownership, shared repository metadata, unknown paths, every rename or deletion, failed or malformed diffs, failed `cargo metadata`, and stale or mismatched plan fields select full proof. Only ordinary additions and modifications may select the union of all known effects, and any fail-closed input in a mixed diff selects full proof. Both sides of a rename remain diagnostic input, but classification of both sides never makes affected selection permissible. This makes false negatives more expensive in compute, never in quality.

The alternative of caching a plan by branch or pull-request number was rejected because either identity can outlive the evidence inputs.

### Event boundaries decide whether selection is permitted

Pull-request workflows for human and Dependabot authors use the same planner and quality bar. They execute only the affected existing contracts when the plan is trusted. Pushes to the default branch, scheduled drift checks, release candidates, releases, and manually invoked release proof bypass affected selection and execute the complete proof set.

This separation keeps optimization at the speculative pull-request boundary while all shared and published boundaries re-establish full truth.

### Stable contexts aggregate proof instead of disappearing

Every protected context is represented by an always-created aggregation job. For each owned contract it accepts only:

1. a successful affected execution bound to the exact plan, or
2. a planner-produced not-applicable record bound to the same plan and context.

A manually skipped job, absent output, cancellation, failure, stale plan, digest mismatch, malformed record, or unrecognized conclusion fails aggregation. A not-applicable record means the closed impact contract proved the work irrelevant; it is not a workflow condition silently omitting proof. Existing context names remain stable.

### Cancellation is limited to superseded read-only pull-request CI

Concurrency cancellation may group only read-only pull-request CI by pull request and cancel an older run when a newer head supersedes it. IssueOps, merge authorization, release, deployment, publication, scheduled drift, default-branch proof, and any mutation path are outside that group. Cancellation of the current plan is a failed/missing required context until the replacement head proves itself.

### Rust, storage, and resource pattern fit

No Rust implementation changes. The accepted seven-crate dependency direction and all public CLI/MCP/storage contracts remain untouched. The Rust pattern-fit judgment is therefore “no named Rust pattern needed”: CI composes existing crate checks rather than adding a trait, enum, adapter crate, or runtime abstraction. Database and SQLite implications are N/A because the contract is a checked-in file and ephemeral workflow output only.

Planner cost is linear in changed paths plus Cargo graph nodes/edges plus declared cross-contract edges. `cargo metadata` runs once, sets/maps remain bounded by repository size, and output is limited to the closed contract set. A full-proof fallback has the current worst-case resource cost; the optimization cannot make the worst case larger except for the small planning step.

## Dependencies / Cross-Issue Impact

#497 has no native blocker and adds no blocker edge to independent product work. Its checked section 1 owns only the accepted proposal, design, capability, dependency, task, and architecture contract required before milestone planning; section 2 remains the executable delivery and closes only after implementation, proof, and final diagram review complete. #497 directly unlocks campaign-automation issue #498. The shared #500 planned-issue mechanism is a readiness prerequisite rather than a native dependency, so implementation handoff still waits for its accepted published contract and promoted graph without making #500 a product blocker.

## Risks / Trade-offs

- **A missing path or edge could under-select proof** → unknown, shared, planner-owning, rename, and deletion inputs select full proof; focused fixtures cover ordinary additions/modifications, every rename/delete shape, mixed diffs, and every declared edge.
- **A stale plan could bless a skip** → every execution and not-applicable record validates exact plan identity and digest before aggregation.
- **Conditional jobs could make a required context look green without work** → stable aggregators accept only plan-bound affected success or trusted not-applicable state and reject every other conclusion.
- **Cancellation could interrupt stateful work** → only superseded read-only pull-request CI shares the cancelable concurrency group.
- **The planner could become a second build system** → the data file names existing contracts and cross-contract edges only; Cargo remains Rust graph authority and workflows remain execution authority.
- **Planning overhead could erase savings on small diffs** → one stdlib process and one `cargo metadata` call remain bounded; representative plan fixtures and workflow timing compare affected and full paths before acceptance.

## Migration Plan

1. Add the closed impact contract, planner, deterministic fixtures, and dry-run plan reporting while the existing full workflow remains authoritative.
2. Integrate affected execution and stable aggregators, then prove human and Dependabot pull requests plus fail-closed/full-boundary cases.
3. Enable read-only pull-request supersession cancellation only after aggregation behavior is proven.
4. Roll back by making all event classes select the existing full proof; do not delete proof jobs or required contexts.

## Open Questions

None.
