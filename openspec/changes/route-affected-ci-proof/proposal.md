## Why

ProjectAtlas currently spends roughly 29-47 minutes of pull-request wall time and 53-75 raw runner-minutes on the same broad Rust and four-platform proof, even for a one-line OpenSpec task update; review and review-comment events can launch that complete pipeline again without any code change. Normal pull requests need materially faster required checks without reducing the tests capable of detecting defects in the exact changed behavior.

## What Changes

- Separate live pull-request metadata from code verification and use GitHub's native required-conversation-resolution rule for review threads, so review activity never recompiles or reruns tests for an unchanged source tree and thread resolution cannot leave a stale workflow result.
- Allow automatic cancellation only when a newer `pull_request` source-verification run supersedes an older source-verification run for the same pull-request number; isolate every other event and workflow namespace from cancellation.
- Add one Python-standard-library planner that reads the exact change set, derives Rust reverse dependencies from one `cargo metadata` result, adds only declared non-Cargo contract edges, and explains both selected and omitted proof contracts.
- Run only selected Rust, integration, and platform proof in local pre-push and concurrently in hosted CI behind one fail-closed required aggregate; unknown, shared, workflow, toolchain, lockfile, schema, planner, or contract-map changes select the complete normal-PR proof.
- Require planned-issue IssueOps to compare the live implementation-task text, order, and checked state literally with the mapped `tasks.md`, including equal-count drift.
- Preserve the complete four-platform installed-product matrix at the integrated release-candidate boundary, with the whole boundary restarted after a confirmed defect is fixed.
- Measure wall time and raw runner-minutes before and after for docs-only, leaf-crate, CLI test-domain, shared-core, and platform-sensitive changes; retain only routing that produces a significant improvement without losing causal proof.

## Capabilities

### New Capabilities

- `affected-ci-proof-routing`: Fail-closed selection, execution, explanation, and aggregation of the build and test contracts capable of detecting defects for an exact change.

### Modified Capabilities

- `codex-pr-review-thread-gate`: Replace the stale Codex-only Actions polling gate with GitHub native required conversation resolution for every pull-request review conversation.

## Non-Goals

- Remove or weaken an owning unit, integration, E2E, failure, compatibility, installer, packaging, security, or platform contract.
- Add mutation, coverage, or nextest campaigns to normal pull-request CI.
- Add Cargo target caching, a third-party planner action, a new Rust crate, a database, or a generalized build system without separate measured evidence.
- Replace the complete installed-product release-candidate matrix with affected pull-request proof.
- Reopen declined issue #497 or broaden the accepted v0.5 release scope beyond this CI acceleration work.

## Impact

The change affects the ordinary CI workflow, local pre-push, branch protection, its required-context migration, one standard-library planner/self-test, removal of the superseded Codex-only polling script, the existing IssueOps and workflow-policy self-tests, and the owning architecture documentation. It builds on #487's responsibility-coherent CLI E2E targets, preserves #366's input-equivalent release-proof reuse, and applies #341's measured-materiality rule without extending its cache design. It changes no product runtime, crate dependency, database schema, public CLI/MCP behavior, package, or release artifact. Issue #555 is a direct `v0.5.0-00` child of release owner #492 and is blocked only by completed #487 so the acceleration lands before the remaining release issues.
