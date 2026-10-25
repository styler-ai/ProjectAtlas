## Why

Purpose curation can become repetitive when ProjectAtlas reports missing, stale, or suggested folder/file purposes. The main agent should not spend highest-reasoning context on planned purpose creation or broad correction when the host can delegate bounded work to its lowest reliable reasoning and cost tier.

## What Changes

- Add ProjectAtlas skill guidance that planned folder/file purpose creation and correction, including initial creation and broad refreshes, should be delegated at the lowest reliable reasoning and cost tier supported by the host when it supports subagents.
- Align the shared init/session-brief `purpose_handoff` recommendation and instructions with the same reliable host-relative rule.
- Allow current host/model names only as conditional examples after that durable capability-based rule, with a generic fallback as names and availability change.
- Give the subagent bounded ProjectAtlas queue rows, summaries, outlines, or exact snippets.
- Treat purposes written by an agent or subagent through ProjectAtlas purpose APIs as agent-approved, with no second approval pass.
- Allow any agent that notices a wrong, stale, vague, or generic purpose during normal work to correct it along the way with the same ProjectAtlas APIs.
- Forbid agents and subagents from editing SQLite directly.

## Capabilities

### New Capabilities
- `bulk-purpose-curation-delegation`: Defines when and how agents should delegate purpose creation and correction.

### Modified Capabilities
- Packaged ProjectAtlas skill.
- Agent integration documentation and startup snippets.
- Init/session-brief purpose-curator handoff metadata.

## Release Scope

This workflow was introduced in v0.3.25. The reliability qualification, conditional examples, and matching host-owned init/session-brief handoff metadata ship in v0.4.3; the Rust server still does not execute curator work.

## Non-Goals

- Do not assume every host has the same subagent API.
- Do not make any host or model name a hard universal requirement.
- Do not let agents or subagents edit ProjectAtlas SQLite directly or bypass MCP/CLI purpose APIs.
- Do not require highest-reasoning subagents for mechanical purpose drafts.

## Pre-Mortem

Likely failure modes:
- A subagent invents purposes from filenames without enough context.
- Host-specific wording implies a capability that one host does not expose.
- A delegated subagent mutates SQLite directly or commits changes outside its scope.

Mitigations:
- Delegate planned purpose creation/correction at the lowest reliable reasoning and cost tier supported by the host, and keep opportunistic along-the-way corrections available to any agent that notices bad metadata.
- Require subagents to use ProjectAtlas summaries/outlines or exact snippets supplied by the main agent.
- Keep the normative rule host-neutral and capability-based; any current host/model examples remain conditional and retain a generic fallback.
- Subagents use ProjectAtlas APIs for writes, report evidence, and CI/ProjectAtlas gates validate the result.
