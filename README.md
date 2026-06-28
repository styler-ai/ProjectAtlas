# ProjectAtlas

![CI](https://github.com/styler-ai/ProjectAtlas/actions/workflows/ci.yml/badge.svg)

ProjectAtlas is a Rust-native repository atlas for coding agents. It gives agents an orientation layer before
they spend tokens on broad search, full-file reads, or symbol-level inspection.

The original ProjectAtlas goal has not changed: maintain an agent-first map where important folders and files have
one-line purposes, so agents can navigate fast, spot structure drift, and choose the right file before deep indexing.
ProjectAtlas 3 keeps that goal and upgrades the implementation with Rust speed, a SQLite atlas database, MCP tools,
broader source support, and an improved deep code index.

ProjectAtlas 3 merges folder purpose, file purpose, source summaries, search, health checks, lint policy, and
token-savings telemetry into one workflow:

1. inspect the project overview
2. choose the relevant folder
3. choose the relevant file
4. inspect compressed outlines and source summaries
5. request exact symbols, ranges, or source only when correctness requires it

ProjectAtlas 3 is inspired by the repository indexing ideas in code-index MCP, but it is a Rust-native
ProjectAtlas implementation with ProjectAtlas-owned naming, architecture, commands, health checks, purpose
tracking, and token telemetry.

## Why It Exists

Large repositories make agents waste context. Without an atlas, an agent tends to search broadly, read entire
files too early, or place new code in folders whose intent is unclear.

ProjectAtlas fixes that by keeping durable local repository intelligence in `.projectatlas/projectatlas.db` and
returning compact TOON responses for agents. The goal is to answer "where should I look?" before "which source
should I read?".

## Current Capabilities

- Rust workspace with strict lint, test, and rustdoc gates.
- `.gitignore`-aware scanning with fast content hashes.
- SQLite-backed index state for files, folders, purposes, and usage telemetry.
- SQL-bounded folder/file ranking, health checks, token aggregation, and streamed text search for large repositories.
- TOON-first output for compact agent context, with JSON available for integrations.
- Progressive CLI funnel: `scan`, `overview`, `folders`, `files`, `summary`, `outline`, `search`, `slice`, `symbols`, `watch`, `health-check`, `settings`, `config --print`, `watch-status`, `reset-index`, `lint`, `parity`, and `token`.
- Native MCP server through `projectatlas mcp`, built on the official Rust MCP SDK, with `atlas_*` tools returning TOON text payloads.
- Rust/Cargo-aware and tree-sitter-backed symbol graph extraction for functions, classes, methods, imports, calls, dependencies, and manifest symbols.
- Event-backed `projectatlas watch` using the canonical Rust `notify` crate, with debouncing, repository excludes, and portable polling fallback.
- Rust-native legacy map/lint compatibility for `.projectatlas/projectatlas.toon`.
- Broad language/file extension recognition across the repository-intelligence parity set.
- Structural summaries for declaration-light Markdown, JSON, YAML, TOML, CSS, HTML, TOON, and config files so supported files do not silently rely on byte-count fallbacks.
- Health checks for missing purposes, duplicate purposes, and repeated temp/generated folder roles.
- Token-savings telemetry through `projectatlas token`.
- Read-only review mode through `PROJECTATLAS_NO_TELEMETRY=1` when orientation commands must not write usage rows.

## ProjectAtlas 3 Roadmap

ProjectAtlas 3 stable must complete the full repository-intelligence surface in Rust:

- workspace selection and safe project switching
- shallow refresh and deep symbol indexing
- all-language discovery and fallback indexing
- literal, regex, fuzzy, paginated, and filtered search; search is intentionally case-insensitive by default for agent discovery and can be narrowed with `--case-sensitive`
- source summaries, outlines, imports, exports, and symbol relationships
- exact symbol/range slices to avoid full-file reads
- watcher status and incremental refresh
- settings/cache inspection
- MCP tools with `atlas_*` names for Codex, OpenCode, Claude Code, and other harnesses
- plugin packaging that installs or invokes the native runtime and registers the MCP server

## Install

From this repository:

```bash
cargo install --path crates/projectatlas-cli --locked
```

For local development without installing:

```bash
cargo run -p projectatlas-cli -- --help
```

## Quickstart

```bash
projectatlas init --seed-purpose
projectatlas map --force
projectatlas scan
projectatlas overview
projectatlas folders auth
projectatlas files auth --folder src
projectatlas files --file-pattern "*.rs"
projectatlas summary src/main.rs --limit 25
projectatlas outline src/main.rs
projectatlas symbols list --file src/main.rs
projectatlas symbols relations --file src/main.rs
projectatlas search "fn main" --file-pattern "*.rs" --context-lines 2
projectatlas search "fnm" --fuzzy --file-pattern "*.rs"
projectatlas slice src/main.rs --start-line 1 --end-line 40
projectatlas symbols build . --max-workers 4 --timeout-seconds 120
projectatlas symbols slice src/main.rs main --symbol-kind function
projectatlas health-check
projectatlas settings
projectatlas watch-status
projectatlas reset-index --dry-run
projectatlas --format json runtime-info
projectatlas --format json mcp-config
projectatlas watch --once
projectatlas watch
projectatlas token
projectatlas token --view tui
PROJECTATLAS_NO_TELEMETRY=1 projectatlas overview
projectatlas lint --strict-folders --report-untracked
```

The legacy `.purpose` files and Purpose headers are still supported as migration/import sources. The ProjectAtlas
3 source of truth is the SQLite index plus explicit purpose records, so future workflows do not need to pollute
source files or product folders with required metadata files.

## Agent Workflow

Agents should use ProjectAtlas before broad source reads:

1. Establish the project root and run ProjectAtlas from that root so `.projectatlas/projectatlas.db` belongs to this project.
2. Run `projectatlas scan` or `projectatlas map --force` when the index may be stale.
3. Run `projectatlas overview` to understand repository shape.
4. Run `projectatlas folders <query>` to choose the right area.
5. Run `projectatlas files <query> --folder <path>` to pick targets; use `projectatlas files --file-pattern <glob>` when the filename or path pattern is already known.
6. Run `projectatlas summary <file> --limit 25` for structured file facts and purpose state.
7. Run `projectatlas outline <file>` if the summary is not enough.
8. Run `projectatlas symbols list --file <file>` and `projectatlas symbols relations --file <file>` when symbol context matters.
9. Run `projectatlas search <pattern> --file-pattern <glob>` when you need bounded text matches inside the chosen area; use `--fuzzy` when you only remember an approximate name, and inspect returned, searched file, searched byte, and truncated counters before widening the search.
10. Run `projectatlas slice <file> --start-line <n> --end-line <m>` or `projectatlas symbols slice <file> <symbol> --symbol-parent <parent> --symbol-kind <kind> --symbol-line <line>` for exact source slices; add symbol disambiguators when names repeat.
11. Run `projectatlas health-check` before cleanup/refactor decisions.
12. Run `projectatlas token` to inspect structured estimated saved tokens; use `projectatlas token --view tui` only when a human terminal dashboard is wanted.

The canonical token report command is `projectatlas token`; the MCP tool is `atlas_token_report`.
Harness-specific slash commands can alias that surface.

Token savings are an estimate of avoided wrong-place exploration, avoided wrong-file opens, and avoided
unnecessary full-code reads. The default CLI and MCP reports stay structured for agents; the opt-in TUI view is a
human-facing terminal dashboard and does not replace the TOON/MCP contract.

`summary` is designed for large repositories: repeated sections are bounded by `--limit`, totals come from SQLite
count queries, `called_by` is conservative when symbol names are ambiguous, and `source_status` tells the agent
whether live source or indexed metadata backed the source-derived fields.
File summaries also expose `parser_kind` and `summary_status`, so agents can distinguish deep symbol summaries,
structural summaries, and weak scanner metadata. Treat `summary_status: fallback` as a reason to inspect deeper or
improve a parser.

`search` and symbol slicing share the same service-layer indexed-file boundary as MCP. Search uses
`globset` repository globs, supports literal/regex/fuzzy line matching, stops after the requested page is
satisfied, and reports how much indexed source was scanned. Symbol slices reject ambiguous duplicate names until
the agent supplies `--symbol-parent`, `--symbol-kind`, or `--symbol-line`, which keeps exact-source reads tied to
the symbol selected during orientation.

`scan`, MCP `atlas_scan`, watcher refresh, map/lint, and legacy purpose cleanup honor configured
`[scan].exclude_dir_names` and `[scan].exclude_path_prefixes`.
Initial scans that import legacy TOON map purposes skip stale or newly excluded map rows and report the skipped
count instead of failing with a raw SQLite no-row error.
The durable inputs `.projectatlas/config.toml` and `.projectatlas/projectatlas-nonsource-files.toon` remain
indexable even though generated `.projectatlas` artifacts such as the SQLite DB, generated map, and MCP config stay
excluded.
Deep symbol builds support `--max-workers` and `--timeout-seconds` so large repositories can trade throughput,
CPU pressure, and bounded agent wait time.

For MCP-capable agents, register the native server:

```json
{
  "mcpServers": {
    "projectatlas": {
      "command": "projectatlas",
      "args": ["mcp"]
    }
  }
}
```

For an absolute, project-local registration document, run:

```bash
projectatlas --format json --db .projectatlas/projectatlas.db mcp-config > .projectatlas/projectatlas.mcp.json
```

The generated config contains the absolute binary path, absolute `--db` path, a `--config` path when
available, and a `cwd` project-root hint. `mcp-config` discovers both `.projectatlas/config.toml`
and `projectatlas.toml` from the selected DB/project root. The MCP server also resolves path-less
root-sensitive tools from config, indexed DB metadata, or the default `.projectatlas/projectatlas.db`
parent, so hosts that ignore `cwd` still scan the intended project.

Use MCP tools in the same funnel order: `atlas_scan`, `atlas_overview`, `atlas_folders`, `atlas_files`,
`atlas_file_summary`, `atlas_outline`, `atlas_symbols`, `atlas_symbol_relations`, `atlas_search`, `atlas_slice`,
`atlas_health`, `atlas_watch_once`, and `atlas_token_report`.

`atlas_health` returns a bounded page by default and accepts `limit`, `start_index`, `category`, `severity`,
`path_prefix`, and `summary_only` arguments so agents can inspect large health surfaces without one oversized
MCP payload.

`projectatlas watch` is the continuous local watcher. It starts with a baseline refresh, then uses filesystem
events to refresh SQLite summaries/symbols after relevant file changes. Ordinary file changes use partial
SQLite and symbol refresh; directory/root/ignore-rule events fall back to a full scan for correctness. `atlas_watch_once` and
`projectatlas watch --once` are the bounded refresh surfaces agents should call after edits when no continuous
watcher is running.

## Codex Plugin

ProjectAtlas ships a plugin package from this repository:

```bash
codex plugin marketplace add styler-ai/ProjectAtlas --ref main
codex plugin add projectatlas --marketplace projectatlas
```

The plugin provides the ProjectAtlas workflow skill, a version-guarded fallback MCP server config at `plugins/projectatlas/.mcp.json`,
runtime install scripts, and a generated project-local MCP config at `.projectatlas/projectatlas.mcp.json`:

```powershell
plugins/projectatlas/scripts/install-runtime.ps1
```

```bash
plugins/projectatlas/scripts/install-runtime.sh
```

Run the installer from the target project root or pass the project root explicitly. The installer verifies
`projectatlas --format json runtime-info`, including the runtime version when the plugin manifest or
`PROJECTATLAS_VERSION` supplies a release tag. It prefers a local source checkout, otherwise downloads the release
tag derived from the plugin manifest, and falls back to the same tagged Cargo Git install path. It then writes the
absolute MCP registration file for that project. `runtime-info` is intentionally a read-only compatibility probe and
does not create `.projectatlas` by itself.

Marketplace installation should run or point to the native runtime installer before registering `projectatlas mcp`,
so Codex, OpenCode, Claude Code, and other MCP-capable harnesses can call the same `atlas_*` TOON tools.

## Local Verification

Run the full Rust gate stack:

```bash
cargo fmt --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo test --doc --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo run -p projectatlas-cli -- map --force
cargo run -p projectatlas-cli -- lint --strict-folders --report-untracked
```

Install local git hooks by linking or copying the scripts in `.githooks/` into `.git/hooks/`.

## Documentation

- `docs/projectatlas-3-architecture.md`: target architecture and parity map
- `docs/agent-integration.md`: agent startup instructions
- `docs/configuration.md`: configuration reference
- `docs/adoption.md`: adoption checklist
- `docs/format.md`: TOON schema
- `docs/workflow.md`: workflow and troubleshooting

Rust API documentation is generated with:

```bash
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
```

## Release Flow

- `dev`: active development branch
- `main`: stable releases only
- release tags match the Cargo workspace version, for example `v0.3.2`

Before a release, run the full local verification stack, merge `dev` into `main` through a PR, and use the manual
Release workflow if a tag needs to be created explicitly.

## Repository Layout

```text
.
|-- .github/
|   `-- workflows/
|-- .projectatlas/
|   |-- config.toml
|   |-- projectatlas-nonsource-files.toon
|   `-- projectatlas.toon
|-- crates/
|   |-- projectatlas-cli/
|   |-- projectatlas-core/
|   |-- projectatlas-db/
|   |-- projectatlas-fs/
|   |-- projectatlas-service/
|   `-- projectatlas-symbols/
|-- docs/
|-- plugins/
|   `-- projectatlas/
|-- skills/
|   |-- claude/
|   `-- codex/
`-- templates/
```

## Configuration

See `docs/configuration.md` for all settings. Most projects only need to adjust:

- `scan.source_extensions`
- `scan.exclude_dir_names`, honored by scan, MCP scan, watcher refresh, map/lint, and legacy purpose cleanup
- `scan.exclude_path_prefixes`, exact repository subtrees to omit from scan, map, lint, watch, search, and imports
- `scan.text_index_max_bytes`, the per-file UTF-8 text-index cap for large repositories
- `untracked.asset_allowed_prefixes`
- `project.map_path`
- `purpose.default_style`
- `purpose.styles_by_extension`

## License

MIT. See `LICENSE`.

## Contribution Policy

External code contributions are not accepted at this time. See `CONTRIBUTING.md`.

Release and CI gates include `cargo fmt`, `cargo check`, `cargo clippy -D warnings`, workspace tests,
doctests, rustdoc warnings as errors, `cargo deny check`, ProjectAtlas lint/map drift checks, and
installer/package smoke checks.
