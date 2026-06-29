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
Legacy `.purpose` files are migration input only; new purpose records should be stored with `projectatlas purpose set`.

## 3. Build the atlas

```bash
projectatlas scan
projectatlas overview
projectatlas folders <query>
projectatlas files <query> --folder <path>
projectatlas files --file-pattern <glob>
projectatlas summary <file> --limit 25
```

ProjectAtlas 3 stores durable index state in `.projectatlas/projectatlas.db`.
Legacy `.purpose` files are migration input, not the final storage model.

## 4. Add or import purpose summaries

Use `projectatlas purpose set <path> <purpose>` for explicit purpose records.
Legacy Purpose headers and `.purpose` files are still imported during migration, but lint no longer requires or enforces them.

## 5. Track non-source files

Add summaries for non-source files in `.projectatlas/projectatlas-nonsource-files.toon` (agent-maintained input).

## 6. Optional compatibility map export

```bash
projectatlas map --force
```

Skip this step unless an older integration still reads `.projectatlas/projectatlas.toon`.

## 7. Lint

```bash
projectatlas lint --report-untracked --purpose-level low
```

## 8. Wire into local scripts

Example shell target:

```bash
projectatlas scan
projectatlas lint --report-untracked --purpose-level low
```

Example `Makefile`:

```makefile
projectatlas-check:
	@projectatlas scan
	@projectatlas lint --report-untracked --purpose-level low

projectatlas-export-map:
	@projectatlas map --force
```

Keep `projectatlas-export-map` opt-in for older integrations only; normal agent workflows should use the SQLite index.

## 9. Agent setup

- Add the startup snippet from `templates/AGENTS.md` to your `AGENTS.md`.
- Install the ProjectAtlas plugin skill or copy public guidance from this repository's docs. Keep personal
  workspace memory local and ignored through `.gitignore`.
