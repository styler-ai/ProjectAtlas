# Adoption Checklist

Use this checklist when adding ProjectAtlas to an existing repo.

## 1. Install

```bash
cargo install --path crates/projectatlas-cli --locked
```

## 2. Initialize

```bash
projectatlas init --seed-purpose
```

Run this from the project root. ProjectAtlas 3 stores one durable index per project at `.projectatlas/projectatlas.db`.
The current Rust `init` command supports `--seed-purpose` for migration seeding; it does not require a language-detection flag.

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
Legacy Purpose headers and `.purpose` files can still be used while migrating existing repositories.

## 5. Track non-source files

Add summaries for non-source files in `.projectatlas/projectatlas-nonsource-files.toon` (agent-maintained input).

## 6. Generate the map

```bash
projectatlas map
```

## 7. Lint

```bash
projectatlas lint --strict-folders --report-untracked
```

## 8. Wire into local scripts

Example shell target:

```bash
projectatlas scan
projectatlas map --force
projectatlas lint --strict-folders --report-untracked
```

Example `Makefile`:

```makefile
projectatlas-map:
	@projectatlas map

projectatlas-lint:
	@projectatlas lint --strict-folders --report-untracked
```

## 9. Agent setup

- Add the startup snippet from `templates/AGENTS.md` to your `AGENTS.md`.
- Install the ProjectAtlas plugin skill or copy public guidance from this repository's docs. Keep personal
  workspace memory local and ignored through `.gitignore`.
