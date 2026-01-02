# Adoption Checklist

Use this checklist when adding ProjectAtlas to an existing repo.

## 1. Install

```bash
pip install -e .
```

## 2. Initialize

```bash
projectatlas init --seed-purpose
```

## 3. Fill folder summaries

- Add a one-line summary to every `.purpose` file.
- Keep summaries short and single-line.

## 4. Add Purpose headers

Add a Javadoc-style `Purpose:` header to every tracked source file. Python modules should use a
module docstring with `Purpose:`.

## 5. Track non-source files

Add summaries for non-source files in `.projectatlas/projectatlas-manual-files.toon`.

## 6. Generate the map

```bash
projectatlas map
```

## 7. Lint

```bash
projectatlas lint --strict-folders --report-untracked
```

## 8. Wire into local scripts

Example `package.json`:

```json
{
  "scripts": {
    "projectatlas:map": "projectatlas map",
    "projectatlas:lint": "projectatlas lint --strict-folders --report-untracked"
  }
}
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
- Copy the Codex skill to `~/.codex/skills/ProjectAtlas.md` if you use Codex.
