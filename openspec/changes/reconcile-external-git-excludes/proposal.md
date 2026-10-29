## Why

Installed 0.5.0 admits files excluded by a quoted global excludes path containing spaces and by a linked worktree's shared Git info/exclude. Native Git confirms the exclusions; full rescans reproduce the mismatches. Full traversal and single-path policy currently use different incomplete input interpretation.

The broader audit also reproduced targeted matching losing ancestor rules when a subdirectory is selected and reopening an ignored parent through a leaf negation, unlike full traversal. Review identified missing continuous-watch coverage/routing for selected external policy files and repeated policy discovery per event path. These manifestations share the selection/observation boundary and remain required RC1 work under this issue.

## What Changes

- Select external global config/exclude inputs with canonical Git-config value parsing, bounded reads and explicit existing source precedence; preserve trusted operator paths.
- Reconcile full traversal and targeted/document selection, including linked Git control files, without disabling no-Git .gitignore support or changing local-rule precedence.
- Observe every selected config/policy input that can alter this selection. Reconcile unchanged prior indexes through existing derived publication invalidation and retain authored state.
- Reuse the configured ignore engine's hierarchical matching for full and targeted paths, including inherited rules and ignored-parent pruning. Continuous CLI/MCP watch must observe selected external inputs, absent-parent creation and changed selections without admitting outside files or reconstructing global policy for each event path.

## Capabilities

### New Capabilities

- `external-source-policy`: consistent external Git exclusion inputs at existing scanner, refresh, document and source-observation boundaries.

### Modified Capabilities

None. Existing canonical specs do not separately own this behavior.

## Impact

Filesystem policy selection, WalkBuilder setup, policy-source observation, extraction fingerprint and causal unit/real CLI/MCP/upgrade proof. Reuse ignore for glob matching and a canonical Git-config syntax parser for config syntax; do not add a custom INI/regex parser or Git process requirement. Review locked dependency/MSRV/license/resource costs before check-in. No graph/schema/public-route change.

## Release Scope

High-priority #739 is a direct child/blocker of #715 for v0.5.1-rc1. It starts at accepted main with no direct implementation blocker and can proceed independently of #738's document admission guard and #731's binding fix. Combined contracts, inventories and exact installed/performance proof belong to #708/#715 before RC1. Planning was independently approved; implementation and proof are in progress. No execution/proof task is complete yet, and the broader selection/watch findings are not deferred.

## Non-Goals

No general Git implementation, Git process execution in normal indexing, blanket filesystem jail, new language/provider, source execution, DB reset or stable publication. Do not claim arbitrary file disclosure/code execution. Unimplemented Git features must be distinguished from regressions using the current supported policy contract and recorded explicitly, not silently generalized.
