# Short `atlas` CLI: function and trigger

`atlas` is the installed short forwarder for the `projectatlas` native runtime. It accepts the same subcommands and flags; there is no separate short-command API or database. Prefer it for work in one exact checkout. Run from that checkout, inspect `atlas --format json runtime-info` when version or executable identity is uncertain, and stop on typed root/schema/freshness errors. Use `projectatlas` when the forwarder is unavailable or native-binary identity itself is under test. For scripts/CI, request `--format json` and `--require-version <expected>`; set `PROJECTATLAS_NO_TELEMETRY=1` for read-only review/CI smoke.

| Command | Function; call when |
| --- | --- |
| `atlas init` | Create/verify project-local config, DB, initial index, and host configs; only for a root without initialized local state. |
| `atlas watch --once` | Incrementally refresh changed files after edits or an offline interval; do not run at every session start. |
| `atlas scan` | Full repository index refresh; only for a missing publication in an initialized project, explicit rebuild, or typed full-refresh guidance. |
| `atlas next <query>` | Rank task-relevant folders/files and suggest a next inspection; normal first navigation call for one checkout. |
| `atlas overview` | Show broad indexed structure; use when layout itself is the question or `next` has no actionable candidate. |
| `atlas folders <query>` | Rank candidate work areas before files when ownership is unclear. |
| `atlas files [query]` | Rank files in the chosen folder, or use `--file-pattern` for bounded glob discovery. |
| `atlas summary <file>` | Get structured file responsibility, symbols, calls, and coverage after selecting a file. |
| `atlas outline <file>` | Get a compact selected-file outline when summary context is insufficient. |
| `atlas search <pattern>` | Search indexed text with optional file pattern, context, regex, or fuzzy mode; inspect completeness and truncation. |
| `atlas slice <file>` | Read an exact line or declaration range after selecting it; copy returned selectors. |
| `atlas symbols list` | Find declarations in a selected file or bounded query. |
| `atlas symbols relations` | Inspect callers, dependencies, imports, documentation bridges, or bounded graph analysis when a summary is insufficient. |
| `atlas symbols slice` | Read one exactly disambiguated declaration's current source. |
| `atlas symbols build` | Rebuild a missing/stale deep symbol projection only when typed guidance or an explicit request calls for it. |
| `atlas health` | Inspect filtered structural, purpose, or parser-coverage findings when planning repair/refactoring; `health-check` is the compatibility spelling. Use `atlas health resolve` only for an inspected, intentional deterministic conflict with a rationale; fill a missing purpose instead. |
| `atlas lint` | Run the project purpose/structure/untracked gate, normally `--report-untracked --purpose-level low`; broaden strictness only intentionally. |
| `atlas purpose queue` | Get bounded missing/suggested purpose work; use `purpose set` for an inspected correction and `purpose review` for an approved batch. |
| `atlas token` | Report local or control-plus-worktree token estimates when asked; `--view tui` is the human terminal dashboard. |
| `atlas config --print` | Inspect effective scan and ignore policy before changing selection. |
| `atlas ignore list` | Inspect Atlas-only stricter exclusions; `ignore add/remove` changes them, `ignore init-gitignore` creates a missing Git ignore file only when needed. |
| `atlas root show` | Inspect selected root, DB, config, and runtime; `root status` checks Git-worktree structure, `root verify` checks binding, and `root set` explicitly rebinds. |
| `atlas settings` | Locate local cache/index settings when diagnosing an installation. |
| `atlas watch-status` | Check watcher availability/status; `atlas watch` runs a continuous foreground refresh loop for a deliberately managed session. |
| `atlas runtime-info` | Report executable/version/capabilities for installation or mismatch diagnosis. |
| `atlas parity report` | Audit CLI/MCP intelligence parity for diagnostics or release/CI proof, not ordinary navigation. |
| `atlas mcp-config` | Print a host MCP config with absolute runtime paths when explicitly configuring a host; `atlas mcp` starts the stdio server and is not an orientation command. |
| `atlas snapshot export/import` | Move a portable derived-only graph snapshot when explicitly requested; validate identity and import preconditions first. |
| `atlas parser-pack status/verify/install/enable/update/disable/remove` | Inspect or explicitly manage the optional parser pack; never change it as a side effect of navigation. |
| `atlas map` | Explicit AtlasMap compatibility output when requested; not required for normal index, navigation, or lint. |
| `atlas strip-legacy-purpose` | Preview legacy `.purpose` cleanup first; apply only after migration has been verified. |
| `atlas reset-index` | Preview derived-index/cache removal first; apply only with explicit authority and preservation of authored state. |

MCP remains the right route for registered alias inventory/add/remove, safe control-atlas hydration of an uninitialized registered worktree, concurrent per-call `worktree` selection, compact `atlas_session_brief` and typed continuations, or labelled cross-worktree `atlas_symbol_relations(worktrees: [...])`. The CLI has no equivalent of those control-catalog and federated calls. From a single target worktree, ordinary `atlas` commands operate on that worktree's own private index.
