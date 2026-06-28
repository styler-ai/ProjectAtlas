# ProjectAtlas (Codex skill)

## Goal

Give Codex a fast repository atlas before broad search or full-file reads. Codex should use ProjectAtlas to choose
the folder, choose the file, inspect compressed context, and only then open exact source.

## When To Use

- At session start.
- After creating, moving, or deleting folders.
- After adding source files.
- Before cleanup, refactor, or architecture work.
- When `projectatlas lint` or `projectatlas health-check` reports drift.
- When the user asks for ProjectAtlas token savings.

## First-Time Setup

1. Establish the project root first. ProjectAtlas stores one project-local index at `.projectatlas/projectatlas.db`.
2. Install the Rust binary if missing: `cargo install --path crates/projectatlas-cli --locked`.
3. Initialize: `projectatlas init --seed-purpose`.
4. Run `projectatlas scan`.
5. Add or import purpose records for important folders and files.
6. Add non-source summaries to `.projectatlas/projectatlas-nonsource-files.toon` when needed.
7. Run `projectatlas map --force`.
8. Run `projectatlas lint --strict-folders --report-untracked` and fix issues.

## Startup Workflow

1. Run ProjectAtlas from the established project root.
2. Run `projectatlas scan` or `projectatlas map --force` when the index may be stale.
3. Run `projectatlas overview`.
4. Run `projectatlas folders <query>` to choose the right area.
5. Run `projectatlas files <query> --folder <path>` to choose targets; use `projectatlas files --file-pattern <glob>` when the file/path pattern is already known.
6. Run `projectatlas summary <file> --limit 25` before opening full source.
7. Run `projectatlas outline <file>` if the structured summary is not enough.
8. Run `projectatlas search <pattern> --file-pattern <glob>` for filtered text matches.
9. Run `projectatlas slice <file> --start-line <n> --end-line <m>` for exact source.
10. Run `projectatlas health-check` before cleanup/refactor decisions.
11. Use deeper source reads only for selected targets.
12. Run `projectatlas token` when token-savings reporting is requested; use `projectatlas token --view tui` only for a human terminal dashboard.

Token savings estimate avoided wrong-folder exploration, wrong-file opens, and unnecessary full-code reads caused by the atlas-first workflow. Agent and MCP surfaces should stay structured by default; the TUI dashboard is explicit terminal UI.

## AGENTS.md Snippet

```text
## Startup
1. Run `projectatlas scan` or `projectatlas map --force` when the index may be stale.
2. Run `projectatlas overview`.
3. Use `projectatlas folders <query>` and `projectatlas files <query> --folder <path>` before opening source; use `projectatlas files --file-pattern <glob>` for direct glob discovery.
4. Run `projectatlas summary <file> --limit 25` for structured file facts.
5. Run `projectatlas outline <file>` for compressed context if needed.
6. Run `projectatlas search <pattern> --file-pattern <glob>` or `projectatlas slice <file> --start-line <n> --end-line <m>` before broad reads.
7. Run `projectatlas lint --strict-folders --report-untracked`.
8. Run `projectatlas token` when asked for token savings; use `projectatlas token --view tui` only for a human dashboard.
```

## References

- ProjectAtlas repo: https://github.com/styler-ai/ProjectAtlas
- `docs/projectatlas-3-architecture.md` for the target architecture.
- `docs/agent-integration.md` for the AGENTS.md snippet.
- `docs/format.md` for TOON schema.
- `docs/workflow.md` for troubleshooting.
