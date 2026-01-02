# ProjectAtlas

![CI](https://github.com/styler-ai/ProjectAtlas/actions/workflows/ci.yml/badge.svg)

Agent-first project map + health index - give coding agents a one-line purpose for every file and folder.

ProjectAtlas scans a repo, reads per-file Purpose headers and per-folder `.purpose` files, and emits a TOON
snapshot you can read at startup to understand structure, spot duplicates, and keep the tree healthy.

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

1. Each folder carries a `.purpose` file with a one-line summary.
2. Each tracked source file starts with a `Purpose:` header or module docstring.
3. `projectatlas map` builds a TOON snapshot with folder tree + file summaries.
4. `projectatlas lint` validates that the snapshot is current and complete.

## Workflow (agent-focused)

1. Run `projectatlas init --seed-purpose` once to scaffold missing `.purpose` files.
2. For every new folder, write a one-line purpose in its `.purpose` file.
3. For every new source file, add a `Purpose:` header or module docstring.
4. Regenerate the map with `projectatlas map`.
5. Use the map to decide which files to inspect with heavier tools (code-index, language servers, etc.).
6. Run `projectatlas lint --strict-folders --report-untracked` to surface drift and missing summaries.

Agent integrations (Codex, Claude, etc.) should read the map at startup and treat it as the authoritative
structural overview before doing deeper indexing.

## Install

Local (editable) install:

```bash
pip install -e .
```

Agent-assisted install:

Give your agent the ProjectAtlas repo URL and ask it to:

1. Install ProjectAtlas (`pip install -e .`).
2. Run `projectatlas init --seed-purpose`.
3. Add `.purpose` files and Purpose headers.
4. Wire `projectatlas map` + `projectatlas lint` into local build steps.
5. Add the startup snippet to your `AGENTS.md`.

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

Run tests, docs, and build artifacts locally:

```bash
python -m unittest discover -s tests
python scripts/check_docstrings.py
python scripts/generate_api_docs.py
python -m pip install build
python -m build --sdist --wheel
```

## Documentation

- `docs/configuration.md`: configuration reference
- `docs/format.md`: TOON schema
- `docs/workflow.md`: workflow + troubleshooting
- `docs/api.md`: generated API documentation

## Branches and releases

- `dev`: active development branch.
- `main`: stable releases only.

Release flow:

1. Merge feature branches into `dev`.
2. Open a PR from `dev` to `main`.
3. Merge to `main` after CI is green.
4. Auto-release runs on `main` pushes and creates a GitHub release if the version is not a `.dev` build.
5. Use the manual Release workflow if you need to re-run tagging.

## Versioning

ProjectAtlas follows PEP 440 for Python packaging. Pre-release versions use `.devN` (for example `0.1.0.dev0`).
Release tags should match the package version (for example `v0.1.0.dev0`).

Helper:

```bash
python scripts/next_version.py --bump patch --apply
```

## Output files

Default outputs:

- `.projectatlas/config.toml`
- `.projectatlas/projectatlas-manual-files.toon`
- `.projectatlas/projectatlas.toon`

## Folder structure (ProjectAtlas repo)

```
.
├─ .projectatlas/
│  ├─ config.toml
│  ├─ projectatlas-manual-files.toon
│  └─ projectatlas.toon
├─ .codex/
│  └─ skills/
│     └─ ProjectAtlas.md
├─ skills/
│  ├─ codex/ProjectAtlas.md
│  └─ claude/ProjectAtlas.md
├─ scripts/
│  ├─ install_hooks.py
│  └─ check_commit_issue.py
└─ templates/
   └─ AGENTS.md
```

Use this tree when contributing to ProjectAtlas itself.

## Folder structure (your repo after install)

```
your-repo/
├─ .projectatlas/
│  ├─ config.toml
│  ├─ projectatlas-manual-files.toon
│  └─ projectatlas.toon
├─ .purpose
└─ (your source and docs)
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
