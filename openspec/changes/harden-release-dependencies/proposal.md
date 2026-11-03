# Proposal

## Why

RC1 must remove the vulnerable diagram-validation dependency reported by GitHub and reconcile the open Dependabot updates. Automatic security monitoring must produce verified coverage and actionable findings rather than an enabled setting alone.

## What Changes

- Replace the affected KaTeX dependency graph with compatible patched versions, preserve diagram validation and add missing npm version-update coverage.
- Review and incorporate Dependabot updates #729, #660, #695, #677 and #675 through the existing dependency and SHA-pinned workflow owners, including related upstream security fixes.
- Detect low-severity npm advisories in the existing audit gates instead of silently admitting them under the moderate threshold.
- Verify automatic GitHub CodeQL, Dependabot and secret protections plus online Codex security review. Assess the initial scanner findings with source evidence, fix confirmed supported-boundary failures for RC1 and record false positives or unsupported coverage honestly.
- Reconcile parser/semantic invalidation, public contracts, installed-platform and performance proof against the final combined dependency identities.

## Capabilities

### New Capabilities

- `dependency-security-admission`: reviewed, compatible dependency updates and advisory-free release admission for the affected dependency graph.
- `repository-security-monitoring`: verified automatic source, workflow, dependency and PR security analysis with evidence-based finding disposition.

### Modified Capabilities

None. The canonical capability inventory contains token-impact reporting; prior dependency changes remain historical evidence.

## Impact

Workspace Cargo manifests/lockfile, filesystem classification, Markdown and identifier parsing, MCP/runtime dependencies, Mermaid manifests/lockfile, Dependabot configuration, pinned CI/release/parser-pack actions, repository security settings and their existing tests/documentation. No new runtime service or database schema.

## Release Scope

High-priority #732 is required for v0.5.1-rc1 under #715. Specification review and native milestone/parent/blocker admission precede source implementation. It can proceed alongside #731; #708 and #715 must consume the final accepted dependencies and workflows before their combined proof. Stable promotion requires separate authorization.

## Non-Goals

No speculative exploit claim, advisory suppression instead of the required upgrade, unreviewed automatic merge, unsafe credential handling, weakened checks or unrelated feature work.
