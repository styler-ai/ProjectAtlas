# ProjectAtlas (Codex skill)

## Purpose

Use ProjectAtlas to generate and validate an agent-first map of a repo, with one-line purposes for every folder and file.

## When to use

- At startup to understand the repo layout before deep indexing.
- Before refactors to detect duplicate folder responsibilities.
- In local builds to fail on missing Purpose headers or `.purpose` files.

## Core workflow

1. Run `projectatlas map` (or `projectatlas init --seed-purpose` once).
2. Read `.projectatlas/atlas.toon`.
3. If lint fails, add/repair Purpose headers or `.purpose` files.
4. Re-run `projectatlas map` to refresh hashes and the folder tree.

## Tips

- Keep Purpose summaries ASCII, comma-free, and under 140 characters.
- Use `projectatlas lint --report-untracked` to spot assets outside approved roots.
- Update `projectatlas.toml` (or `.projectatlas/config.toml`) for custom exclusions.
