## Why

Purpose curation can become repetitive when ProjectAtlas reports missing, stale, or suggested folder/file purposes. The main agent should not spend highest-reasoning context on planned purpose creation or broad correction when the host can delegate bounded work to a low-reasoning subagent.

## What Changes

- Add ProjectAtlas skill guidance that planned folder/file purpose creation and correction, including initial creation and broad refreshes, should be delegated to a low-reasoning subagent when the host supports subagents.
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

## Release Scope

This change is scheduled for v0.3.25 and affects agent workflow guidance only.

## Non-Goals

- Do not assume every host has the same subagent API.
- Do not let agents or subagents edit ProjectAtlas SQLite directly or bypass MCP/CLI purpose APIs.
- Do not require highest-reasoning subagents for mechanical purpose drafts.

## Pre-Mortem

Likely failure modes:
- A subagent invents purposes from filenames without enough context.
- Host-specific wording implies a capability that one host does not expose.
- A delegated subagent mutates SQLite directly or commits changes outside its scope.

Mitigations:
- Delegate planned purpose creation/correction to a low-reasoning subagent when supported, and keep opportunistic along-the-way corrections available to any agent that notices bad metadata.
- Require subagents to use ProjectAtlas summaries/outlines or exact snippets supplied by the main agent.
- Use host-neutral language: "when the host supports subagents."
- Subagents use ProjectAtlas APIs for writes, report evidence, and CI/ProjectAtlas gates validate the result.
