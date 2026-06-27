# Product Context

Purpose: Describe the user problem ProjectAtlas solves and the target workflow.

- Agents and humans need a quick structural overview before deep indexing.
- The atlas guides file selection and keeps folder intent explicit over time.
- Enterprise users need this without modifying source files or adding metadata files across product folders.
- ProjectAtlas 3 should become the native context funnel for Codex, Claude Code, OpenCode, and other MCP-aware coding harnesses.
- Target workflow: first identify the correct folder, then identify the correct file, then inspect compressed file details, then request exact code slices only if required.
- The agent experience must be seamless on large repositories: get a project atlas, choose where to look, inspect folder/file purpose, inspect source summaries, then land on exact code slices with minimal token waste.
- ProjectAtlas must merge structural orientation and code intelligence into one workflow: index folders, file purposes, source summaries, symbols, relationships, search results, and exact slices without forcing the agent to switch tools.
- File summaries must be structured deterministic intelligence, not vague prose. The deep summary surface should include file path, language, line count, purpose state, observed one-line summary, imports/dependencies, functions, methods, classes/types, signatures, line ranges, and call/called-by data where available.
- Repository-intelligence summaries must at least cover the practical Code Index file-summary data shape: language, line count, imports, exports, file/symbol docstrings where parsers can extract them, functions, methods, classes/types, signatures, line ranges, and called-by/call relationships. ProjectAtlas may expose this through richer ProjectAtlas-native fields rather than Code Index names.
- Folder purpose and file purpose are different product concepts. Folder purpose is an agent-approved responsibility statement for a directory; file purpose is an agent-approved reason the file exists within the selected folder. Folder purposes should not be auto-approved from syntax alone.
- ProjectAtlas setup and plugin workflow must establish the project root first. Each repository/project gets its own `.projectatlas/projectatlas.db`; ProjectAtlas is not a single global index shared across unrelated projects.
- All supported programming languages/file families from the repository-intelligence parity target must be recognized and indexed; unsupported deep parsing falls back to useful language/file metadata and text search rather than becoming invisible.
- ProjectAtlas should report estimated token savings for each usage/session so users can see how much context the funnel avoided: wrong-folder exploration, wrong-file opens, and unnecessary full-code reads.
- Token-savings UX should be harness-neutral: canonical CLI `projectatlas token`, canonical MCP `atlas_token_report`, with possible harness aliases such as `/projectAtlas:token`.
- Human-facing token UX should be explicit: `projectatlas token --view tui` can render a nice terminal dashboard, but agent and MCP defaults stay structured for reliable harness consumption.
- Health checks should make structural drift visible: duplicated folders, repeated temp/generated asset roots, overlapping purposes, duplicated files/classes/functions/methods, stale purposes, and DRY violations.
- A correct purpose is more important than an automatically filled field. Generated summaries may be deterministic and immediately useful; generated purposes should remain review-required suggestions until the agent verifies the selected folder/file context and approves or corrects them.
