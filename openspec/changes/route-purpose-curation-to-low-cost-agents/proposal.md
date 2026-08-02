## Why

Purpose curation is bounded structured metadata work that should use the host's lowest reliable reasoning and cost tier, including fixed bounded subagent tiers that expose no reasoning selector. Durable guidance must remain capability-based while conditional examples help current Codex and Claude Code users choose economical models.

## What Changes

- Require the lowest reliable host-supported reasoning and cost tier for bounded purpose creation/correction.
- Allow one fixed reliable bounded subagent tier to delegate without a reasoning selector; fall back to the main agent only when bounded isolated subagents are unavailable.
- Keep model names conditional examples rather than product invariants, including Codex Luna with low reasoning and Claude Code Haiku when available.
- Synchronize repository/distributed guidance, packaged skill, init/session-brief handoffs, integration/navigation/adoption docs, active overlapping OpenSpec contracts, Mermaid views, and owning tests.
- Preserve bounded non-overlapping queue ownership, ProjectAtlas purpose APIs, immediate agent approval, and quiet maintenance.

## Capabilities

### New Capabilities

- `reliable-low-cost-purpose-curation`: Host-neutral delegation selection, conditional examples, fallback semantics, and synchronized runtime/documentation contracts for purpose curation.

### Modified Capabilities

None. The behavior was landed as v0.4.3 guidance/runtime contract refinement and does not rename an existing synchronized main spec.

## Impact

- Completed v0.4.3 work in repository and distributed AGENTS guidance, the packaged ProjectAtlas skill, init/session-brief handoffs, integration/navigation/adoption docs, related #305/#308 OpenSpec text, diagrams, and owning Rust/MCP/package tests.
- No model dependency, network call, spawned agent inside ProjectAtlas, database schema, MCP tool schema, or new runtime abstraction.

## Non-Goals

- Making Luna, Haiku, Sonnet, or another named model mandatory.
- Letting ProjectAtlas itself spawn or bill an agent.
- Delegating overlapping or unbounded purpose queues.
- Replacing the main-agent fallback when the host exposes no bounded isolated subagent execution.
