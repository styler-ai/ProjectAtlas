## 1. Spec and Issue Setup

- [x] 1.1 Update the capability/settings OpenSpec proposal, design, spec delta, and task list for v0.3.25 implementation.
- [x] 1.2 Mirror this task list into #293 under `OpenSpec Tasks` and assign the release milestone.

## 2. MCP Implementation

- [x] 2.1 Add typed MCP session capability structs and enum-backed policy fields.
- [x] 2.2 Extend `atlas_settings` additively with runtime identity, selected project identity, nearest-project policy, path scope, scan policy, telemetry mode, and privacy guarantees.
- [x] 2.3 Preserve CLI `runtime-info` separation and existing settings fields.

## 3. Tests and Documentation

- [x] 3.1 Add tests for nearest-project enabled, nearest-project disabled, missing index, no-secret output, and runtime-info separation.
- [x] 3.2 Update MCP setup docs and ProjectAtlas skill instructions.
- [x] 3.3 Run OpenSpec, issue-checklist, MCP, and Rust verification gates.
