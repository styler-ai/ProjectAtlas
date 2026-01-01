# ProjectAtlas (Codex skill)

## Purpose

Give a Codex agent a fast, structured project overview before deep indexing. ProjectAtlas is the
map layer above code-index tools so the agent knows where to look and where to place new files.

## When to use

- At the start of every session (before running deep indexing).
- After creating/moving folders or adding new source files.
- When lint reports missing Purpose headers or missing `.purpose` files.

## First-time setup (repo adoption)

1. Install locally: `pip install -e .`
2. Initialize: `projectatlas init --seed-purpose`
3. Fill each `.purpose` file with a one-line summary (use `Purpose:` if you want).
4. Add Purpose headers to every tracked source file.
5. Add non-source files to `.projectatlas/projectatlas-manual-files.toon` with summaries.
6. Run `projectatlas map` to generate `.projectatlas/projectatlas.toon`.
7. Run `projectatlas lint --strict-folders --report-untracked` and fix any errors.

## Startup workflow (every session)

1. Run `projectatlas map` (unless `PROJECTATLAS_SKIP_UPDATE=1` is set).
2. Read `.projectatlas/projectatlas.toon`.
3. Use `folder_tree[]` and `folders[]` to decide which files to open or index.
4. If `folder_summary_duplicates[]` is non-empty, flag it as a structure health issue.
5. Only then use code-index tools for deeper file detail.

## Purpose headers

- Javadoc-style block with a single `Purpose:` line for JS/TS/CSS/etc.
- Python modules use a module docstring with `Purpose:` on the first lines.
- Vue SFCs place the Javadoc block at the top of the first `<script>` or `<style>` block.

## Folder summaries (.purpose)

- Every folder must have a `.purpose` file with a one-line summary.
- Use `projectatlas seed-purpose` to scaffold missing `.purpose` files.
- Keep summaries short, ASCII, and single-line.

## Map interpretation

- `overview:` shows tracked counts so you can spot drift quickly.
- `folder_tree[]` is the tree with summaries for fast navigation.
- `folders[]` and `files[]` are the definitive summaries the agent should trust.
- `folder_summary_duplicates[]` highlights likely structural duplicates.

## Untracked handling

- `projectatlas lint --report-untracked` lists non-source files.
- Add required non-source files to `.projectatlas/projectatlas-manual-files.toon`.
- Exclude unwanted paths via config, or move assets into approved roots.

## Build and CI behavior

- `projectatlas map` skips in CI by default; use `--force` if CI should regenerate.
- CI enforces lint and docstring checks; PRs must reference `#NNN`.
- Local builds should run map + lint so the agent gets current structure.

## Env toggles

- `PROJECTATLAS_SKIP_UPDATE=1` skips map generation locally.
- `PROJECTATLAS_ALLOW_UNTRACKED=1` allows local builds to pass while still reporting.

## References

- `docs/workflow.md` for workflow and troubleshooting.
- `docs/format.md` for TOON schema.
- `docs/configuration.md` for config options.
