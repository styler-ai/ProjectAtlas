## Context

ProjectAtlas v0.3.26 was built and released with ordinary engineering proof: focused tests for changed behavior, workspace-wide Rust checks, normal CI/review, and synchronized OpenSpec/GitHub checklists. During v0.4, #309 accumulated a second proof system around those checks: unique test IDs per task, task verification manifests, task evidence ledgers, tested-commit receipts, rendered issue comments, hosted links per checkbox, and post-merge issue sealing. It also grew into a large nextest, coverage, and mutation program that became a prerequisite to implementing product issues.

The user has accepted the v0.3.26 workflow as the quality baseline for v0.4 implementation. This change therefore owns only restoration of that lean workflow. It does not claim that coverage or mutation analysis has no value; it removes them as an unfinished repository-wide #309 campaign and leaves future quality work to focused proposals justified by concrete risk.

This is repository workflow policy, not product runtime behavior. The smallest design is a standard-library Python checklist synchronizer, existing GitHub workflow composition, existing Rust workspace gates, and focused E2E assertions. No Rust framework, new crate, task database, evidence schema, or hosted renderer is justified.

## Goals / Non-Goals

**Goals:**

- Make local OpenSpec tasks and authoritative GitHub issue checklists match exactly.
- Keep split issue ownership deterministic, gap-free, ordered, and non-overlapping.
- Let one meaningful behavior test cover several coherent tasks.
- Keep ordinary Rust/workspace checks and affected behavior tests blocking.
- Keep issue references, milestones, review-thread resolution, and release-only milestone completion.
- Delete the per-task receipt/evidence/sealing system and the abandoned coverage/mutation campaign.
- Preserve genuine source, workflow, supply-chain, package, and release integrity controls.

**Non-Goals:**

- Requiring a unique test, test ID, run link, source link, SHA receipt, or evidence row for every task.
- Proving a repository-wide coverage percentage or mutation score.
- Running nextest, LLVM coverage, changed-source mutation, or a full mutation campaign as part of #309.
- Changing ProjectAtlas product, MCP, parser, storage, graph, memory, or installer behavior.
- Touching `main`, publishing a release, or weakening release artifact integrity.

## Decisions

### 1. Keep one authoritative checklist contract

`openspec/changes/<change>/tasks.md` remains the local task definition. `openspec/issue-map.json` maps every active change to one primary GitHub issue and, only when needed, ordered owner ranges. `.github/scripts/issue-checklists.py` extracts only `OpenSpec Tasks` or `OpenSpec Task Checklist` sections and compares task text, order, ownership, and checked state exactly. Closed issues fail when their authoritative checklist still contains unchecked tasks.

The script accepts the existing integer mapping and schema-2 owner mapping so the restoration does not require unrelated issue-map migration. It invokes authenticated `gh` through fixed argument vectors without a shell. Its self-test covers task parsing, section isolation, owner slicing, exact comparison, and paginated GitHub responses.

Alternative considered: retain the general evidence/renderer engine and disable most rules. Rejected because dormant schemas and commands preserve the maintenance burden and invite the same ceremony back into normal implementation.

### 2. Treat tests as behavior proof, not task receipts

A meaningful implementation slice adds or updates the smallest unit, integration, E2E, smoke, or validation test appropriate to its actual failure risk. Several related tasks may share one coherent test. Documentation and planning tasks do not invent production tests. GitHub Actions already records the commit and result of the normal test run, so IssueOps does not create a second receipt for each checkbox.

Alternative considered: require one uniquely named test per task. Rejected because task boundaries are planning boundaries, not necessarily behavior boundaries, and duplicate wrappers do not add coverage.

### 3. Keep the proven ordinary Rust gates

Normal CI and local integration use the same core commands:

- `cargo fmt --all --check`;
- `cargo check --workspace --all-targets --all-features --locked`;
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`;
- `cargo test --workspace --all-features --locked`;
- `cargo test --doc --workspace --all-features --locked`;
- warning-free workspace rustdoc;
- ProjectAtlas source lints and dependency policy;
- affected CLI/MCP/installer behavior tests where the slice changes those surfaces.

These commands are sufficient to integrate a significant compiling issue slice into `dev`. They do not wait for a repository-wide evidence campaign. Existing SHA-pinned Actions, locked dependency use, parser/package/signature/digest checks, checksums, and least-privilege workflow permissions remain because they protect executable and release integrity rather than task bookkeeping.

Alternative considered: keep the incomplete independent nextest/coverage/mutation jobs as required #309 gates. Rejected because the user selected the v0.3.26 workflow, and the campaign was blocking product work before its own scope and value were stable.

### 4. Separate ordinary integration from release completion

Pull requests reference their issue, carry the intended milestone, pass ordinary CI, and resolve actionable review threads. The IssueOps synchronization check runs on ordinary PRs, but it does not require every issue in the milestone to be complete. The full `--milestone` checklist gate runs only in the release path.

Significant compiling v0.4 slices integrate into `dev`; `main` and release publication remain untouched until the combined `dev` surface works. This preserves the user's requested development order without turning local task completion into release ceremony.

Alternative considered: run milestone completion on every PR. Rejected because unrelated planned work in the same milestone would block incremental integration.

## Risks / Trade-offs

- [Removing the evidence subsystem could be mistaken for removing tests] -> Keep ordinary workspace and focused behavior tests explicit in CI, the hook, docs, OpenSpec, and E2E assertions.
- [A small checklist parser could accept unrelated checkboxes] -> Parse only the authoritative heading and its numbered subsections; self-test section isolation.
- [Split ownership could hide gaps] -> Require the full local task sequence to be covered exactly once in order.
- [Release could proceed with incomplete issues] -> Keep full milestone completion in the release-only invocation.
- [Security checks could be removed with evidence checks] -> E2E policy assertions distinguish task receipts from SHA-pinned Actions and real integrity validation.
- [Future quality work could repeat the same scope failure] -> Require a separate focused proposal tied to a demonstrated risk rather than rebuilding a universal per-task proof framework.

## Migration Plan

1. Replace #309's broad quality/evidence artifacts with this lean v0.3.26-parity OpenSpec contract.
2. Reduce IssueOps to exact checklist synchronization and release-only milestone completion.
3. Remove task evidence plans, ledgers, renderers, receipt tests, quality/mutation policy code, and their CI jobs.
4. Restore ordinary workspace tests and stable doctests to normal CI and align the hook, PR template, and workflow documentation.
5. Run the IssueOps self-test, focused workflow-policy E2E tests, strict OpenSpec validation, and ordinary Rust/workspace gates.
6. Synchronize GitHub #309, integrate the compiling commit into `dev`, and close #309. Leave `main` and release publication untouched.
