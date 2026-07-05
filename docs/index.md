# ProjectAtlas

ProjectAtlas gives agents a fast structural overview before broad search or full-file reads. ProjectAtlas 3 is
Rust-native and stores repository intelligence in `.projectatlas/projectatlas.db`, with compact TOON output for
agent-facing context.

Use it to choose folders, files, structured summaries, outlines, and exact source slices in that order.

`projectatlas token --view tui` opens the human Ratatui token impact dashboard with Ani, the reconciled saved-token equation, file reads avoided, observed/modeled savings, source rows, calibration notes, and status hints. Add `--theme light` for light terminal color schemes.

## Public docs surfaces

- README is the primary product and release overview.
- GitHub Pages publishes generated cargo documentation from `cargo doc` at `https://styler-ai.github.io/ProjectAtlas/`.
- Markdown files in `docs/` carry workflow, configuration, architecture, and benchmark details.
- Design references live in `docs/design/`, including Ani's PNG/SVG mascot assets and the Ratatui token impact dashboard target.

After every merged or closed PR that changes installation, CLI behavior, MCP behavior, release process, public API,
token reporting, or documented agent workflow, refresh README and the relevant docs or Pages-facing content before
closing linked issues. If a PR does not require docs changes, confirm that README and the published docs surface are
still current in the PR checklist.

## Quick start

1. Establish the project root and run ProjectAtlas from that root.
2. `projectatlas init` for first-run setup, initial scan/index, generated MCP configs, and purpose handoff
3. `projectatlas overview`
4. `projectatlas folders <query>`
5. `projectatlas files <query> --folder <path>` or `projectatlas files --file-pattern <glob>`
6. `projectatlas summary <file> --limit 25`
7. `projectatlas outline <file>` when the structured summary is not enough
8. `projectatlas slice <file> --start-line <n> --end-line <m>` only after selecting the indexed file
9. `projectatlas scan` or `projectatlas watch --once` when the index may be stale
10. `projectatlas lint --report-untracked --purpose-level low`

## Why it matters

Without a structural map, agents waste context by reading the wrong files and recreate folders because intent is
hidden. ProjectAtlas adds an atlas-first layer so you ask "where should I look?" before running deeper scans.
