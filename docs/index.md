# ProjectAtlas

ProjectAtlas gives agents a compact task-oriented route to the right source before broad search or full-file reads. ProjectAtlas 3 is
Rust-native and stores repository intelligence in `.projectatlas/projectatlas.db`, with compact TOON output for
agent-facing context.

For normal MCP work, call one compact `atlas_session_brief`, follow its returned next call, and read exact source only after the target is known. Overview, folders, and files remain the manual or unavailable-brief fallback.

`projectatlas token --view tui` opens the human Ratatui token impact dashboard with the reconciled saved-token equation; separate proportional bars for observed and modeled file reads avoided; broad folder walks skipped and candidate files not opened retained in the exact source ledger and navigation composition; calibration notes; status hints; and, at wide terminal sizes, a bounded Atlas map. Benchmark comparisons remain in structured CLI/MCP output and never add TUI rows. Add `--theme light` for light terminal color schemes. Ani remains documented as the ProjectAtlas mascot asset, but mascot rendering is deferred from the token TUI for now.

## Public docs surfaces

- README is the primary product and release overview.
- GitHub Pages publishes generated Cargo API documentation and the generated Language & Ecosystem Support catalog at `https://styler-ai.github.io/ProjectAtlas/`.
- Markdown files in `docs/` carry workflow, configuration, architecture, and benchmark details.
- Design references live in `docs/design/`, including Ani's PNG/SVG mascot assets and the Ratatui token impact dashboard target.

After every merged or closed PR that changes installation, CLI behavior, MCP behavior, release process, public API,
token reporting, or documented agent workflow, refresh README and the relevant docs or Pages-facing content before
closing linked issues. If a PR does not require docs changes, confirm that README and the published docs surface are
still current in the PR checklist.

## Quick start

1. Establish the project root and run `projectatlas init` for first-run setup, indexing, generated MCP configs, and purpose handoff.
2. Bind the intended project in MCP; use per-call `project_path` for shared or concurrent hosts.
3. Refresh with `atlas_watch_once`, `atlas_scan`, or the CLI equivalents only when the index may be stale.
4. Call `atlas_session_brief` once with the task and `compact: true`.
5. Follow its returned summary, search, relation, health, or slice call without repeating discovery.
6. Copy returned selectors into `atlas_slice` for the smallest exact source range.
7. When session brief is unavailable or broader structure is the task, use the manual CLI fallback: `projectatlas overview`, folders, files, summary, then slice.
8. Run `projectatlas lint --report-untracked --purpose-level low`.

## Why it matters

Without a structural map, agents waste context by reading the wrong files and recreate folders because intent is
hidden. ProjectAtlas adds an atlas-first layer so you ask "where should I look?" before running deeper scans.
