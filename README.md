# ProjectAtlas

Agent-first project map + health index — give coding agents a one-line purpose for every file and folder.

ProjectAtlas scans a repo, reads per-file Purpose headers and per-folder `.purpose` files, and emits a TOON
snapshot you can read at startup to understand structure, spot duplicates, and keep the tree healthy.

## Features

- Purpose enforcement for source files and folders.
- TOON output for fast agent consumption.
- Duplicate summary detection for early structure drift.
- Lint mode that fails when headers or `.purpose` files are missing.
- Configurable exclusions, asset roots, and allowlists.

## Quickstart

```bash
pip install -e .
projectatlas init --seed-purpose
projectatlas map
projectatlas lint --strict-folders --report-untracked
```

Run tests and build artifacts locally:

```bash
python -m unittest discover -s tests
python -m pip install build
python -m build --sdist --wheel
```

Default outputs:

- `.projectatlas/config.toml`
- `.projectatlas/projectatlas-manual-files.toon`
- `.projectatlas/projectatlas.toon`

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

## License

MIT. See `LICENSE`.

## Contribution policy

External code contributions are not accepted at this time. See `CONTRIBUTING.md`.
