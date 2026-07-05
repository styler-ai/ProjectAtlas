## Context

ProjectAtlas purpose quality improves during normal work, but planned folder/file purpose creation and correction can consume agent context. A low-reasoning subagent can own bounded purpose creation or refresh work using ProjectAtlas queue items, summaries, and bounded slices. When assigned that ownership, the subagent may apply purposes through `atlas_purpose_set`, `atlas_purpose_review`, `projectatlas purpose set`, or `projectatlas purpose review` and report the changed paths plus commands it ran.

This belongs in the ProjectAtlas skill because it is an agent workflow optimization, not a runtime behavior change.

## Contract

The ProjectAtlas skill SHALL guide agents to:

- delegate planned folder/file purpose creation and correction to a low-reasoning subagent when the host supports subagents,
- include initial purpose creation and broad purpose refreshes in that delegation rule,
- give delegated subagents bounded inputs: queue rows, summaries, outlines, or exact snippets,
- allow delegated subagents to apply purposes only through ProjectAtlas MCP/CLI purpose APIs when the work is delegated to them,
- treat purposes written by an agent or subagent through ProjectAtlas purpose APIs as approved,
- let any agent correct wrong, stale, vague, or generic purposes along the way when it notices them during normal work.

## Implementation Notes

- Use "when the host supports subagents" wording without assuming identical APIs.
- Avoid setting a hard universal model name; recommend low-reasoning subagents relative to the main task.
- Include examples of batch triggers:
  - several missing/stale purposes,
  - release-driven purpose replay updates,
  - large moved/added folder sets.

## Edge Cases

- Opportunistic wrong purpose noticed during normal work: the observing agent fixes it along the way with ProjectAtlas purpose APIs.
- Ambiguous file role: provide narrower ProjectAtlas context to the subagent or inspect exact context before writing a purpose.
- No subagent support: the current agent completes the curation directly.
- Delegated subagent returns uncertain or conflicting purposes: narrow the context and rerun a focused subagent pass or decide directly.

## Pre-Mortem

Risk: delegated drafts become low-quality generic labels.
Mitigation: require specific one-line purposes tied to actual path role and reject vague descriptions.

Risk: the approval rule lets bad purpose text persist.
Mitigation: treat later corrections as normal work; if any agent sees wrong, stale, vague, or generic purpose text, it corrects it along the way with ProjectAtlas purpose APIs.

Risk: host-specific names age badly.
Mitigation: phrase the rule around supported subagent capability without naming a model or host-specific API.
