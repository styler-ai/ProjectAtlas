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
3. Use the folder tree + purpose summaries to pick files before deep indexing.
```

## Codex skills

ProjectAtlas ships a Codex skill at `.codex/skills/ProjectAtlas.md`. Copy that file into your
Codex workspace or keep it in place so Codex can load the ProjectAtlas workflow.

## Claude / skills

Drop the `skills/claude/ProjectAtlas.md` into your Claude skills folder and reference it in your agent setup.

## Lint and CI

ProjectAtlas `lint` is meant for local workflows. Many teams skip map generation in CI, but still run
lint on PRs to surface missing headers. Use `projectatlas map --force` if CI must regenerate the map.
