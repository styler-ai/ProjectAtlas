## Context

The nearest-project routing work introduces a startup-level policy that affects how absolute paths are interpreted. Agents and harnesses need a deterministic way to inspect this policy and other MCP server capabilities at runtime instead of inferring behavior from config files or human text.

## Goals / Non-Goals

**Goals:**
- Expose startup flags and effective project identity through a typed MCP response.
- Make route-affecting policy inspectable before agents call summary, slice, search, or file ranking tools.
- Keep the response stable enough for JSON/TOON snapshot tests.

**Non-Goals:**
- Do not expose secrets or environment variables.
- Do not make the settings response mutate project selection or config.
- Do not replace `runtime-info`; this is MCP session policy, not just binary identity.

## Decisions

- Prefer a new MCP tool or an extended existing settings tool only after API review. A separate tool avoids breaking existing consumers; extending `atlas_settings` avoids surface sprawl.
- Model policy fields with enums/booleans rather than diagnostics. For example, `nearest_project: enabled|disabled`, `telemetry: enabled|disabled`, and `path_scope: selected_project|nearest_indexed_project`.
- Include both configured startup policy and effective selected project state so harnesses can diagnose stale generated configs.

## Risks / Trade-offs

- Adding a new tool increases MCP surface area -> keep it narrow and documented.
- Extending `atlas_settings` may surprise existing tests -> use additive fields only.
- Capability data may drift from startup args -> source fields directly from server state and existing runtime/config helpers.
