# Project Brief

Purpose: High-level summary of ProjectAtlas goals and scope.

- ProjectAtlas provides an agent-first structure map and lint gate for repository hygiene.
- ProjectAtlas 3 target is Rust-only. Legacy Python implementation/tooling must be removed or replaced by Rust before the migration is considered complete.
- Strategic direction is ProjectAtlas 3: a Rust-native repository intelligence engine that merges ProjectAtlas structural purpose indexing with complete repository indexing/search/symbol intelligence.
- ProjectAtlas 3 must avoid source pollution: no required Purpose headers in source files and no required `.purpose` files in folders.
- ProjectAtlas 3 should store structural purpose, symbol index data, health findings, and usage telemetry in SQLite.
- ProjectAtlas 3 should reduce agent token usage through a progressive context funnel: repo overview, folder choice, file choice, compressed details, exact code only when needed.
- ProjectAtlas 3 must support very large codebases efficiently through Rust-native scanning, indexing, query, and incremental refresh behavior.
- ProjectAtlas 3 must support every language/file type in the repository-intelligence parity set through specialized parsers or fallback indexing.
- Code Index MCP behavior is a minimum baseline, not the finish line. ProjectAtlas 3 must provide at least the same practical repository-intelligence capabilities, then improve them with Rust performance, atlas-first navigation, TOON-first agent payloads, purpose governance, health cleanup, token telemetry, and better large-repo ergonomics.
- ProjectAtlas 3 must distinguish deterministic observed summaries from intent-bearing purposes. Folder purpose describes the folder's structural responsibility; file purpose describes why the file exists in that structure. Generated file-purpose guesses may be stored only as suggestions until an agent reviews and approves or corrects them.
- ProjectAtlas 3 must keep Code Index-inspired behavior as clean-room, ProjectAtlas-native implementation: names, commands, storage model, MCP tools, and source organization must stay ProjectAtlas-specific while covering and exceeding the useful behavior.
- ProjectAtlas 3 completion requires agent-usable end-to-end behavior, not only CLI availability: plugin install, MCP runtime, skill instructions, project-root setup, SQLite index state, TOON payloads, health gates, and token telemetry must all work together.
