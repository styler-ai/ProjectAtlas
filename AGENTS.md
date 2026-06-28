# Codex Agent Instructions

## Core Rules (All Projects)
- Keep code changes minimal, clear, and aligned with SOLID / KISS / DRY principles.
- Prefer established frameworks or libraries; document any exception explicitly.
- Add docstrings or comments only when intent is not obvious; avoid redundant narration.
- Enforce strong logging, error handling, and security through input validation, environment-based secrets, HTTPS assets, and OWASP-aware design.
- Follow conventional commits, for example `feat(core): ...`, and link the relevant GitHub issue when committing.
- Run or describe appropriate tests with explicit timeouts; never leave commands hanging.
- Use MCP tooling before CLI/manual steps when an MCP surface is available; close sessions when finished.
- For interactive browser or Electron debugging and manual UI QA loops, use the global `playwright-interactive` skill by default.
- Keep documentation and specs in sync with behavior; update decision records as needed.
- Ask for clarification when requirements conflict; document assumptions and blockers.

## ProjectAtlas Workflow
1. Establish the project root and run ProjectAtlas from that root so `.projectatlas/projectatlas.db` is project-local.
2. Run `projectatlas scan` or `projectatlas map --force` when the index may be stale.
3. Run `projectatlas overview` to orient on the repository.
4. Run `projectatlas folders <query>` before choosing a work area.
5. Run `projectatlas files <query> --folder <path>` before opening source; use `projectatlas files --file-pattern <glob>` when the file/path pattern is already known.
6. Run `projectatlas summary <file> --limit 25` for structured file facts and purpose state.
7. Run `projectatlas outline <file>` if the summary is not enough.
8. Run `projectatlas search <pattern> --file-pattern <glob>` for bounded glob-filtered text matches; add `--fuzzy` for approximate names and inspect returned, searched file, searched byte, and truncated counters before widening the search.
9. Run `projectatlas slice <file> --start-line <n> --end-line <m>` or `projectatlas symbols slice <file> <symbol> --symbol-parent <parent> --symbol-kind <kind> --symbol-line <line>` for exact source; add disambiguators when duplicate symbol names exist.
10. Run `projectatlas health-check` for cleanup/refactor work.
11. Run `projectatlas lint --strict-folders --report-untracked`.
12. Only then use language-server lookups or broad file reads on selected files.
13. Run `projectatlas runtime-info` when installer/runtime identity is unclear.
14. Run `projectatlas token` when asked for token savings.

## Rust/Dependency Discipline
- Prefer official or canonical Rust crates and standard implementations for protocols, formats, parsers, storage, watchers, token tooling, and platform integration before writing custom code.
- Keep custom Rust code focused on ProjectAtlas-specific product logic, agent workflow policy, and composition between proven libraries.
- Document any exception to the official/canonical crate preference and cover it with tests.
- Architecture reviews must explicitly check for unnecessary reimplementation, cross-platform path issues, performance bottlenecks, and drift from agent-first ProjectAtlas workflows.
