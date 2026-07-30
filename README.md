# ProjectAtlas

<p align="center">
  <img src="docs/assets/projectatlas-mascot.png" alt="ProjectAtlas mascot holding a repository map labeled src, docs, tests, and issues" width="720">
</p>

<p align="center">
  <strong>Rust-native, high-performance local repository intelligence for coding agents and large codebases.</strong><br>
  A persistent SQLite map guides Codex, Claude Code, OpenCode, and other MCP-capable agents to the right code before they spend context reading the wrong files.
</p>

<p align="center">
  <a href="https://github.com/styler-ai/ProjectAtlas/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/styler-ai/ProjectAtlas/actions/workflows/ci.yml/badge.svg"></a>
  <a href="https://github.com/styler-ai/ProjectAtlas/releases/tag/v0.4.0"><img alt="release" src="https://img.shields.io/badge/release-v0.4.0-blue"></a>
  <img alt="rust" src="https://img.shields.io/badge/Rust-2024-orange">
  <img alt="license" src="https://img.shields.io/badge/license-MIT-green">
</p>

## About

ProjectAtlas is a native Rust CLI and MCP server that keeps a project-local map of folders, files, reviewed purposes, deterministic summaries, symbols, graph relationships, searchable text, health findings, and token telemetry. `.gitignore`-aware scanning, BLAKE3 hashing, SQLite storage, filesystem watching, and compact TOON output keep repeated repository orientation local and fast.

The map is deliberately agent-first: purposes identify the responsible area, graph relationships reveal connected code, compact summaries and outlines explain the selected files, and exact source slices provide the final evidence. Agents narrow before they read broadly.

No required `.purpose` files. No source-header tax. No hosted index or credentials. Project state lives beside the repository in `.projectatlas/`.

## Quickstart

```bash
codex plugin marketplace add styler-ai/ProjectAtlas --ref v0.4.0
codex plugin add projectatlas --marketplace projectatlas
```

If Codex already has an older ProjectAtlas marketplace snapshot cached, refresh
the configured Git snapshot before reinstalling the plugin:

```bash
codex plugin marketplace upgrade projectatlas --json
codex plugin remove projectatlas --marketplace projectatlas
codex plugin add projectatlas --marketplace projectatlas
codex plugin list --marketplace projectatlas --available --json
```

If the marketplace source is pinned to an older release tag, `marketplace upgrade`
correctly keeps that pinned ref. In that case, replace only the dedicated `styler-ai/ProjectAtlas` source after confirming no unrelated plugin depends on it, then install again with the commands above:

```bash
codex plugin marketplace remove projectatlas
codex plugin marketplace add styler-ai/ProjectAtlas --ref v0.4.0
```

Then tell Codex: "Use ProjectAtlas for this repo."

That is the intended path. The plugin supplies the version-matched workflow skill, native runtime installer, and MCP configuration templates.

## Agent Workflow

ProjectAtlas is intentionally agent-first. In normal use you should not have to memorize command syntax.

The agent follows this loop:

1. Bind the intended project and refresh only when the index may be stale.
2. Call `atlas_session_brief` once with the task and compact output.
3. Follow its returned summary, search, relation, health, or exact-slice call directly.
4. Continue from returned selectors instead of restarting folder/file discovery.
5. Open the smallest exact source slice needed to answer the task.

`atlas_overview` → `atlas_folders` → `atlas_files` remains the fallback when the brief is unavailable, returns no actionable candidate, or broader repository structure is itself the task.

For active sessions, the agent can run the watcher so file edits continuously refresh the database. For cleanup sessions, it can ask ProjectAtlas for missing purposes, stale metadata, duplicate folder roles, and structure drift.

## Representative Token Estimate

The token-overview record below is a representative large-application audit,
not a marketing constant, provider billing total, or universal savings claim.
Its result depends on repository size, how often the agent asks for orientation,
and how much source ProjectAtlas prevents the agent from opening.

The estimate is:

```text
without ProjectAtlas = avoided candidate files, directory walks, and full-file reads
with ProjectAtlas    = compact TOON payloads returned by overview, folders, files, summaries, search, symbols, and slices
legacy gross saved   = without ProjectAtlas - with ProjectAtlas
savings rate         = legacy gross saved / without ProjectAtlas
tokens avoided       = measured saved + deduped modeled avoided
file reads avoided   = observed summary/slice replacements + search-modeled narrowing
```

