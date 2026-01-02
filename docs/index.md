# ProjectAtlas

ProjectAtlas gives agents a fast structural overview before deep indexing. It reads per-file Purpose
headers and per-folder `.purpose` summaries, then produces a TOON snapshot that is easy to scan at
startup. Use it to choose the right files quickly and avoid duplicate folders or scattered intent.

## Quick start

1. `projectatlas init --seed-purpose`
2. Fill `.purpose` files and Purpose headers.
3. `projectatlas map`
4. `projectatlas lint --strict-folders --report-untracked`

## Why it matters

Without a structural map, agents waste context by reading the wrong files and recreate
folders because intent is hidden. ProjectAtlas adds a lightweight layer above code-index tools
so you can ask "where should I look?" before running deeper scans.
