## Context

PR #566 updates the libraries used by the existing MCP server and optional parser worker. The manifest retains the current MCP feature selection and disables default language-pack features while enabling dynamic loading. The worker and accepted parser artifacts remain separate authorities.

## Goals / Non-Goals

**Goals:** Adopt the intended maintenance versions with a reproducible lockfile and prove compatibility at the existing consumer boundaries.

**Non-Goals:** No new runtime component, MCP transport, parser language, parser artifact, supported platform, feature flag, or test harness.

## Decisions

- Apply the existing manifest and lockfile update onto accepted main. Retain dependency features and unrelated package versions instead of running an unrestricted dependency update.
- Keep optional parser artifacts and their manifests unchanged. A loader dependency update does not itself authorize replacing the accepted native grammar inventory.
- Reuse existing MCP stdio, parser-worker, and platform tests. Help output or successful compilation alone cannot prove transport or dynamic-loader compatibility.
- Let the affected-proof planner and normal pre-push hook select mandatory checks. Inspect hosted selection for optional parser-pack consumers and execute existing supported proof routes where necessary; do not introduce another workflow.
- Architecture diagrams are N/A because MCP, parser-worker, and artifact trust ownership remain unchanged.

## Risks / Trade-offs

- Dependency or feature drift: compare the candidate manifest and lockfile against the intended update.
- MCP protocol or lifecycle regression: run existing identity, stdio, and failure-boundary tests with the new locked SDK.
- Native loader or artifact compatibility regression: preserve built-in precedence and verify existing supported loading and unsupported/refusal behavior with the unchanged accepted artifact contract.
- A skipped CI job can hide a proof gap: inspect the selected checks and actual completed platform results before acceptance.

## Migration Plan

Land the dependency update through PR #566 with issue #568 as its sole owner. No database or authored-state migration is required. Roll back through a reviewed revert of the manifest and lockfile changes if a confirmed incompatibility appears. A required runtime or artifact change must be assessed against this owning scope before implementation.

## Dependencies / Cross-Issue Impact

Issue #568 owns PR #566 and is a direct child of release owner #492, which remains blocked until acceptance and merge. There are no direct implementation blockers. Other dependency updates, #465, and #358 remain outside this issue.

## Open Questions

None.
