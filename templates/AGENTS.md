# ProjectAtlas Startup Snippet

## Startup
0. If ProjectAtlas MCP tools are available, use `atlas_*` tools for normal scan, overview, folder, file, summary, search, slice, health, and purpose calls. Use the CLI for bootstrap/install/update/release/CI, MCP config generation, MCP startup debugging, human terminal workflows, or when MCP tools are unavailable.
1. Establish the project root and run ProjectAtlas from that root so `.projectatlas/projectatlas.db` is project-local.
2. Run `projectatlas scan` when the SQLite index may be stale.
3. Run `projectatlas overview` to orient on the repository.
4. Run `projectatlas folders <query>` before choosing a work area.
5. Run `projectatlas files <query> --folder <path>` before opening source; use `projectatlas files --file-pattern <glob>` when the file/path pattern is already known.
6. Run `projectatlas summary <file> --limit 25` for structured file facts and purpose state.
7. Run `projectatlas outline <file>` if the summary is not enough.
8. Run `projectatlas search <pattern> --file-pattern <glob>` for filtered text matches.
9. Run `projectatlas slice <file> --start-line <n> --end-line <m>` for exact source.
10. Run `projectatlas health-check` for cleanup/refactor work.
11. Run `projectatlas lint --report-untracked --purpose-level low`; low fails stale, duplicate, and temporary-folder health but keeps first-pass purpose curation advisory. Use `projectatlas purpose queue` for the next curation actions, `--purpose-level medium` when all source files must be agent-reviewed, and `--purpose-level strict` only when every indexed file and folder must be agent-reviewed.
12. Only then use symbol-level indexing, language-server lookups, or broad file reads on selected files.
13. Run `projectatlas token` when asked for token savings.
14. After ProjectAtlas plugin/runtime updates, verify Codex global MCP registry drift with `codex mcp get projectatlas` or `codex mcp list` when `codex` is available. A stale global `projectatlas` entry pointing at an old version or another repo is a bug; rerun the ProjectAtlas installer so it repairs the registry automatically. Use `PROJECTATLAS_SKIP_CODEX_MCP_REGISTRY_UPDATE=1` only for intentionally managed global registries.
15. Correct wrong, stale, vague, or generic purposes opportunistically with `atlas_purpose_set` or `projectatlas purpose set` after inspecting enough context. Use `atlas_purpose_review` or `projectatlas purpose review --from-file <json> --apply` for reviewed batches; never edit SQLite directly. Purpose entries live in SQLite and are preserved across scans; changed approved files become stale instead of losing their curated purpose text.

## Rust/Dependency Discipline
- Prefer official or canonical Rust crates and standard implementations for protocols, formats, parsers, storage, watchers, token tooling, and platform integration before writing custom code.
- Keep custom Rust code focused on ProjectAtlas-specific product logic, agent workflow policy, and composition between proven libraries.
- Document any exception to the official/canonical crate preference and cover it with tests.
- Architecture reviews must explicitly check for unnecessary reimplementation, cross-platform path issues, performance bottlenecks, and drift from agent-first ProjectAtlas workflows.