The default estimator is deliberately simple and local: `ceil(chars_or_bytes / 4)`. It is a workflow estimate, not provider billing telemetry. Reports preserve the legacy gross saved number for continuity and also expose `measured_tokens_saved`, `gross_modeled_tokens_avoided`, `deduped_modeled_tokens_avoided`, conservative headline `tokens_avoided`, and likely file reads avoided split into observed summary/slice replacements versus search-modeled narrowing.

| Signal | Result |
| --- | ---: |
| Files | 679 |
| Folders | 206 |
| Indexed text files | 554 |
| Indexed text bytes | 7,088,446 |
| Symbols | 5,145 |
| Relations | 12,122 |
| Token telemetry calls | 142 |
| Average baseline avoided per call | 1,557,144 tokens |
| Average ProjectAtlas payload per call | 2,997 tokens |
| Total estimated without ProjectAtlas | 221,114,448 tokens |
| Total estimated with ProjectAtlas | 425,622 tokens |
| Legacy gross estimated saved | 220,688,826 tokens |
| Observed savings rate for this workload | 99.8% |

<p align="center">
  <img src="docs/assets/token-savings-bar.svg" alt="Token savings benchmark bar chart: without ProjectAtlas 221.1 million estimated tokens, with ProjectAtlas 0.4 million estimated tokens across 142 calls" width="820">
</p>

That bar chart is intentionally lopsided. The point of ProjectAtlas is not that every repository magically saves 99.8%; the point is that repeated agent lookups in a large repo should read compact folder/file intelligence first, not broad source trees first.

Sizing intuition:

| Workload shape | What usually happens |
| --- | --- |
| Small repo, few lookups | Less source exists to avoid, so estimated differences are usually smaller. |
| Medium repo, repeated feature work | Repeated orientation creates more opportunities to avoid wrong-file reads. |
| Large repo, many exploratory lookups | Broad candidate sets make narrowing effects larger in this audit model. |

## Expected Large-Repo Latency

The latency sample below is for warm indexed reads after ProjectAtlas has already scanned the repo. Initial scan/watch refresh is a different operation because it hashes files, updates SQLite, refreshes text, and parses symbol candidates.

Benchmark scale:

| Repo shape | Size |
| --- | ---: |
| Files | 679 |
| Folders | 206 |
| Indexed text files | 554 |
| Indexed text bytes | 7.1 MB |
| Symbols | 5,145 |
| Relations | 12,122 |

Warm CLI reads from that audit stayed around 160-166 ms:

| Command shape | Sample latency |
| --- | ---: |
| `summary <large-source-file> --limit 25` | ~165 ms |
| `files workflow --folder .github/workflows --limit 20` | ~164 ms |
| `token` | ~161 ms |
| `overview` | ~166 ms |

In this audited application, warm orientation commands stayed comfortably sub-second. These values are not a universal latency guarantee: hardware, repository shape, database state, and command bounds all matter. The scan/build step can take longer, but the agent should not pay that full cost for every lookup; it should use `watch` or `watch --once` to keep the database fresh and then read from the indexed atlas.

Token reports expose bucket, baseline, and confidence metadata so observed full-file compression is not silently mixed with modeled navigation savings. That is deliberate: normal agent orientation stays local, fast, and credential-free.

## CLI Reference

Most users can stop at the plugin install. The CLI is here for local debugging, automation, and release verification.

Only need the CLI yourself? Install it from the released tag:

```bash
cargo install --git https://github.com/styler-ai/ProjectAtlas --tag v0.4.0 projectatlas-cli --locked
```

From this checkout:

```bash
cargo install --path crates/projectatlas-cli --locked
```

Then initialize and inspect a repo:

```bash
projectatlas init
projectatlas overview
```

`projectatlas init` is the one-call first-run bootstrap: it creates `.projectatlas/`, writes default config and
non-source scaffolding when missing, initializes `.projectatlas/projectatlas.db`, runs the initial scan/index, writes
project-local MCP configs for Codex/generic MCP, Claude Code, and OpenCode, and returns the purpose-curation handoff.
Use `projectatlas scan` later when you want an explicit refresh.

