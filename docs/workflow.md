# Workflow and Troubleshooting

ProjectAtlas is designed to run locally and produce a deterministic map.

## Recommended workflow

1. `projectatlas init --seed-purpose` (first-time setup).
2. Add Purpose headers to new source files.
3. Add or update `.purpose` summaries for new folders.
4. Run `projectatlas map`.
5. Run `projectatlas lint --strict-folders --report-untracked`.
6. Commit map updates and any Purpose changes.

## Branching

- `dev` for active development.
- `main` for stable releases only.
- Merge `dev` -> `main` via pull request after CI is green.

## CI behavior

- `projectatlas map` skips in CI unless you pass `--force`.
- `projectatlas lint` validates that the map is current.

Environment toggles:

- `PROJECTATLAS_SKIP_UPDATE=1` skips map generation locally.
- `PROJECTATLAS_ALLOW_UNTRACKED=1` allows local builds while still reporting untracked files.

## Troubleshooting

### Map is stale

If lint reports stale hashes or an overview mismatch, re-run:

```bash
projectatlas map
```

### Missing Purpose headers

Add a Javadoc-style header with `Purpose:` to the file. For Python, use a module docstring.

### Missing .purpose files

Create a `.purpose` file in the folder and add a one-line summary. You can seed them with:

```bash
projectatlas seed-purpose
```

### Untracked files

Use `--report-untracked` to list non-source files. Either:

- add to the manual file list
- add to allowlists/exclusions
- move into an approved asset root

## Schema reference

The TOON schema is documented in `docs/format.md`.
