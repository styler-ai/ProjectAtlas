# Memory Bank Extension Decision

Issue #74 asked whether ProjectAtlas should grow into a broader memory-bank extension. The current ProjectAtlas 3 decision is to keep the core focused on repository intelligence and agent orientation.

ProjectAtlas owns:

- Project-local folder and file purpose state.
- Deterministic observed summaries.
- Deep source symbols, relations, search, slices, health findings, and token telemetry.
- MCP and CLI surfaces that help Codex, OpenCode, and Claude Code choose where to inspect next.

A memory-bank integration can consume ProjectAtlas output, but it should remain an integration layer unless it needs project-local repository facts that belong in the atlas database. This boundary keeps ProjectAtlas fast, portable, and usable across agent harnesses without coupling it to one memory-bank convention.
