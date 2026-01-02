# ProjectAtlas Startup Snippet

## Startup
1. Run `projectatlas map` (or ensure it runs as part of local build scripts).
2. Read `.projectatlas/projectatlas.toon` and scan the folder tree + summaries.
3. Check duplicate summaries for drift, then select files to inspect.
4. Run `projectatlas lint --strict-folders --report-untracked`.
5. Fix missing Purpose headers or `.purpose` files before deeper work.
6. Only then use deep indexing (code-index, LSPs) on selected files.
7. If using Codex, load `.codex/skills/ProjectAtlas.md` for the map workflow.
