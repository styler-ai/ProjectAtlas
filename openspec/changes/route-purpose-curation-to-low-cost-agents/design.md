## Context

ProjectAtlas emits purpose-curation handoffs through repository guidance, its packaged skill, init, and session brief. Hosts differ: some expose model and reasoning controls, some expose named fixed tiers, and some expose no bounded subagents. Durable product guidance cannot depend on temporary model names, but users benefit from conditional economical examples.

## Goals / Non-Goals

**Goals:**

- Select the lowest reliable reasoning and cost tier supported by the host.
- Delegate through a fixed reliable bounded tier even without a reasoning selector.
- Keep current examples conditional and replaceable as host inventories change.
- Preserve exact bounded queue ownership and ProjectAtlas API-only purpose writes.

**Non-Goals:**

- Require a named model or provider.
- Let the ProjectAtlas runtime spawn agents.
- Expand automatic work beyond low-scope purpose creation/correction.

## Decisions

### 1. Capability is the contract; model names are examples

All durable guidance uses `lowest_reliable_host_supported` and explains both reasoning and cost. Codex Luna-low and Claude Code Haiku are conditional examples only when those tiers are available and reliable. This keeps the contract stable when names change.

Alternative rejected: a fixed model name drifts and fails on other hosts. Purely saying “cheap” omits the reliability boundary.

### 2. Fixed bounded tiers are valid delegation

A host with one reliable isolated bounded subagent tier delegates at that tier even if it exposes no reasoning selector. The main-agent fallback applies only when bounded isolated execution is unavailable. This distinguishes “cannot tune reasoning” from “cannot delegate safely.”

### 3. One rule is projected through all handoffs

Repository/distributed AGENTS guidance, packaged skill, init/session brief, integration/navigation/adoption docs, related OpenSpec contracts, and tests use the same vocabulary. Compact session briefs retain the selection instruction within their existing 4 KiB bound. ProjectAtlas only emits the handoff; the host owns agent selection and execution.

## Risks / Trade-offs

- [Named examples become stale] -> Keep them conditional and retain a generic capability fallback.
- [Lowest cost is mistaken for any low-quality tier] -> Require reliability before cost/reasoning minimization.
- [Fixed-tier hosts fall back unnecessarily] -> Test delegation without a reasoning selector.
- [Guidance surfaces drift] -> Keep one shared E2E/static contract and synchronize related OpenSpec text.

## Migration Plan

No data or protocol migration. Ship the synchronized guidance/runtime text and tests in v0.4.3. Future releases may replace examples without changing the capability contract.

## Open Questions

None.
