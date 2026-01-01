# ProjectAtlas (Claude skill)

## Purpose

ProjectAtlas generates a concise project map so Claude can reason about repo structure before editing.

## How to use

1. Run `projectatlas map` at session start.
2. Open `.projectatlas/atlas.toon` and scan the folder tree + summaries.
3. Use `projectatlas lint --strict-folders` to enforce missing `.purpose` files.
4. Refresh the map after structural changes.

## Notes

- Purpose headers should be one-line summaries starting with `Purpose:`.
- Folder summaries come from `.purpose` files at each directory level.
- Update the config file to tune exclusions or asset roots.
