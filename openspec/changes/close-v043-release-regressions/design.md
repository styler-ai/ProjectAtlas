## Context

The four failures share a release concern—an optional or bounded adapter discarded an already valid local state—but do not share production ownership. The graph database owns bounded current-generation reads, the CLI owns TUI preview orchestration and optional Git probes, the analysis service owns VCS-impact children, the TUI owns deterministic visual sampling, and each installer owns host-specific Codex mutation. The fixes must stay at those existing boundaries.

## Goals / Non-Goals

**Goals:**

- Render a representative full-project Atlas preview without loading an unbounded graph into Rust.
- Keep structural/local ProjectAtlas navigation operational when Git is absent while preserving typed child failures.
- Keep VCS-impact requests responsive while a persistent MCP host keeps its own stdin open.
- Make failed official Codex plugin replacement locally reversible even when every network add fails.
- Prove each behavior in the accumulated local and hosted v0.4.3 release gate.

**Non-Goals:**

- Change graph publication, relation semantics, MCP schemas, database schema, or worktree isolation.
- Change the TUI visual language or checked-in README image.
- Reimplement or bundle Git.
- Add an installer cache service, package manager, dependency, crate, or generic transaction abstraction.

## Decisions

### 1. Rank representative graph seeds in SQLite and keep Rust expansion bounded

The database reads high-degree endpoints across the complete current-generation relation family, restricted to locally resolved, non-self, non-containment rows, using the existing relation-family/resolution index. It returns only a small deterministic hub page. Existing source/target adjacency indexes provide bounded continuation pages.

The CLI seeds each family from four ranked hubs and reuses two bounded adjacency rounds. The TUI chooses the largest connected component and ranks candidates by reachable branch size before applying the unchanged 12-link visual degree cap. Final 48-node and 64-link caps remain rendering limits rather than discovery limits.

Alternatives rejected: increasing the arbitrary first-128 relation page retains opaque-key bias; loading every relation into Rust violates memory/latency bounds; a generic graph-ranking subsystem adds no durable owner.

### 2. Isolate optional and VCS-impact Git probes at their existing owners

The effective Git-config probe returns unavailable only when process creation reports executable-not-found. Permission, timeout, malformed output, wait/output, and cleanup failures keep their typed error paths. The child continues to use null stdin and the existing deadline, preserving #409.

The analysis service's shared `git_command` builder also gives its noninteractive status/diff children null stdin. This fixes the sibling persistent-host failure once for every VCS-impact selection while preserving the existing output bounds, cancellation, deadline, cleanup, and typed unavailable behavior.

Structural `.git` control files and the exact selected project/database remain authoritative for local scan/navigation. Git-backed impact analysis reports typed unavailable when native Git cannot start; missing index and wrong-root behavior remain unchanged.

Alternative rejected: treating every spawn error as Git absence hides trust-boundary and host failures. Raising global timeouts cannot fix missing executables or inherited stdin.

The shared release-test batch helper now derives the request IDs it must observe, reads stdout concurrently, and closes stdin only after those responses arrive or the existing per-progress deadline fails. This explicitly supersedes #416's earlier generic-helper non-goal: the 34-call release matrix proved the shared helper is itself a required-response client, while immediate EOF hid the VCS-impact production defect.

### 3. Snapshot validated Codex state before destructive replacement

Each installer serializes inventory, snapshot, mutation, validation, and restore for the selected Codex root, then captures the validated official marketplace/plugin source tree and exact Codex config bytes inside Codex-owned containment before the first remove or replacement operation. Restore validates containment and writes those local bytes directly while the installer still owns that lock; it never follows links, trusts an unofficial source, or calls the network. Generated ProjectAtlas runtime/MCP/config state remains separately authoritative and is not rolled back by a failed plugin acquisition.

If a terminated updater leaves a recovery snapshot, the next updater fails closed before plugin mutation and retains that state for explicit inspection. Automatic replay is intentionally excluded because the snapshot alone cannot prove whether the prior updater died before or after a successful replacement.

Successful updates, intentionally managed marketplaces, skip controls, and obsolete-MCP handoff keep their existing behavior. Windows and POSIX use equivalent state transitions in their existing scripts rather than a shared framework.

Alternative rejected: remove-then-network-add plus network rollback is not reversible offline. Copying unvalidated or external trees weakens the installer trust boundary.

## Risks / Trade-offs

- [Family-wide hub aggregation regresses latency] -> Use the existing covering index, bounded output, query-plan assertions, and representative cold/warm timing.
- [Strong hubs overrepresent one component] -> Select deterministically by component/branch reach while preserving family seeds and final caps.
- [Missing Git hides a real failure] -> Match only executable-not-found and retain all other typed errors.
- [A persistent host masks another inherited-stdin child] -> Keep the 34-call MCP matrix input open through every response and retain the focused #409 regression.
- [Installer snapshot follows hostile links or escapes ownership] -> Reuse exact official-source and containment validation before snapshot and restore.
- [Restore overwrites newer successful state] -> Hold one cross-process lock per Codex root across inventory, snapshot, mutation, validation, and restore.
- [Concurrent release edits drift] -> Run focused checks first, then exact candidate/full hosted gates and automatic review at one head.

## Migration Plan

No database or protocol migration exists. Land the four fixes and owning regression selectors in the v0.4.3 candidate, run exact-head release proof, install the package, and verify CLI/MCP/TUI/plugin/runtime behavior. Rollback is the previous v0.4.2 code plus the installer-local restoration behavior; no authored database state is removed.

## Open Questions

None. Measured query plans, live TUI counts, installed no-Git behavior, and total-offline installer fault tests are release gates rather than deferred design questions.
