# ProjectAtlas (Claude skill)

## Purpose

ProjectAtlas generates a concise project map so Claude can reason about repo structure before editing.

## Quick use

1. Run `projectatlas init --seed-purpose` once.
2. Run `projectatlas map` at session start.
3. Open `.projectatlas/projectatlas.toon` and scan the folder tree + summaries.
4. Use `projectatlas lint --strict-folders --report-untracked` to surface issues.

## Notes

- Purpose headers must start with `Purpose:` and remain one line.
- Folder summaries live in `.purpose` files at each directory level.
- Use `projectatlas map --force` to run in CI if needed.
- Set `PROJECTATLAS_ALLOW_UNTRACKED=1` to allow local builds while still reporting.
- PR titles or bodies must reference a GitHub issue (`#NNN`) for CI to pass.

## References

- See `docs/workflow.md` for schema and troubleshooting details.
