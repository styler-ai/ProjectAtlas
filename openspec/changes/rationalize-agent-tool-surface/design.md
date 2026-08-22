## Context

ProjectAtlas v0.5.0 is planned to install one complete `atlas <command>` surface while preserving canonical `projectatlas` automation and MCP. That changes the optimization question for the public agent interface: the decision is no longer whether CLI or MCP can expose a behavior, but which route minimizes discovery, calls, latency, wrong reads, recovery cost, and compatibility risk for each real task.

Issue #310 contains historical inventories and directional tool-count evidence, but the v0.6 decision must use the actual installed stable v0.5 surface. Issue #314 needs the stabilized surface so Memory Atlas can add authored project continuity without duplicating task, issue, source, graph, or host-memory authority.

## Goals / Non-Goals

**Goals:**

- Establish a reproducible installed v0.5 baseline for every public CLI command, MCP tool/schema, skill route, generated host contract, and representative agent workflow.
- Select the smallest evidence-backed mixed surface: concise CLI for ordinary one-shot shell work and MCP for capabilities where its typed session boundary materially helps.
- Give every public route an explicit disposition and keep retained schemas, selectors, errors, freshness, output bounds, and project isolation concrete.
- Deliver any incompatible change as one versioned runtime/plugin/skill/host contract with real migration and rollback proof.

**Non-Goals:**

- A predetermined maximum tool count or percentage reduction.
- CLI-only or MCP-only policy, a generic administration multiplexer, or removal based only on observed absence.
- Changes to underlying graph, freshness, purpose, repository-navigation, or administrative semantics except where an adapter contract must be versioned explicitly.
- Any SQLite schema, authored/derived-state, or freshness-model change merely to support route classification; an unavoidable persistence compatibility delta requires its own database-first review.
- Memory Atlas, TUI, GUI, or visualization implementation.

## Decisions

### The installed stable v0.5 product is the baseline

The evaluation SHALL pin one released v0.5 version and record runtime, plugin, skill, host configuration, CLI help/inventory, MCP `tools/list`, concrete schemas, serialized discovery bytes, and representative task outcomes. Repository revisions, warm/cold state, project roots, prompts, timeouts, outputs, and accounting rules remain identical across compared routes.

Alternative considered: reuse the historical v0.4 40-tool inventory. Rejected because v0.5 adds a complete installed CLI and may change parity, parser, routing, or task behavior.

### Route by task capability, not transport loyalty

Ordinary completed shell operations use `atlas ...` by default because the concise installed command is directly inspectable, composable, and does not require a persistent tool session. MCP remains preferred for no-shell hosts, typed tool discovery, persistent multi-client routing, session-local task status/cancellation, or another measured capability where it improves task success or cost.

Alternative considered: teach both routes equally for every task. Rejected because it expands skill text and selection ambiguity without adding behavior. A transport remains available where supported even when it is not the default teaching route.

### Every public route receives one evidence-backed disposition

The frozen inventory is classified as retained, merged into an existing typed response, CLI-owned, automatic backend behavior, or intentionally removed. The classification includes aliases, skill references, fixtures, host configuration, compatibility tests, and migration effects. Unobserved use alone cannot justify removal, and no action-string multiplexer may hide unrelated schemas.

### Typed behavior and isolation outrank inventory size

Retained MCP schemas use concrete typed fields rather than `unknown` arrays or broad action strings. Retained CLI routes preserve help, arguments, structured formats, stdout/stderr, exit codes, explicit project roots, and safe administrative confirmation. Wrong-root and missing-index inputs return typed results without implicit initialization or mutation. Per-call project paths remain the concurrency-safe shared-host route.

### Task status and cancellation require a real lifecycle

MCP task status and cancellation remain only if an installed normal operation produces a bounded task identifier, exposes progress, accepts cancellation, and proves exact process/database cleanup. Otherwise the pair is removed together rather than preserving an unused speculative contract.

### Runtime, plugin, skill, fixtures, and hosts migrate atomically

The Rust router, CLI help, frozen inventory, plugin skill, generated host configuration, installer validation, docs, release notes, and installed E2E change in one versioned boundary. Old tool names receive standard unknown-tool behavior unless an explicitly specified compatibility alias is justified. Rollback restores the last compatible complete runtime/plugin pair; it never mixes a new skill with an old inventory.

### The release hierarchy separates implementation from final acceptance

#310 is a direct native sub-issue of #493 and has no direct blocker. #314 is another direct #493 child but is blocked by #310. #493 is blocked by both, owns the final complete installed public-surface regression and holistic #310/#314 E2E, implements no feature/bug, and closes last. The hierarchy exposes version progress; direct blockers determine execution order.

## Risks / Trade-offs

- [Risk] A rarely used but critical MCP tool is removed because local telemetry did not observe it. → Mitigation: combine static inventory, packaged workflow tests, explicit administrative contracts, representative agent evaluation, and maintainer review; absence is evidence, not proof.
- [Risk] CLI-first guidance loses MCP root safety or typed errors. → Mitigation: require explicit roots, machine formats, bounded outputs, typed exit/error contracts, and wrong-root/missing-index/no-mutation tests for both routes.
- [Risk] Tool count drops while schema bytes or task ambiguity stay high. → Mitigation: measure complete serialized discovery and real tasks; reject hidden action multiplexers and oversized merged responses.
- [Risk] Runtime, plugin, skill, and host configuration drift during a breaking change. → Mitigation: package and install them as one versioned contract and read back real host/runtime identity.
- [Risk] Evaluation overfits one repository or model. → Mitigation: use several representative public or synthetic repositories and tasks, preserve failures, report workload limitations, and avoid universal savings claims.

## Migration Plan

1. Finish and accept stable v0.5.0, then freeze its installed CLI/MCP/plugin/host baseline.
2. Run identical CLI-first, MCP-first, and mixed task evaluations and ratify every public route.
3. Change the smallest adapter, schema, skill, fixture, installer, and documentation surface required by the accepted dispositions.
4. Run real stdio MCP, installed CLI, generated-host, concurrent-root, migration, and rollback E2E on supported platforms.
5. Accept #310, then refresh/rebase and begin #314 only against that surface.
6. Let #493 compose the accepted issues through complete installed public-surface and holistic release proof; rollback reinstalls the last complete compatible runtime/plugin release, never mixed-version artifacts.

## Dependencies / Cross-Issue Impact

- #491 and #492 establish and release the complete v0.5 installed CLI/MCP baseline consumed here.
- #310 solely owns this agent-surface decision and its compatibility migration.
- #314 depends on this issue and implements Memory Atlas only after the public surface is stable.
- #493 is the native parent/release-acceptance issue, is directly blocked by #310 and #314, implements neither contract, and closes last.
- #357, #369, and #466 remain unversioned visual backlog and do not influence the transport decision.

## Open Questions

None.
