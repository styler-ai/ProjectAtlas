# ProjectAtlas (Claude skill)

## Goal

Give Claude a fast structure map before deep indexing so it can pick the right files and avoid wasting context.

## When to use

- At session start (before deep indexing).
- After creating or moving folders.
- After adding new source files.
- When `projectatlas lint` reports missing Purpose headers or missing `.purpose` files.
- Before refactors or cleanup passes.

## Definitions

- Deep indexing = full-file or symbol-level analysis via tools like code-index MCP or language servers.

## First-time setup (repo adoption)

1. Install locally: `pip install -e .`
2. Initialize: `projectatlas init --seed-purpose` (auto-detects repo languages for config; use `--no-detect-languages` to keep the static template)
3. Fill each `.purpose` file with a one-line summary (ASCII, no commas).
4. Add Purpose headers to every tracked source file (comment style per extension; see `purpose.styles_by_extension`).
5. Add non-source files to `.projectatlas/projectatlas-nonsource-files.toon`.
6. Run `projectatlas map`.
7. Run `projectatlas lint --strict-folders --report-untracked` and fix issues.

## Startup workflow (every session)

1. Run `projectatlas map` (unless `PROJECTATLAS_SKIP_UPDATE=1` is set).
2. Read `.projectatlas/projectatlas.toon`.
3. Scan `folder_tree[]` and `folders[]` to pick where to work.
4. Check duplicate summary lists for structural drift.
5. Use deep indexing tools only for the specific files you chose from the atlas.
6. Fix missing Purpose headers or `.purpose` files before continuing.

## AGENTS.md integration

Add a startup snippet so the atlas is always read:

```
## Startup
1. Run `projectatlas map`.
2. Read `.projectatlas/projectatlas.toon`.
3. Use the atlas to select files before deep indexing.
4. Fix missing Purpose headers or `.purpose` files if lint fails.
```

## Companion tools

- code-index (deep code summaries): https://github.com/johnhuang316/code-index-mcp
- Without deep indexing, open files manually using the atlas to guide selection.

## References

- ProjectAtlas repo: https://github.com/styler-ai/ProjectAtlas
- `docs/agent-integration.md` for the AGENTS.md snippet.
- `docs/format.md` for TOON schema.
- `docs/workflow.md` for troubleshooting.
