# ProjectAtlas

ProjectAtlas gives agents a fast structural overview before broad search or full-file reads. ProjectAtlas 3 is
Rust-native and stores repository intelligence in `.projectatlas/projectatlas.db`, with compact TOON output for
agent-facing context.

Use it to choose folders, files, structured summaries, outlines, and exact source slices in that order.

## Quick start

1. Establish the project root and run ProjectAtlas from that root.
2. `projectatlas init --seed-purpose`
3. `projectatlas scan`
4. `projectatlas overview`
5. `projectatlas folders <query>`
6. `projectatlas files <query> --folder <path>` or `projectatlas files --file-pattern <glob>`
7. `projectatlas summary <file> --limit 25`
8. `projectatlas outline <file>` when the structured summary is not enough
9. `projectatlas slice <file> --start-line <n> --end-line <m>` only after selecting the indexed file
10. `projectatlas lint --strict-folders --report-untracked`

## Why it matters

Without a structural map, agents waste context by reading the wrong files and recreate folders because intent is
hidden. ProjectAtlas adds an atlas-first layer so you ask "where should I look?" before running deeper scans.
