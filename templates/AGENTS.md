# ProjectAtlas Startup Snippet

## Startup
1. Establish the project root and run ProjectAtlas from that root so `.projectatlas/projectatlas.db` is project-local.
2. Run `projectatlas scan` or `projectatlas map --force` when the index may be stale.
3. Run `projectatlas overview` to orient on the repository.
4. Run `projectatlas folders <query>` before choosing a work area.
5. Run `projectatlas files <query> --folder <path>` before opening source; use `projectatlas files --file-pattern <glob>` when the file/path pattern is already known.
6. Run `projectatlas summary <file> --limit 25` for structured file facts and purpose state.
7. Run `projectatlas outline <file>` if the summary is not enough.
8. Run `projectatlas search <pattern> --file-pattern <glob>` for filtered text matches.
9. Run `projectatlas slice <file> --start-line <n> --end-line <m>` for exact source.
10. Run `projectatlas health-check` for cleanup/refactor work.
11. Run `projectatlas lint --strict-folders --report-untracked`.
12. Only then use symbol-level indexing, language-server lookups, or broad file reads on selected files.
13. Run `projectatlas token` when asked for token savings.

## Rust/Dependency Discipline
- Prefer official or canonical Rust crates and standard implementations for protocols, formats, parsers, storage, watchers, token tooling, and platform integration before writing custom code.
- Keep custom Rust code focused on ProjectAtlas-specific product logic, agent workflow policy, and composition between proven libraries.
- Document any exception to the official/canonical crate preference and cover it with tests.
- Architecture reviews must explicitly check for unnecessary reimplementation, cross-platform path issues, performance bottlenecks, and drift from agent-first ProjectAtlas workflows.
