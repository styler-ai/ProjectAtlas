## Why

Agents need to know MCP startup policy before they make path-sensitive calls. Nearest-project routing, selected DB/config roots, telemetry mode, and scan policy affect whether an absolute path should be served by ProjectAtlas or by normal filesystem tools. That should be exposed as typed MCP state rather than inferred from prose, generated config text, or `runtime-info`.

## What Changes

- Extend `atlas_settings` with an additive typed MCP session capability block.
- Include runtime identity, compiled MCP tools, selected project identity, nearest-project startup policy, path-scope policy, scan policy, telemetry mode, and no-secret guarantees.
- Keep CLI `runtime-info` identity-only; MCP startup policy must not leak into the CLI runtime-info contract.
- Represent missing index/config state with typed status values instead of optimistic prose.

## Capabilities

### New Capabilities
- `mcp-capability-settings`: Defines the typed MCP capability/settings contract for startup flags, selected project identity, scan policy, telemetry mode, path routing, and runtime identity.

### Modified Capabilities
- `atlas_settings` returns additive MCP session policy fields.

## Release Scope

This change is scheduled for the next version. The implementation chooses additive `atlas_settings` fields instead of a new tool to keep the MCP surface smaller while preserving existing lower-level settings output.

## Non-Goals

- Do not mutate selected project state, scan, or repair config from settings.
- Do not expose secrets, token values, arbitrary environment variables, or unrelated user profile data.
- Do not put MCP startup policy into CLI `runtime-info`.
- Do not remove existing settings fields.

## Pre-Mortem

Likely failure modes:
- Capability fields duplicate runtime-info and drift.
- Startup policy is emitted as prose and harnesses still have to parse text.
- Settings accidentally expose environment secrets.
- Missing-index state creates the index while trying to inspect it.
- Existing settings consumers break because top-level fields are renamed.

Mitigations:
- Source runtime fields from `build_runtime_info()` and policy fields from `ProjectAtlasMcpServer`.
- Use enum-backed serialized fields with stable snake_case values.
- Keep the change additive under a nested MCP session/capabilities block.
- Add tests for nearest-project on/off, missing index, no-secret output, and CLI runtime-info separation.