The explicit CLI fallback for humans, automation, or a host without `atlas_session_brief` is:

```bash
projectatlas overview
projectatlas folders "auth"
projectatlas files "login" --folder src
projectatlas summary src/main.rs --limit 25
projectatlas slice src/main.rs --start-line 1 --end-line 80
```

For active work:

```bash
projectatlas watch
```

For a human token dashboard:

```bash
projectatlas token --view tui
```

That renders a Ratatui token-impact snapshot: a readable reconciled `Without ProjectAtlas - With ProjectAtlas = Saved by ProjectAtlas` equation, source-reconciled file reads avoided with their observed and modeled split, persisted modeled folder-walk steps, source rows, and calibration notes. A wide terminal adds a bounded, non-interactive atlas map drawn only from resolved relations in the active project database; narrower terminals keep the proven overview without crowding it. The default theme is dark; use `--theme light` for light terminals or `--theme terminal` to preserve the terminal background. Rerun the command to refresh the snapshot. Ani remains the ProjectAtlas mascot in the design assets, but the token TUI defers mascot rendering until a future focused pass.

For a local tokenizer calibration of indexed UTF-8 files, add `--tokenizer o200k_base` or `--tokenizer cl100k_base`.

## Agent And MCP Setup

ProjectAtlas ships plugin metadata and installer scripts for Codex and Claude Code, plus an OpenCode MCP config template.

`projectatlas init` writes project-local MCP configs automatically. Regenerate them manually when needed:

```bash
projectatlas --format json --db .projectatlas/projectatlas.db mcp-config > .projectatlas/projectatlas.mcp.json
projectatlas --format json --db .projectatlas/projectatlas.db mcp-config --harness claude-code > .projectatlas/projectatlas.claude.mcp.json
projectatlas --format json --db .projectatlas/projectatlas.db mcp-config --harness opencode > .projectatlas/projectatlas.opencode.json
```

