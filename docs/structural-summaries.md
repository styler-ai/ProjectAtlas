# Structural Summaries

ProjectAtlas stores distinct navigation fields for indexed project nodes:

- `folder_purpose`: why a folder exists. This is agent-approved project intent used before choosing a work area.
- `file_purpose`: why a file exists. This is agent-approved or generated-suggested project intent used before detailed inspection.
- `content_summary`: what the index can deterministically observe inside the file.

For source files with declarations, the deep symbol graph produces the content summary. For declaration-light files, ProjectAtlas uses deterministic structural adapters before symbol extraction so agents do not see weak byte-count fallbacks as normal intelligence.

## Current Structural Adapters

- Markdown and MDX: title and heading structure.
- JSON and JSONC: top-level keys, package manifests, and dataset manifests.
- YAML: top-level keys and GitHub Actions workflow name, triggers, and jobs.
- TOML: top-level tables, Cargo manifests, and ProjectAtlas config shape.
- CSS-family files: selectors, custom properties, media queries, and supports queries.
- HTML: title, meta description, H1/H2 headings, and structured-data markers.
- TOON: named sections.
- Simple config/text files: key-like entries, plus first non-empty line excerpts for plain text.

The adapters are intentionally bounded and deterministic. They do not approve purposes. If a file has no approved purpose, the scan can create a generated `file_purpose` suggestion from the `content_summary`, and the agent harness must inspect enough context to approve or correct it with `projectatlas purpose set` or `atlas_purpose_set`.

## Quality Signals

`projectatlas summary <file>` exposes:

- `parser_kind`: `tree-sitter-symbol-graph`, `manifest-symbol-graph`,
  `structural-symbol-graph`, `fallback-symbol-graph`, `mixed-symbol-graph`,
  `structural`, `scanner-metadata`, or `missing`.
- `summary_status`: `ok`, `fallback`, or `missing`.

ProjectAtlas persists file-level parser metadata separately from emitted symbols, so an empty native parse still reports its native parser and an empty fallback parse still reports `fallback-symbol-graph`.

Agent integrations should treat `summary_status: fallback` as a reason to escalate into deeper inspection or parser improvement. Normal supported files should not rely on summaries like `<language> file, N bytes`.

## Regression Fixtures

Representative fixtures live in `fixtures/languages`. Their accepted outputs are
recorded in `fixtures/languages/baselines.toon`, decoded in tests through the
official `toon-format` crate, and verified by the CLI end-to-end test
`language_fixture_summaries_match_baselines`.

The separate end-to-end test `scan_indexes_every_supported_language_extension`
creates one temporary fixture for every extension in ProjectAtlas's public
language registry and proves the real scanner indexes each path with the
expected language family. It also runs the real `projectatlas summary` command
for each fixture and verifies a non-empty content summary, non-missing parser
kind, and non-missing summary status. This broad registry test is intentionally
separate from the exact summary baseline so fallback-supported languages remain
covered without requiring brittle one-line summaries for every extension alias.
