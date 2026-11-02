## Why

Installed 0.5.0 admits files excluded by a quoted global excludes path containing spaces and by a linked worktree's shared Git info/exclude. Native Git confirms the exclusions; full rescans reproduce the mismatches. Full traversal and single-path policy currently use different incomplete input interpretation.

The broader audit also reproduced targeted matching losing ancestor rules when a subdirectory is selected and reopening an ignored parent through a leaf negation, unlike full traversal. Review identified missing continuous-watch coverage/routing for selected external policy files and repeated policy discovery per event path. These manifestations share the selection/observation boundary and remain required RC1 work under this issue.

## What Changes

- Select external global config/exclude inputs with canonical Git-config value parsing, bounded reads and explicit existing source precedence; preserve trusted operator paths.
- Reconcile full traversal and targeted/document selection, including linked Git control files, without disabling no-Git .gitignore support or changing local-rule precedence.
- Observe every selected config/policy input that can alter this selection. Reconcile unchanged prior indexes through existing derived publication invalidation and retain authored state.
- Reuse the configured ignore engine's hierarchical matching for full and targeted paths, including inherited rules and ignored-parent pruning. Continuous CLI/MCP watch must observe selected external inputs, absent-parent creation and changed selections without admitting outside files or reconstructing global policy for each event path.
- Preserve complete reconciliation for incremental watch limits at the controlled production owner under the original work budget; test that owner rather than a test-only convenience wrapper. Bound repeated continuous-CLI policy-fence overlap and retain ordinary polling fallback with the active control carried through its first reconciliation.
- Distinguish selected/resolved SQLite runtime namespaces from distinct source aliases to the same DB or metadata directory. Preserve external DB symlinks, managed Windows short-name cohorts, real CLI/MCP mutation/read workflows and policy-event visibility without retargeting SQLite opens.
- Invalidate warm verified epochs for document-only target events even when ignore policy excludes every source path; preserve typed read-only refresh guidance, last-valid publication and explicit reconciliation without a recursive rebind for ordinary target changes.
- Restore recursive coverage when a supported source directory is replaced at its selected pathname while the database/config remain external. Serialize structural uncertainty with event ingress before accepting a warm result; preserve existing root/lifecycle admission and ignore ordinary access events without triggering a rebind.
- Apply the already specified document-prefix admission boundary to the shared uncached policy inspector used by this issue's freshness probe and ordinary projection. Check each existing native prefix before per-target policy reads, using the caller's cooperative control; retain explicit global/shared policy inputs and typed source outcomes. #738 retains its separate complete acceptance and closure.
- Reuse one bounded native ignore matcher per document-resolution pass, including every keyset page of a cold probe. Bound cumulative hierarchical policy parsing and retained directory contexts; separately admit at most 1,000,000 cold-verification rows per pass before normalization or any early exit. Preserve fresh policy at the next pass and final publication revalidation, typed non-retrying refusal and authored state.
- Pass the required dependency/security gate with the patched Markdown parser for reachable RUSTSEC-2026-0433. Preserve ordinary and wiki references, use upstream nested-image handling rather than disabling wikilinks, and repair changed heading facts through the existing derivation/publication owner.

## Capabilities

### New Capabilities

- `external-source-policy`: consistent external Git exclusion inputs at existing scanner, refresh, document and source-observation boundaries.

### Modified Capabilities

None. Existing canonical specs do not separately own this behavior.

## Impact

Filesystem policy selection, WalkBuilder setup, policy-source observation, extraction fingerprint and causal unit/real CLI/MCP/upgrade proof. Reuse ignore for glob matching and a canonical Git-config syntax parser for config syntax; do not add a custom INI/regex parser or Git process requirement. Review locked dependency/MSRV/license/resource costs before check-in. Additive Rust batch-matcher APIs preserve the existing single-path API and closed error/resource enums. No graph/schema/CLI-command/MCP-tool change.

## Release Scope

High-priority #739 is a direct child/blocker of #715 for v0.5.1-rc1. The latest security review confirmed skipped cold-verification rows bypassed policy and native-prefix quotas. Source907 repairs the shared paged owner with a cumulative row allowance, preserving existing typed non-retrying refusal and state. Causal and document/MCP regression tests, independent source review, fresh default-artifact costs, six upgrade/rollback cases, six optimized CLI/MCP scenarios and a 10,801-row CLI compatibility fixture pass locally. The unchanged native selection/observer behavior retains the earlier four-platform proof at source6f9; the new exact row regression is added to every native platform job and still requires hosted completion. Tasks 2.1 and 2.2 were reopened for this boundary; complete only their earned behavior and proof. No broader finding is deferred.

The updated selector, cost, row-limit, contract and predecessor/rollback handoff is recorded on [release owner #715](https://github.com/styler-ai/ProjectAtlas/issues/715#issuecomment-6589398173) and linked from [combined-performance owner #708](https://github.com/styler-ai/ProjectAtlas/issues/708#issuecomment-6589401634); it retains the earlier detailed predecessor requirements. Their remaining installed/performance/release work is separate. Tasks2.1/2.2 still require the affected native/hosted proof; task3.1, all five acceptance reviews and the final publication/review gates remain open before #739 merge and closure.

## Non-Goals

No general Git implementation, Git process execution in normal indexing, blanket filesystem jail, new language/provider, source execution, DB reset or stable publication. Do not claim arbitrary file disclosure/code execution. Unimplemented Git features must be distinguished from regressions using the current supported policy contract and recorded explicitly, not silently generalized.