For normal setup, point your agent to
[https://github.com/styler-ai/ProjectAtlas](https://github.com/styler-ai/ProjectAtlas)
and ask it to install and set up ProjectAtlas through its plugin store or the
supported method best suited to that agent harness.

For manual setup, resolve the version-matched installer from an installed
ProjectAtlas plugin or source checkout, then pass the target project root
separately:

```powershell
& "<ProjectAtlas checkout>\plugins\projectatlas\scripts\install-runtime.ps1" -ProjectRoot "<target project root>"
```

```bash
bash "<ProjectAtlas checkout>/plugins/projectatlas/scripts/install-runtime.sh" "<target project root>"
```

The generated configs pin the runtime version, project database, config path, and working directory where the host supports it.
When `codex` is available, the installer also repairs a stale official Codex
`projectatlas` marketplace/plugin cache to the runtime release tag, then repairs
a stale global `codex mcp` registry entry named `projectatlas` so it uses the
verified runtime and this project's `.projectatlas` DB/config. It verifies the
Codex ProjectAtlas skill artifact when Codex exposes the plugin source path,
reports Claude Code/OpenCode generated-config status, and warns on stale
official ProjectAtlas release pins in downstream `.github/workflows` files. Set
`PROJECTATLAS_SKIP_CODEX_PLUGIN_UPDATE=1` only if you intentionally manage the
Codex ProjectAtlas plugin marketplace yourself, and
`PROJECTATLAS_SKIP_CODEX_MCP_REGISTRY_UPDATE=1` only if you intentionally manage
that global Codex MCP entry yourself. After updates, agents should verify with
`codex plugin list --marketplace projectatlas --json` and
`codex mcp get projectatlas` or `codex mcp list`.

Claude Code and OpenCode convergence is generated-config based: the installer parses the generated
host JSON and verifies the absolute runtime path, `--require-version`, selected DB path, optional
config path, and final `mcp` command. OpenCode verification also checks `type = "local"`,
`enabled = true`, and the project `cwd`. The installer does not mutate Claude Code or OpenCode
user-managed settings or pretend those hosts have a Codex-style ProjectAtlas marketplace cache; restart
the running host if it cached older instructions.

For task-directed MCP startup, agents call `atlas_session_brief` once with `compact: true` to get selected project identity, index
state, bounded ranked candidates, health blockers, and ready next-call recommendations. They follow that call directly instead of repeating folder/file discovery. `atlas_settings`
also includes a typed `mcp_session` capability block with nearest-project policy, path scope, telemetry
mode, scan policy, runtime identity, and no-secret guarantees. `atlas_task_status` and
`atlas_task_cancel` expose the bounded task-progress contract; existing scan/watch/search/summary/slice
calls remain synchronous in this release.

## What The Agent Gets

ProjectAtlas exposes the same indexed capabilities through CLI and MCP; the compact task-start route is MCP-first:

| Need | CLI | MCP |
| --- | --- | --- |
| Select project | `projectatlas --db <repo>/.projectatlas/projectatlas.db ...` | `atlas_set_project_path` / per-call `project_path` |
| Refresh state | `projectatlas scan` / `projectatlas watch --once` | `atlas_scan` / `atlas_watch_once` |
| Start a task | `overview` → `folders` → `files` → `summary` → `slice` | `atlas_session_brief` with `compact: true`, then its returned call |
| Understand shape | `projectatlas overview` | `atlas_overview` |
| Pick an area | `projectatlas folders <query>` | `atlas_folders` |
| Pick files | `projectatlas files <query> --folder <path>` | `atlas_files` |
| Inspect a file | `projectatlas summary <file>` | `atlas_file_summary` |
| See symbols | `projectatlas symbols list --file <file>` | `atlas_symbols` |
| Search narrowly | `projectatlas search <pattern> --file-pattern <glob>` | `atlas_search` |
| Read exact code | `projectatlas slice <file> --start-line <n> --end-line <m>` | `atlas_slice` |
| Find cleanup work | `projectatlas health-check --source-only --limit <n>` | `atlas_health` |
| Curate purposes | `projectatlas purpose queue --limit <n>` / `projectatlas purpose set <path> "<purpose>"` / `projectatlas purpose review --from-file <json> --apply` | `atlas_purpose_queue` / `atlas_purpose_set` / `atlas_purpose_review` |
| Report savings | `projectatlas token` | `atlas_token_report` |

## Release Quality

`v0.4.0` ships through the full release matrix:

- Rust format, check, clippy, dependency policy, tests, doctests, and rustdoc.
- ProjectAtlas scan, parity, database-backed purpose lint, and health checks.
- Linux x64, Windows x64, macOS x64, and macOS arm64 packages.
- Prepublish packaged-runtime installer smokes.
- Postpublish release-runtime installer smokes.
- Codex, Claude Code, and OpenCode MCP config generation checks.

## Repository Layout

```text
crates/
  projectatlas-cli/       CLI, MCP server, release-facing runtime logic
  projectatlas-core/      shared models, TOON rendering, telemetry
  projectatlas-db/        SQLite storage
  projectatlas-fs/        .gitignore-aware scanning
  projectatlas-service/   summaries, search, slices, health
  projectatlas-symbols/   symbol extraction
docs/                     architecture, workflow, configuration
plugins/projectatlas/     Codex and Claude Code plugin metadata, OpenCode MCP config template
skills/                   standalone agent skill snippets
```

## Docs

- Published rustdoc and Pages landing page: https://styler-ai.github.io/ProjectAtlas/
- Language & Ecosystem Support: https://styler-ai.github.io/ProjectAtlas/language-support/
- CLI/MCP runtime crate docs: https://styler-ai.github.io/ProjectAtlas/projectatlas/
- Core model crate docs: https://styler-ai.github.io/ProjectAtlas/projectatlas_core/
- `docs/agent-integration.md`
- `docs/configuration.md`
- [`docs/language-support.md`](docs/language-support.md) — generated capability and ecosystem authority
- [`docs/relation-support.md`](docs/relation-support.md) — generated accepted relation-family inventory
- `docs/workflow.md`
- `docs/structural-summaries.md`
- `docs/benchmarks/large-application-token-savings.md`
- `docs/projectatlas-3-architecture.md`

Documentation closeout rule: after a PR changes installation, CLI behavior, MCP behavior, release process, public API, token reporting, or documented agent workflow, update README and the relevant docs/Page-facing content before closing the PR and linked issues. If no docs-facing behavior changed, explicitly confirm README and the published docs surface are still current in the PR checklist.

## License

MIT. See `LICENSE`.
