## Context

The README is the GitHub landing page and must explain ProjectAtlas before routing readers to detailed documentation. The v0.4.1 revision already consolidated repeated prose, removed stale comparison material, and added the approved TUI screenshot.

## Goals / Non-Goals

**Goals:**

- Keep one concise Rust-native, local, large-codebase product statement.
- Show the agent-first navigation flow and TUI without duplicating reference documentation.
- Keep every performance statement scoped to its published workload.
- Verify the real GitHub rendering at desktop and narrow widths in light and dark appearances.

**Non-Goals:**

- Redesign the TUI or rerun benchmarks.
- Change runtime, installer, CLI, MCP, or database behavior.

## Decisions

- The README owns product positioning and discovery; detailed procedures remain in their existing documentation owners.
- The repository-owned TUI screenshot is the primary product example and is paired with `projectatlas token --view tui`.
- The large-application audit table stays adjacent to its chart so the workload qualifier is not separated from the result.
- Visual acceptance is based on the rendered GitHub page, not Markdown source alone.

## Risks / Trade-offs

- [Linked documentation drifts] → Keep one owner per detailed topic and validate links.
- [A screenshot becomes stale] → Track future TUI visual changes separately rather than expanding this release correction.
- [Performance wording becomes universal] → Keep the workload qualifier adjacent to each result.
