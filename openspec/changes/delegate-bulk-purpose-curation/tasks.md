## 1. Spec and Issue Setup

- [x] 1.1 Create the OpenSpec proposal, design, spec delta, and task list with pre-mortem risks.
- [x] 1.2 Create GitHub issue #301, assign it to v0.3.25, and map `delegate-bulk-purpose-curation` in `openspec/issue-map.json`.
- [x] 1.3 Mirror this task list into #301 under `OpenSpec Tasks`.

## 2. Skill And Documentation

- [x] 2.1 Update the packaged ProjectAtlas skill with low-reasoning subagent purpose creation/correction guidance.
- [x] 2.2 Update agent integration docs and startup snippets with the same bounded rule.
- [x] 2.3 Keep the skill and shared init/session-brief handoff rule host-neutral and capability-based; allow Codex `gpt-5.6-luna` with `low` reasoning and Claude Code `haiku` only as conditional skill examples when available after that rule, reject hard universal model names, and preserve a generic fallback as names and availability change.

## 3. Verification

- [x] 3.1 Run OpenSpec validation and issue checklist validation.
- [x] 3.2 Run ProjectAtlas scan, purpose replay, and strict lint after updating purpose metadata.
