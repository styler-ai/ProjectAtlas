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
- Do not turn local editor, agent, cache, or workspace-state folder names into product invariants. ProjectAtlas honors `.gitignore` dynamically, and the ProjectAtlas ignore config is only the stricter atlas-specific layer applied after `.gitignore`.

## ProjectAtlas Workflow
0. If ProjectAtlas MCP tools are available, use `atlas_*` tools for normal scan, overview, folder, file, summary, search, slice, health, and purpose calls. Use the CLI for bootstrap/install/update/release/CI, MCP config generation, MCP startup debugging, human terminal workflows, or when MCP tools are unavailable.
1. Establish the project root and run ProjectAtlas from that root so `.projectatlas/projectatlas.db` is project-local.
2. Run `projectatlas scan` when the SQLite index may be stale.
3. Run `projectatlas overview` to orient on the repository.
4. Run `projectatlas folders <query>` before choosing a work area.
5. Run `projectatlas files <query> --folder <path>` before opening source; use `projectatlas files --file-pattern <glob>` when the file/path pattern is already known.
6. Run `projectatlas summary <file> --limit 25` for detailed file facts: `file_purpose`, `content_summary`, parser state, imports, symbols, calls, and counts; inspect `parser_kind` and `summary_status` before trusting the `content_summary`.
7. Run `projectatlas outline <file>` if the summary is not enough.
8. Run `projectatlas search <pattern> --file-pattern <glob>` for bounded glob-filtered text matches; add `--fuzzy` for approximate names and inspect returned, searched file, searched byte, and truncated counters before widening the search.
9. Run `projectatlas slice <file> --start-line <n> --end-line <m>` or `projectatlas symbols slice <file> <symbol> --symbol-parent <parent> --symbol-kind <kind> --symbol-line <line>` for exact source; add disambiguators when duplicate symbol names exist.
10. Run `projectatlas health-check` for cleanup/refactor work.
11. Run `projectatlas lint --report-untracked --purpose-level low`; low fails stale, duplicate, and temporary-folder health but keeps first-pass purpose curation advisory. Use `projectatlas purpose queue` for the next curation actions, `--purpose-level medium` when all source files must be agent-reviewed, and `--purpose-level strict` only when every indexed file and folder must be agent-reviewed.
12. Only then use language-server lookups or broad file reads on selected files.
13. Run `projectatlas config --print` when effective scan, purpose, or exclusion policy is unclear.
14. Run `projectatlas ignore list` before adding repository-specific atlas excludes; `.gitignore` is inherited dynamically and manual ProjectAtlas ignores are applied after it as stricter atlas-only exclusions. Use `projectatlas ignore init-gitignore` only when a project needs a missing project-root `.gitignore` created. Keep personal/local workspace state in `.gitignore`.
15. Run `projectatlas runtime-info` when installer/runtime identity is unclear.
16. Run `projectatlas token` when asked for token savings; use `projectatlas token --view tui` only when a human asks for the terminal dashboard.
17. Generate harness MCP config with `projectatlas --format json --db .projectatlas/projectatlas.db mcp-config`, adding `--harness claude-code` or `--harness opencode` for those hosts. Prefer installer-generated project-local configs over checked-in fallback templates.
18. Correct wrong, stale, vague, or generic purposes opportunistically with `atlas_purpose_set` or `projectatlas purpose set` after inspecting enough context. Purpose entries live in SQLite and are preserved across scans; changed approved files become stale instead of losing their curated purpose text.

## Rust/Dependency Discipline
- Prefer official or canonical Rust crates and standard implementations for protocols, formats, parsers, storage, watchers, token tooling, and platform integration before writing custom code.
- Keep custom Rust code focused on ProjectAtlas-specific product logic, agent workflow policy, and composition between proven libraries.
- Document any exception to the official/canonical crate preference and cover it with tests.
- Architecture reviews must explicitly check for unnecessary reimplementation, cross-platform path issues, performance bottlenecks, and drift from agent-first ProjectAtlas workflows.
