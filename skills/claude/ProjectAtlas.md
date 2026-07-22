# ProjectAtlas (Claude skill)

## Goal

Give Claude a fast repository atlas before broad search or full-file reads. Use ProjectAtlas to move from
repository overview to folder, file, compressed outline, and exact source only when needed.

## When To Use

- At session start.
- After creating, moving, or deleting folders.
- After adding source files.
- Before refactors or cleanup passes.
- When `projectatlas lint` or `projectatlas health-check` reports drift.

## First-Time Setup

1. Establish the project root first. ProjectAtlas stores one project-local index at `.projectatlas/projectatlas.db`.
2. Install the ProjectAtlas plugin or run the plugin runtime installer from the target project root. Use `cargo install --path crates/projectatlas-cli --locked` only when developing ProjectAtlas from this source checkout. When `codex` is available after a plugin/runtime update, verify `codex mcp get projectatlas` or `codex mcp list`; a stale global `projectatlas` entry for another repo/version is a bug and should be repaired by rerunning the installer. Set `PROJECTATLAS_SKIP_CODEX_MCP_REGISTRY_UPDATE=1` only for intentionally managed global registries.
3. Initialize: `projectatlas init`.
4. Run `projectatlas scan`.
5. Add or import purpose records for important folders and files.
6. Add non-source summaries to `.projectatlas/projectatlas-nonsource-files.toon` when needed.
7. Use `projectatlas purpose review --from-file <json> --apply` for reviewed batches; never edit the SQLite database directly.
8. Run `projectatlas lint --report-untracked --purpose-level low` and fix issues. Use `--purpose-level medium` for all source files and `--purpose-level strict` only when every indexed file and folder must be agent-reviewed.
9. Run `projectatlas map --force` only when an explicit legacy TOON map export is needed.

## Startup Workflow

0. If ProjectAtlas MCP tools are available, use `atlas_*` tools for normal scan, overview, folder, file, summary, search, slice, health, and purpose calls. Use the CLI for bootstrap/install/update/release/CI, MCP config generation, MCP startup debugging, human terminal workflows, or when MCP tools are unavailable.
1. Run ProjectAtlas from the established project root.
2. Run `projectatlas scan` when the SQLite index may be stale.
3. Run `atlas_session_brief` when available. If its purpose handoff is actionable and the host supports bounded subagents, delegate the exact `low`-scope batch to an isolated purpose curator at the lowest reasoning tier the host can enforce while the main task continues. Otherwise process the same batch in the main agent. Copy `task`, `work_key`, and `state_token` into purpose review; never edit SQLite, claim the Rust server spawned an agent, emit successful maintenance chatter, or start `medium`/`strict` implicitly.
4. Run `projectatlas overview`.
5. Run `projectatlas folders <query>` to choose where to work.
6. Run `projectatlas files <query> --folder <path>` to select targets; use `projectatlas files --file-pattern <glob>` when the file/path pattern is already known.
7. Run `projectatlas summary <file> --limit 25` for structured file facts and purpose state.
8. Run `projectatlas outline <file>` if the structured summary is not enough.
9. Run `projectatlas search <pattern> --file-pattern <glob>` for filtered text matches.
10. Run `projectatlas slice <file> --start-line <n> --end-line <m>` for exact source.
11. Run `projectatlas health-check` before cleanup/refactor decisions.
12. Open full source only for selected files or exact slices.
13. Run `projectatlas token` when token-savings reporting is requested; use `projectatlas token --view tui` only for a human terminal dashboard.
14. Correct a wrong, vague, generic, inconsistent, or genuinely repurposed accepted purpose explicitly with `atlas_purpose_set` or `projectatlas purpose set` after inspecting enough context. Purpose entries live in SQLite and remain approved across source/hash/summary/symbol/graph changes. Absent paths leave their purposes dormant, exact-path reappearance restores them, and renames do not transfer approval automatically.

Token savings estimate avoided wrong-folder exploration, wrong-file opens, and unnecessary full-code reads caused by the atlas-first workflow. Agent and MCP surfaces should stay structured by default; the TUI dashboard is explicit terminal UI with "Without PA", "With PA", and "Saved" comparison bars.

Token reports are offline by default. The heuristic is `ceil(chars / 4)` for emitted ProjectAtlas text and `ceil(bytes / 4)` for file-size baselines, labeled as `heuristic_estimate`, not model billing tokens. Check bucket metadata before making claims: `full_file_compression` with `observed` confidence is stronger than modeled `navigation_avoidance` with `inferred` or `policy_estimate` confidence.

## MCP Config

Prefer installer-generated project-local config:

```bash
projectatlas --format json --db .projectatlas/projectatlas.db mcp-config --harness claude-code
```

The ProjectAtlas installer writes `.projectatlas/projectatlas.claude.mcp.json` after verifying `projectatlas --format json runtime-info`. The Claude Code config binds the project through absolute DB/config arguments instead of relying on `cwd`.

## References

- ProjectAtlas repo: https://github.com/styler-ai/ProjectAtlas
- `docs/projectatlas-3-architecture.md` for the target architecture.
- `docs/agent-integration.md` for startup snippets.
- `docs/format.md` for TOON schema.
- `docs/workflow.md` for troubleshooting.
