## Why

ProjectAtlas produced v0.3.26 with a lean engineering loop: implement a meaningful behavior slice, add or update focused tests, run ordinary Rust/workspace checks, commit the compiling slice, use normal CI and review, and synchronize the OpenSpec/GitHub checklist. The v0.4 work expanded that loop into per-task test identifiers, verification plans, evidence ledgers, SHA receipts, rendered evidence comments, and a broad coverage/mutation campaign. That machinery slowed product implementation without making ordinary task completion more trustworthy than working tests and the GitHub check that already identifies the commit it ran.

## What Changes

- Restore checklist-only IssueOps: every active OpenSpec change is mapped and each authoritative GitHub checklist exactly mirrors its local task text, order, ownership, and checked state.
- Restore the readable v0.3.26 issue contract used by #305 for active mapped work: why, what changes, capabilities, release scope, non-goals, pre-mortem, and the authoritative OpenSpec tasks.
- Make every pre-mortem mitigation a visible checkbox mapped to existing OpenSpec task IDs; its state follows those tasks instead of creating a second proof system.
- Keep deterministic multi-issue task ranges only when one OpenSpec checklist must be split; ranges remain ordered, gap-free, and non-overlapping.
- Use focused behavior, integration, E2E, smoke, or validation tests for meaningful implementation slices. One coherent test may prove several related tasks.
- Keep the ordinary blocking Rust/workspace gates used for v0.3.26: format, workspace check, strict Clippy, workspace tests, stable doctests, warning-free rustdoc, source lints, dependency policy, and affected behavior/E2E checks.
- Keep ordinary PR issue references, milestone assignment, and unresolved-review-thread validation. Run full milestone checklist completion only for release.
- Remove task-specific test identifiers, verification plans, evidence ledgers, task commit digests, exact-head task receipts, rendered evidence workflows, task-level run/permalink requirements, OpenSpec commit-link blocks, and issue-sealing machinery.
- Remove the unfinished nextest/coverage/mutation quality campaign from #309. Future focused quality improvements may be proposed independently when they solve a demonstrated product or release risk.
- Preserve real integrity controls such as SHA-pinned GitHub Actions, locked Cargo commands, package/signature/digest validation, release checksums, least privilege, and failure propagation.

## Capabilities

### New Capabilities

- `rust-test-quality-gates`: Lean Rust quality and IssueOps workflow matching the proven v0.3.26 development loop.

### Modified Capabilities

None.

## Impact

- **IssueOps:** `.github/scripts/issue-checklists.py` becomes a small standard-library synchronizer for authoritative OpenSpec/GitHub checklists and release-only milestone completion.
- **Issue quality:** active mapped issues use the #305 planning shape, and their pre-mortem mitigation checkboxes reference and track real OpenSpec task IDs.
- **CI:** ordinary workspace tests and stable doctests run with the existing format/check/Clippy/rustdoc/source/dependency/affected-E2E gates; task evidence, coverage, and mutation campaign jobs are removed.
- **Developer workflow:** `.githooks/pre-push`, the PR template, and `docs/workflow.md` describe the same lean loop.
- **Tests:** one IssueOps self-test and focused workflow-policy E2E coverage protect the contract without per-task wrappers.
- **Runtime:** no ProjectAtlas product API, MCP, storage, parser, or runtime behavior changes.
