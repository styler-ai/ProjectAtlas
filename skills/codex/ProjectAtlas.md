# ProjectAtlas (Codex skill)

## Goal

Give Codex a fast, accurate structure map before deep indexing so it knows where to look and where to place new
files. ProjectAtlas is the layer above code-index tools.

## When to use

- At the start of every session (before deep indexing).
- After creating or moving folders.
- After adding new source files.
- When `projectatlas lint` reports missing Purpose headers or missing `.purpose` files.
- Before large refactors or cleanup decisions.

## Definitions

- Deep indexing = full-file or symbol-level analysis via tools like code-index MCP or language servers. This is
  powerful but expensive in context budget if you run it blindly.

## Required files

- `.projectatlas/projectatlas.toon` (the atlas snapshot).
- `.projectatlas/config.toml` (scan rules).
- `.projectatlas/projectatlas-nonsource-files.toon` (agent-maintained summaries for non-source files).

## First-time setup (repo adoption)

1. Install locally: `pip install -e .`
2. Initialize: `projectatlas init --seed-purpose` (auto-detects repo languages for config; use `--no-detect-languages` to keep the static template)
3. Fill each `.purpose` file with a one-line summary (ASCII, no commas).
4. Add Purpose headers to every tracked source file (comment style per extension; see `purpose.styles_by_extension`).
5. Add non-source files to `.projectatlas/projectatlas-nonsource-files.toon`.
6. Run `projectatlas map` to generate `.projectatlas/projectatlas.toon`.
7. Run `projectatlas lint --strict-folders --report-untracked` and fix issues.
8. (Optional) Install git hooks: `python scripts/install_hooks.py` to enforce issue references in commits.

## Startup workflow (every session)

1. Run `projectatlas map` (unless `PROJECTATLAS_SKIP_UPDATE=1` is set).
2. Read `.projectatlas/projectatlas.toon`.
3. Scan `folder_tree[]` to pick the correct area of the repo.
4. Check `folder_summary_duplicates[]` / `file_summary_duplicates[]` for drift.
5. Use `folders[]` / `files[]` to pick targets.
6. Only then use deep-index tools (code-index, LSPs) on those targets.
7. If lint errors appear, fix them immediately (add Purpose headers or `.purpose` files) or remove the stale file.

## How to interpret the map

- `overview:` shows tracked counts so you can spot drift quickly. It now reports
  `tracked_source_files`, `tracked_nonsource_files`, and `tracked_files_total`.
- `folder_tree[]` provides a tree with summaries for fast navigation.
- `folders[]` and `files[]` are the authoritative summaries for lookup.
- `*_summary_duplicates[]` highlight likely overlap to clean up.

## Why non-source files are tracked separately

- Some files cannot safely carry inline `Purpose:` headers (JSON, lockfiles, images, generated outputs).
- Those entries live in `.projectatlas/projectatlas-nonsource-files.toon` and are merged into the atlas.
- Agents read only the generated atlas; the nonsource file is the durable input list.

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
- If you do not use deep indexing, rely on the atlas and open files directly as needed.

## References

- ProjectAtlas repo: https://github.com/styler-ai/ProjectAtlas
- `docs/agent-integration.md` for the AGENTS.md snippet.
- `docs/format.md` for TOON schema.
- `docs/workflow.md` for troubleshooting.
