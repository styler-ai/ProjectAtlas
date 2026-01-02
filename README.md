# ProjectAtlas

![CI](https://github.com/styler-ai/ProjectAtlas/actions/workflows/ci.yml/badge.svg)

Agent-first project map + health index - give coding agents a one-line purpose for every file and folder.

ProjectAtlas scans a repo, reads per-file Purpose headers and per-folder `.purpose` files, and emits a TOON
snapshot you can read at startup to understand structure, spot duplicates, and keep the tree healthy.

Live docs: https://styler-ai.github.io/ProjectAtlas/

## Problem it solves

Agents and humans struggle with the same problem on growing repos: you start a new session without a structural
overview. Without a fast map:

- Agents over-read code or index the wrong files, wasting context budget.
- Duplicate folders and files appear because intent is not visible.
- A clean folder structure slowly drifts into a mess.

ProjectAtlas fixes this by creating a lightweight, human- and agent-readable map that sits above deep code search.
It answers "where should I look?" and "where should I put this?" before running heavy indexing tools.

## Features

- Purpose enforcement for source files and folders.
- TOON output for fast agent consumption.
- Duplicate summary detection for early structure drift.
- Lint mode that fails when headers or `.purpose` files are missing.
- Configurable exclusions, asset roots, and allowlists.

## How it works

ProjectAtlas is designed for the first 60 seconds of an agent session.

1. Each folder carries a `.purpose` file with a one-line summary so folder intent is explicit (no guesswork).
2. Each tracked source file starts with a `Purpose:` header or module docstring so file intent is visible without
   a deep read.
3. `projectatlas map` builds a TOON snapshot (`.projectatlas/projectatlas.toon`) that contains:
   - a folder tree with inline purpose summaries
   - file summaries for targeted code reads
   - duplicate-summary warnings to spot drift
   - overview stats that show scope and coverage
4. `projectatlas lint` fails when Purpose headers or `.purpose` files are missing. The goal is to force a decision
   before you proceed: add the missing summary, or remove/relocate the folder/file if it no longer belongs.

Why this matters:

- Agents read the atlas at startup (via AGENTS.md) to decide where to look next.
- The atlas tells you *which* files to open with deep-indexing tools (for example code-index MCP or language
  servers) so you only deep-index what you actually need. Deep indexing here means full-file or symbol-level
  analysis that can consume a lot of context if you run it blindly.
- The lint gate keeps structure healthy over time by preventing silent drift.

ProjectAtlas also supports non-source files (README, workflows, configs) via
`.projectatlas/projectatlas-nonsource-files.toon` so the snapshot stays complete even for files without headers.

### Why there are two TOON files

- `.projectatlas/projectatlas.toon` is **generated output**. It is safe to rebuild on every run.
- `.projectatlas/projectatlas-nonsource-files.toon` is **agent-maintained input** for non-source files that cannot
  carry a `Purpose:` header (for example YAML, TOML, images, or configs you do not want to edit).

ProjectAtlas merges the non-source entries into the generated atlas, so **agents only read the generated atlas**.
The input file exists only to preserve those non-source summaries across regenerations. Agents update it when
`projectatlas lint` reports missing non-source entries or when new config/doc files are added.

## Workflow (agent-focused)

1. Run `projectatlas init --seed-purpose` once to scaffold missing `.purpose` files.
2. For every new folder, write a one-line purpose in its `.purpose` file (this is your folder contract).
3. For every new source file, add a `Purpose:` header or module docstring (this is your file contract).
4. Add non-source summaries to `.projectatlas/projectatlas-nonsource-files.toon`.
5. Regenerate the map with `projectatlas map`.
6. Read `.projectatlas/projectatlas.toon` at startup and look for:
   - the folder tree to locate the right area of the repo
   - duplicate summaries to spot drift or overlap
   - file summaries to pick targets for deeper inspection
6. Use code-index or other deep tools *only* on the files you selected from the atlas.
7. Run `projectatlas lint --strict-folders --report-untracked` and fix any missing Purpose entries before you move on.

Agent integrations (Codex, Claude, etc.) should read the map at startup and treat it as the authoritative
structural overview before doing deeper indexing.

## Install

Local (editable) install:

```bash
pip install -e .
```

Agent-assisted install:

Give your agent the repo URL (https://github.com/styler-ai/ProjectAtlas) and ask it to:

1. Install ProjectAtlas (`pip install -e .`).
2. Run `projectatlas init --seed-purpose`.
3. Add or update `.purpose` files and Purpose headers.
4. Wire `projectatlas map` + `projectatlas lint` into local build steps.
5. Paste the startup snippet into your `AGENTS.md`.

## Quickstart

```bash
pip install -e .
projectatlas init --seed-purpose
projectatlas map
projectatlas lint --strict-folders --report-untracked
```

Install git hooks (enforces issue references in commit messages):

```bash
python scripts/install_hooks.py
```

Issue hygiene: label every issue with `type:*`, `priority:*`, and `status:*`.
Assign issues you are actively working on to the target release milestone; CI enforces that referenced issues have a milestone.

Run tests, docs, and build artifacts locally:

```bash
python -m unittest discover -s tests
python scripts/check_docstrings.py
python scripts/generate_api_docs.py
python -m pip install build
python -m build --sdist --wheel
```

## Documentation

- Live docs: https://styler-ai.github.io/ProjectAtlas/
- `docs/configuration.md`: configuration reference
- `docs/adoption.md`: adoption checklist for new repos
- `docs/format.md`: TOON schema
- `docs/workflow.md`: workflow + troubleshooting
- `docs/api.md`: generated API documentation

## Related projects

- TOON format: https://github.com/toon-format/toon
- code-index (deep code summaries / symbol extraction): https://github.com/johnhuang316/code-index-mcp

If you do not use a deep indexing tool, ProjectAtlas still provides the atlas and lint gate, but you will need to
open source files manually for deeper context.

## Branches and releases

- `dev`: active development branch.
- `main`: stable releases only.

Release flow:

1. Merge feature branches into `dev`.
2. Ensure `dev` includes the latest `main` changes (sync if needed).
3. Run `python scripts/prepare_release.py --issue <NNN> --bump patch` to create a release branch and PR.
4. If `dev` is behind `main`, the script will stop unless you pass `--allow-base-divergence`.
5. Optional: add `--post-release` to open a dev PR that bumps to the next `.dev` version.
6. Merge to `main` after CI is green.
7. Auto-release runs on `main` pushes and creates a GitHub release if the version is not a `.dev` build.
8. Use the manual Release workflow if you need to re-run tagging.

## Versioning

ProjectAtlas follows PEP 440 for Python packaging. Pre-release versions use `.devN` (for example `0.1.0.dev0`).
Release tags should match the package version (for example `v0.1.0.dev0`).

Helper:

```bash
python scripts/prepare_release.py --issue <NNN> --bump patch
```

## Output files

Default outputs:

- `.projectatlas/config.toml`
- `.projectatlas/projectatlas-nonsource-files.toon`
- `.projectatlas/projectatlas.toon`

## Folder structure (ProjectAtlas repo)

```
.
|-- .projectatlas/
|   |-- config.toml
|   |-- projectatlas-nonsource-files.toon
|   `-- projectatlas.toon
|-- .codex/
|   `-- skills/
|       `-- ProjectAtlas.md
|-- skills/
|   |-- codex/ProjectAtlas.md
|   `-- claude/ProjectAtlas.md
|-- scripts/
|   |-- install_hooks.py
|   `-- check_commit_issue.py
`-- templates/
    `-- AGENTS.md
```

Use this tree when contributing to ProjectAtlas itself.

## Folder structure (your repo after install)

```
your-repo/
|-- .projectatlas/
|   |-- config.toml
|   |-- projectatlas-nonsource-files.toon
|   `-- projectatlas.toon
|-- .purpose
`-- (your source and docs)
```

For Codex, copy `.codex/skills/ProjectAtlas.md` into your local Codex skills folder
(for example `~/.codex/skills/ProjectAtlas.md`). For Claude, copy
`skills/claude/ProjectAtlas.md` into your Claude skills location.

## Purpose headers

ProjectAtlas expects a one-line `Purpose:` entry at the top of each tracked file:

```txt
/**
 * Purpose: Describe the file in one sentence.
 */
```

Python files should use a module docstring with `Purpose:` on the first lines. Vue files should place the
Javadoc-style block at the top of the first `<script>` or `<style>` block.

## Configuration

See `docs/configuration.md` for all available settings. Most teams only need to adjust:

- `scan.source_extensions`
- `scan.exclude_dir_names`
- `untracked.asset_allowed_prefixes`
- `project.map_path`

## Agent integration

Use `docs/agent-integration.md` for a ready-to-copy snippet for AGENTS.md, plus suggested startup steps.
ProjectAtlas includes a Codex skill at `.codex/skills/ProjectAtlas.md` and a Claude skill in
`skills/claude/ProjectAtlas.md`.

## License

MIT. See `LICENSE`.

## Contribution policy

External code contributions are not accepted at this time. See `CONTRIBUTING.md`.
