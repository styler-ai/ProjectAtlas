# Agent Integration

ProjectAtlas is designed to be read at agent startup so you can:

- Pick the correct file quickly.
- Spot duplicated folder roles early.
- Keep structure clean as the repo grows.

## Codex / AGENTS.md snippet

```
## Startup
1. Run `projectatlas map` (or ensure your build does).
2. Read `.projectatlas/projectatlas.toon`.
3. Scan `folder_tree[]` for where to work and `folders[]`/`files[]` for precise targets.
4. Check `folder_summary_duplicates[]` / `file_summary_duplicates[]` and flag drift.
5. Run `projectatlas lint --strict-folders --report-untracked`.
6. If lint fails, add missing Purpose headers or `.purpose` files (or remove stale items) before continuing.
7. Only then run deep indexing (code-index, LSPs) on the files you selected from the atlas.

Note: the non-source file list (`.projectatlas/projectatlas-nonsource-files.toon`) is agent-maintained input for
non-source summaries and is merged into the atlas. Agents still read only the generated atlas.
```

## Codex skills

ProjectAtlas ships a Codex skill at `.codex/skills/ProjectAtlas.md`. Copy that file into your
Codex workspace or keep it in place so Codex can load the ProjectAtlas workflow.

## Claude / skills

Drop the `skills/claude/ProjectAtlas.md` into your Claude skills folder and reference it in your agent setup.

## Lint and CI

ProjectAtlas `lint` is meant for local workflows. Many teams skip map generation in CI, but still run
lint on PRs to surface missing headers. Use `projectatlas map --force` if CI must regenerate the map.
