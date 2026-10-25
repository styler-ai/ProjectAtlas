# Purpose: Guide teams through adopting ProjectAtlas in an existing repository.

# Adoption Checklist

Use this checklist when adding ProjectAtlas to an existing repo.

## 1. Install

```bash
cargo install --path crates/projectatlas-cli --locked
```

## 2. Initialize

```bash
projectatlas init
```

Run this from the project root. ProjectAtlas 3 stores one durable index per project at `.projectatlas/projectatlas.db`.
`projectatlas init` also runs the initial scan/index and writes generated MCP configs under `.projectatlas/`.
Legacy `.purpose` files are migration input only; new purpose records should be stored with `projectatlas purpose set`.

## 3. Inspect or refresh the atlas

```bash
projectatlas overview
projectatlas folders <query>
projectatlas files <query> --folder <path>
projectatlas files --file-pattern <glob>
projectatlas summary <file> --limit 25
```

ProjectAtlas 3 stores durable index state in `.projectatlas/projectatlas.db`.
Run `projectatlas scan` later when you need an explicit refresh after file changes and no watcher is running.
Legacy `.purpose` files are migration input, not the final storage model.

## 4. Add or import purpose summaries

Use `projectatlas purpose set <path> <purpose>` for explicit purpose records.
Legacy Purpose headers and `.purpose` files are still imported during migration, but lint no longer requires or enforces them.

## 5. Track non-source files

Add summaries for non-source files in `.projectatlas/projectatlas-nonsource-files.toon` (agent-maintained input).

## 6. Optional current map response

```bash
projectatlas map
```

Use this when you need the current AtlasMap on stdout. It does not write `.projectatlas/projectatlas.toon`; use `projectatlas map --json` only when an adjacent JSON sidecar is explicitly needed.

## 7. Lint

```bash
projectatlas lint --report-untracked --purpose-level low
```

## 8. Wire into local scripts

Example recurring refresh target after first-run init:

```bash
projectatlas scan
projectatlas lint --report-untracked --purpose-level low
```

Example `Makefile`:

```makefile
projectatlas-check:
	@projectatlas scan
	@projectatlas lint --report-untracked --purpose-level low

projectatlas-show-map:
	@projectatlas map
```

Keep `projectatlas-show-map` opt-in; normal agent workflows should use the SQLite index.

## 9. Agent setup

- Add the startup snippet from `templates/AGENTS.md` to your `AGENTS.md`.
- Install the ProjectAtlas plugin skill or copy public guidance from this repository's docs. Keep personal
  workspace memory local and ignored through `.gitignore`.
