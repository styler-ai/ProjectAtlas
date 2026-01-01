# ProjectAtlas (Codex skill)

## Purpose

Maintain and evolve ProjectAtlas maps (Purpose headers, folder summaries, generated map files).

## Quick start

1. Run `projectatlas init --seed-purpose` once.
2. Regenerate the map with `projectatlas map` (output: `.projectatlas/projectatlas.toon`).
3. Validate locally with `projectatlas lint --strict-folders --report-untracked`.
4. Commit the updated map and any Purpose header or folder summary changes.

## Purpose headers

- Ensure each source file has the required Purpose header format (Javadoc-style block with a `Purpose:` line).
- Update headers before re-running the map so the generator captures intent.

## Documentation accuracy

- Document generator changes using the repo's preferred style (Javadoc-style headers or Python docstrings).
- Keep map-related rules, skills, and Memory Bank notes aligned with generator behavior.

## Folder summaries

- Add or update `.purpose` summaries for new folders as required by the schema or lint rules.

## Build and CI integration

- If the map must stay current, wire `projectatlas map` into a local build step.
- Keep updates deterministic so builds remain reproducible.

## Health check intent

- Treat the map and untracked report as a structure health check.
- Every local build forces a decision to document, exclude, or reorganize files and folders.

## Startup navigation

- Use map summaries to prioritize which source files to inspect with code indexing at startup.
- Follow repo rules for markdown and non-source files; the map is for source intent, not content.

## Overview line

- The map includes an overview line with tracked counts; it is emitted by the generator and validated by lint.
- Do not edit the overview line manually; regenerate the map instead.

## Untracked report

- Use `--report-untracked` to list non-source files and excluded paths.
- Use `--strict-untracked` only when you want untracked files to fail the build.
- The report distinguishes allowed vs disallowed untracked files and flags asset files outside approved roots.

## CI behavior

- Map generation skips when `CI` or `GITHUB_ACTIONS` is truthy (use `projectatlas map --force` to override).
- Set `PROJECTATLAS_SKIP_UPDATE=1` to skip updates locally.
- Set `PROJECTATLAS_ALLOW_UNTRACKED=1` to allow local builds to pass while still reporting untracked files.

## References

- See `docs/workflow.md` for schema and troubleshooting details.
