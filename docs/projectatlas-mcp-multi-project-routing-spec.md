# ProjectAtlas MCP Multi-Project Routing Spec

Issue: https://github.com/styler-ai/ProjectAtlas/issues/273
Title: `bug(mcp): allow ProjectAtlas MCP to switch active project root`

## Summary

ProjectAtlas MCP must support multiple repositories from one MCP server process.
Today, a stdio MCP host that has one global `projectatlas` registry entry is
effectively pinned to the repository, database, and config path that were passed
when that MCP server started. That makes ProjectAtlas hard to use as a durable
global tool because every repository needs its own registered MCP server or a
registry overwrite.

The fix is a two-part routing model:

1. Add `atlas_set_project_path` to select the active default project for later
   MCP calls that omit `project_path`.
2. Add optional per-call `project_path` overrides to normal `atlas_*` tools so
   stdio clients can route each request deterministically without mutating
   process-active state.
3. Allow root-level compatibility `path` arguments to route to another
   repository only when that addressed root already has
   `.projectatlas/projectatlas.db`; never auto-initialize arbitrary outside
   directories from an implicit `path`.

`project_path` means repository root. It selects the `.projectatlas` database
and config for the request. Existing file, folder, symbol, purpose, and health
paths remain repository-relative paths inside that selected project.

`atlas_set_project_path` is a process-scoped convenience for single-client stdio
sessions. Hosts that share one server process across clients or issue
concurrent cross-project requests must pass per-call `project_path`; the active
default is not an isolation boundary.

## Severity And Blocker Impact

Severity: blocker for global MCP registration and multi-repository agent work.

Impact:

- A global Codex MCP registry entry can point at only one `--db` and
  `--config` pair. If the entry points to repo A, repo B receives repo A
  results until the registry is overwritten or a second server name is added.
- Agents lose the atlas-first workflow in repo B because `atlas_overview`,
  `atlas_folders`, `atlas_files`, search, slices, health, and settings are
  all answered from the wrong durable index.
- Workarounds are operationally fragile: rerun the installer, overwrite the
  global registry, or register one MCP server per repository.
- Write-capable tools such as scan, watch refresh, reset, purpose curation, and
  health resolution can mutate the wrong `.projectatlas` state if the server is
  bound to the wrong startup database.
- This blocks ProjectAtlas from behaving like a single reusable stdio MCP
  service across multiple local repositories.

## Current Failure

Current behavior is startup-bound:

- `projectatlas mcp` is launched with a single `--db` path and optional
  `--config` path.
- The MCP server stores those paths in server state.
- Read and write tools open the stored database path.
- Root-sensitive helpers derive the project root from the stored config,
  indexed database metadata, default `.projectatlas/projectatlas.db` layout, or
  process current directory.
- Path validation rejects paths outside the startup-selected project root.

Failure scenario:

1. Codex global MCP registry contains one server named `projectatlas`.
2. That entry was generated or repaired while working in repo A, so it launches
   with repo A's `.projectatlas/projectatlas.db` and config path.
3. The user opens repo B and asks for ProjectAtlas MCP context.
4. `atlas_overview` and related tools still read repo A's database.
5. Repo B cannot use ProjectAtlas MCP correctly unless the global registry is
   overwritten, a second MCP entry is registered, or the user falls back to the
   CLI.

This is a routing bug, not an indexing bug. The selected project context is
wrong before any normal MCP tool logic runs.

## Code Index Audit Findings

Reference source:

- Repository: https://github.com/johnhuang316/code-index-mcp
- Local audit clone:
  `<temporary-directory>/code-index-mcp-src`
- Audited commit: `5e8d5fce10a29a58b71f736824253fb22e239610`

Relevant mechanisms found in Code Index:

- `src/code_index_mcp/server.py` accepts startup project selection through
  CLI `--project-path` and environment variable `PROJECT_PATH`.
- `server.py` exposes MCP tool `set_project_path(path, ctx)` for runtime
  project selection.
- `project_manager_cache.py` caches managers per project path so repeated
  requests for one repository reuse existing project managers.
- `request_context.py` stores per-request project path in a Python
  `ContextVar`.
- `middleware/project_context_middleware.py` reads HTTP header
  `mcp-project-path` for HTTP/SSE per-request routing.
- `services/project_management_service.py` validates and initializes projects,
  updates request/server context, and starts watchers.

Important limitation:

- In stdio mode, Code Index's `set_project_path` is process-active state.
  The per-request `ContextVar` routing is driven by HTTP/SSE middleware, not
  by stdio request parameters.

ProjectAtlas should borrow the useful idea of an active selector, but improve
stdio routing by adding explicit `project_path` parameters on normal tools.
That gives deterministic request routing even when multiple host sessions share
one stdio MCP process.

## ProjectAtlas Design

### Routing Model

Each MCP call resolves a `McpProjectState` before opening a store, loading
config, validating paths, rendering settings, recording telemetry, or mutating
state.

Resolution precedence:

1. If the call includes non-empty `project_path`, use that project for this
   call only.
2. Otherwise, use the active project selected by `atlas_set_project_path`.
3. If a root-level tool receives a `path` that does not resolve to the active
   project root, and that `path` names an existing directory that already has
   `.projectatlas/projectatlas.db`, use that addressed indexed project for this
   call only.
4. Otherwise, use the startup project derived from `--db`, `--config`, indexed
   root metadata, or the default `.projectatlas/projectatlas.db` layout.

Per-call `project_path` must not mutate the active project. It is an override
for exactly one request.

`atlas_set_project_path` mutates only the MCP server's active default. It must
not scan, initialize, create a database, or rewrite config by itself.

### Project State

Use a small typed state object:

```rust
struct McpProjectState {
    root: PathBuf,
    db_path: PathBuf,
    config_path: Option<PathBuf>,
}
```

The state must be built from a canonical project root:

- `root` is the canonical repository root.
- `db_path` defaults to `root/.projectatlas/projectatlas.db`.
- `config_path` is the first existing supported config path:
  `root/.projectatlas/config.toml`, then `root/projectatlas.toml`.
- If an explicit config path is discovered from startup state, it may be used
  for the startup default only when it belongs to the same canonical root.
- If startup `--db` is the conventional `root/.projectatlas/projectatlas.db`
  for repo B and startup `--config` resolves repo A, reject or drop the
  mismatched config before any scan/watch operation can write repo A content to
  repo B's database.

The server should store:

```rust
struct ProjectAtlasMcpServer {
    startup_project: McpProjectState,
    active_project: Arc<RwLock<McpProjectState>>,
    session: String,
    tool_router: ToolRouter<Self>,
}
```

Clone the selected state before doing file I/O or SQLite work. Do not hold a
lock while scanning, opening SQLite, searching, rendering, or recording
telemetry.

### Tool API Contract

Add this tool:

```text
atlas_set_project_path(project_path: string) -> project state response
```

Response shape:

```text
project:
  root: "<canonical repository root>"
  db: "<selected .projectatlas/projectatlas.db>"
  config: "<selected config path or null>"
  status: active
```

The response must be rendered from typed structs and enums, not ad hoc JSON
string literals.

Add optional `project_path` to normal tools, including:

- `atlas_overview`
- `atlas_folders`
- `atlas_files`
- `atlas_outline`
- `atlas_file_summary`
- `atlas_search`
- `atlas_slice`
- `atlas_symbols_build`
- `atlas_symbols`
- `atlas_symbol_relations`
- `atlas_health`
- `atlas_health_resolve`
- `atlas_token_report`
- `atlas_parity_report`
- `atlas_settings`
- `atlas_watch_status`
- `atlas_watch_once`
- `atlas_strip_legacy_purpose`
- `atlas_reset_index`
- `atlas_purpose_queue`
- `atlas_purpose_set`
- `atlas_purpose_review`
- `atlas_scan`

For parameter structs that already have a `path` field:

- `project_path` selects the repository root, database, and config.
- Existing `path` keeps its existing meaning.
- For root-level legacy tools where `path` previously meant repository root
  (`atlas_scan`, `atlas_watch_once`, and legacy cleanup surfaces), keep `path`
  only as a selected-root assertion. It may be omitted, `"."`, `"./"`, or the
  selected root itself. It may select another repository only when the
  addressed root already has `.projectatlas/projectatlas.db`; this is a
  bounded compatibility route, not arbitrary discovery or initialization.
- If `path` resolves outside the active or per-call `project_path`, return a
  validation error unless it is an already indexed ProjectAtlas root. The
  error must tell the agent to pass `project_path`, call
  `atlas_set_project_path`, or use ordinary filesystem tools for
  out-of-project files.
- New clients must prefer `project_path`.

For path fields that are repository-relative today, keep them
repository-relative inside the selected project:

- `atlas_outline.file`
- `atlas_file_summary.file`
- `atlas_slice.file`
- `atlas_symbols.file`
- `atlas_symbol_relations.file`
- purpose `path`
- health `path_prefix`, `path`, and `related_path`
- query `folder` and `file_pattern`

### Validation Rules

For `project_path`:

- Accept absolute paths.
- Accept relative paths only by resolving against the MCP server process current
  directory. Clients that need deterministic multi-repo routing should pass an
  absolute path.
- Canonicalize before storing or using the path.
- Require the canonical target to exist and be a directory.
- Reject file paths.
- Preserve spaces and normal Windows drive/UNC paths.
- Use the canonical root as the cache key if any cache is added.
- Do not expand shell-specific syntax such as `~`, `%VAR%`, or `$VAR` unless a
  later requirement explicitly asks for it.

For repository-relative paths:

- Resolve against the selected project root.
- Canonicalize when reading disk paths.
- Reject paths that escape the selected project root.
- Error messages must name the selected project root and suggest either passing
  `project_path`, calling `atlas_set_project_path`, or using normal filesystem
  tools when the requested file intentionally lives outside the selected
  ProjectAtlas project.

### State And Concurrency

`atlas_set_project_path` is process-active state. It is useful for simple stdio
hosts and interactive sessions, but it is not request isolation.

Rules:

- Concurrent calls that include `project_path` are deterministic.
- Concurrent calls that omit `project_path` use whatever active state is visible
  when their request resolves state.
- Tool handlers must clone the active state at request start and keep that
  clone for the whole request.
- A later `atlas_set_project_path` call must not change the project used by an
  already-running scan, search, slice, or purpose write.
- Host integrations that can know the workspace root should pass `project_path`
  on every call instead of relying on process-active state.

### Store And Config Handling

Do not share one SQLite database across projects. Each selected root maps to
its own `.projectatlas/projectatlas.db`.

Preferred first implementation:

- Open `AtlasStore` per call using the selected state's `db_path`, matching the
  existing simple store-opening pattern.
- Do not add a connection pool or manager cache unless profiling shows it is
  necessary.
- If path/config resolution becomes noisy, cache only `McpProjectState` by
  canonical root, not long-lived SQLite connections.

Read-only tools should fail clearly when the selected database is missing or
not initialized. Write-capable refresh tools may create/update the selected
project's index only when that is already their documented behavior.

### Output Compatibility

Normal tool response payloads should stay compatible. Adding `project_path`
must not force every existing response to add a wrapper.

Control/status tools may report selected project state:

- `atlas_set_project_path` must report the active project.
- `atlas_settings` should report settings for the selected project and include
  selected root/db/config in the existing settings payload.
- `atlas_watch_status` should reflect the selected project database and watcher
  availability.

Error payloads should use the existing MCP error text shape unless this work
adds a typed error schema for all MCP errors.

## Rust-Specific Improvements

ProjectAtlas can improve on the Code Index approach by using typed Rust state
and explicit parameters instead of request-local globals.

Implementation preferences:

- Use `Arc<RwLock<McpProjectState>>` or the smallest equivalent thread-safe
  primitive for active project state.
- Use existing path helpers such as `canonical_project_root`,
  `default_mcp_project_root`, and config candidate discovery instead of adding
  new path semantics.
- Add a small `McpProjectContext` or equivalent helper only if it removes
  repeated `db_path/config_path/root` plumbing across handlers.
- Keep `AtlasStore::open` per request until there is measured evidence that a
  cache is needed.
- Keep all schema-bearing MCP payloads as `serde` structs/enums with
  `schemars::JsonSchema` derives.
- Use enums for control status values such as `active`, `missing_index`, or
  `invalid`, instead of raw strings.
- Use constants for tool names, command names, event names, and MCP schema keys
  where they must be reused.
- Centralize strings at the smallest owning boundary. Adapter-only contracts
  belong in the adapter module, service-owned rules belong in the service
  module, and only cross-crate public contracts should move to shared
  crate-level modules.
- Do not use thread-local or context-local globals for stdio routing.
- Do not add a new dependency for path expansion, caching, or synchronization
  unless the standard library and existing dependencies cannot cover it.

## Requirements

Functional requirements:

1. One stdio ProjectAtlas MCP process can serve at least two repositories.
2. `atlas_set_project_path` is listed in MCP tool discovery and in the parity
   surface check.
3. `atlas_set_project_path` accepts `project_path`, canonicalizes it, validates
   it as an existing directory, updates the active project, and returns typed
   project state.
4. Every normal `atlas_*` tool that reads or writes project state accepts an
   optional `project_path` parameter.
5. Per-call `project_path` overrides active state for that call only.
6. Calls that omit `project_path` use the latest active project selected by
   `atlas_set_project_path`.
7. Calls that omit `project_path` before any selector call use startup
   behavior, preserving existing project-local MCP configs.
8. File/folder/path parameters are interpreted relative to the selected
   project root, not the startup root.
9. Root-level `path` can route to another project only if that addressed root
   already has `.projectatlas/projectatlas.db`; otherwise it fails and does not
   create ProjectAtlas state outside the selected root.
10. Scan/watch/reset/purpose/write-capable tools mutate only the selected
   project's `.projectatlas` state.
11. `atlas_settings` reports the selected project state.
12. Errors for invalid paths, missing indexes, corrupt databases, and root
    escapes mention the selected project root.
13. Windows drive paths, paths with spaces, and canonicalized symlink targets
    work.
14. No global Codex MCP registry rewrite is required to switch from repo A to
    repo B at runtime.

Compatibility requirements:

1. Existing generated project-local MCP configs keep working.
2. Existing clients that call tools without `project_path` keep getting the
   startup-selected project until they call `atlas_set_project_path`.
3. Existing `path` behavior for root-level scan/watch style tools remains for
   selected-root assertions such as `"."`, `"./"`, and the selected root path.
   Cross-project access must be explicit through `project_path` or
   `atlas_set_project_path`.
4. Existing response shapes for normal data tools remain stable unless a
   response already represents project state.

Documentation requirements:

1. This spec is the implementation contract for issue #273.
2. After implementation, update user-facing MCP docs and agent instructions to
   prefer per-call `project_path` for host integrations that know the workspace
   root.
3. Generated MCP config docs should continue to describe project-local configs
   as supported, but no longer as the only way to use multiple repositories.

## Non-Goals

- Do not implement HTTP/SSE `mcp-project-path` header routing in this issue.
- Do not add a repository picker UI.
- Do not add a global repository registry database.
- Do not merge multiple repositories into one ProjectAtlas SQLite database.
- Do not require every host to register one MCP server per repository.
- Do not auto-initialize arbitrary directories during `atlas_set_project_path`.
- Do not start long-running per-project watcher daemons as part of project
  selection.
- Do not change CLI command behavior outside the MCP routing path.
- Do not remove support for generated project-local MCP configs.
- Do not add a broad source-code string-literal ban that flags normal prose,
  docs, tests, or user-facing messages.

## Edge Cases

- `project_path` does not exist: return validation error; active state remains
  unchanged.
- `project_path` is a file: return validation error; active state remains
  unchanged.
- `project_path` is relative: resolve against server process current directory
  and return canonical absolute root in the response.
- `project_path` includes spaces: preserve it through schema parsing,
  canonicalization, response rendering, and store opening.
- `project_path` points through a symlink: use the canonical target as the
  project root and state key.
- Windows paths differ only by case: canonicalization decides equality.
- Selected root has no `.projectatlas/projectatlas.db`: read-only tools return
  a clear missing-index error; scan/init-capable behavior is unchanged for
  tools that already create or update index state.
- Selected root has config but no database: settings can report config; data
  tools fail until the index exists or a refresh tool creates it.
- Selected root has database but no config: derive root from indexed metadata
  or default db layout and use default scan policy.
- Startup `--config` points to repo A but `project_path` points to repo B: the
  per-call state for repo B must use repo B config discovery, not repo A config.
- `atlas_scan` receives both `path` and `project_path`: if `path` resolves to
  the selected root, allow it; if not, return an error.
- `atlas_scan` receives `path` pointing at another repository and no
  `project_path`: if that path is an existing directory with
  `.projectatlas/projectatlas.db`, route this one call to that indexed project;
  otherwise reject it. The agent must either pass `project_path`, call
  `atlas_set_project_path`, or use ordinary filesystem tools for files outside
  the selected ProjectAtlas project.
- `atlas_slice.file` is `../repo-a/src/lib.rs` while selected root is repo B:
  reject the root escape.
- `atlas_set_project_path` runs while another request is scanning the old
  active project: the running request keeps its cloned state; future calls use
  the new state.
- Two clients share one stdio server and alternate active defaults: calls that
  omit `project_path` are process-active and can race by design; clients should
  pass per-call `project_path` for isolation.
- A stale manual MCP registry passes repo B's default DB with repo A's config:
  startup must not retain repo A's config for repo B and must not scan repo A
  into repo B's SQLite database.
- `atlas_reset_index` includes `project_path`: reset only that selected
  project's index/cache files.
- Token telemetry must record against the selected database/session, so token
  reports do not mix repo A and repo B.

## Implementation Touch Points

Primary files:

- `crates/projectatlas-cli/src/mcp.rs`
  - Add `atlas_set_project_path`.
  - Add shared `project_path` parameter support.
  - Add active project state to `ProjectAtlasMcpServer`.
  - Resolve selected state at the start of each handler.
  - Replace direct `self.db_path` / `self.config_path` use in handlers with the
    selected state.
  - Update `REQUIRED_MCP_TOOL_NAMES` and generated router parity checks.
  - Keep path validation inside the selected root.
  - Render project state with typed structs/enums.
- `crates/projectatlas-cli/src/runtime.rs`
  - Add or reuse a helper that resolves a canonical root into MCP project
    state: root, db path, and config path.
  - Keep config candidate logic shared with existing MCP config generation.
  - Ensure startup default and per-call project resolution use the same rules.
- `crates/projectatlas-cli/src/main.rs`
  - Keep MCP command startup compatible.
  - Ensure parity reporting knows about the new required MCP tool.
  - Avoid changing generated config shape unless needed for docs.
- `crates/projectatlas-service/src/lib.rs`
  - No routing state should live here unless a service API currently assumes a
    startup root. Service functions should receive store/root inputs from the
    selected MCP state.
- `crates/projectatlas-core/src/toon.rs`
  - Prefer existing `encode_agent_payload` over new render code.
  - Add typed render helpers only if they remove duplicate schema rendering.

Tests can live in the existing crate test structure. Do not add a new test
framework for this feature.

No database schema change is expected.

## Strict String-Contract Lint Requirement

MCP schema and control/status payloads must not be built from repeated ad hoc
string literals. ProjectAtlas must enforce this with a strict cargo-adjacent
source lint, not only with convention or a brittle grep.

Stable Rust does not support repository-defined Clippy plugins as a normal
portable workflow. The ProjectAtlas gate is therefore a workspace binary
invoked beside Clippy:

```text
cargo run --locked -p projectatlas-lints --bin cargo-projectatlas-lints -- strict-strings
```

For local convenience, `.cargo/config.toml` may expose the same gate as:

```text
cargo projectatlas-lints strict-strings
```

Strings covered by this requirement include, at minimum:

- `project`
- `root`
- `db`
- `config`
- `status`
- `active`
- future control statuses such as `missing_index` or `invalid`

Required implementation style:

```rust
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
struct McpProjectStateResponse {
    project: McpProjectStatePayload,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
struct McpProjectStatePayload {
    root: String,
    db: String,
    config: Option<String>,
    status: McpProjectStatus,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
enum McpProjectStatus {
    Active,
    MissingIndex,
    Invalid,
}
```

Centralization placement:

- Prefer typed `Serialize` structs/enums for response schemas and status
  values.
- Use constants for repeated command, route, event, mode, or schema key
  strings when typed serialization is not a natural fit.
- Keep constants/enums near the owner first: MCP adapter strings in
  `mcp.rs`, service-domain strings in the service module, database/domain
  strings in the database/core crate, and shared public contracts in a shared
  module only when multiple crates truly depend on them.
- Do not create a broad global constants file for unrelated strings.
- Do not centralize one-off diagnostics, rustdoc prose, or test fixture prose
  when doing so would create false abstraction.

Lint coverage:

- Add `crates/projectatlas-lints` as a workspace package with a binary named
  `cargo-projectatlas-lints`.
- Parse Rust source with `syn` and inspect real syntax nodes plus macro token
  streams. Do not grep source text.
- Scope the initial hard-fail rule to `crates/projectatlas-cli/src/mcp.rs`.
  Within that file, ban unreviewed production string literals and maintain a
  narrow allowlist for required format templates or macro-required literals.
  Do not ban common words such as `status` repository-wide.
- Ignore comments and docs by construction through Rust parsing.
- Attribute literals are intentionally out of scope for this lint because
  serde, clap, cfg, test, and documentation attributes require literals in
  valid Rust syntax. Typed serialization remains the preferred way to own
  schema strings.
- Allow `const` and `static` declarations as local centralization points.
- Add tests showing macro literals and direct expression literals fail, while
  constants, typed serialization, attributes, comments, and non-exact prose
  pass.
- Run the strict string lint in CI, release verification, and the local
  pre-push hook.

Acceptance for this requirement:

- `project`, `root`, `db`, `config`, `status`, and `active` appear as struct
  fields, enum variants, constants, schema output, or test expectations, not as
  scattered ad hoc JSON construction.
- A future rename or status addition requires changing the typed schema in one
  obvious place.
- The strict string-contract lint fails on newly introduced protected inline
  literals in the guarded source file and does not fail on comments, attributes,
  one-off prose, or constants.

## Tests

Unit tests:

- Resolve startup default from `--db` and optional `--config`.
- Resolve `project_path` into canonical root, db path, and config path.
- Reject missing project paths and file paths.
- Reject repository-relative path escapes after project selection.
- `atlas_set_project_path` updates active state and returns typed project
  state.
- Per-call `project_path` uses the override and does not mutate active state.
- Precedence is `project_path` override, then active default, then startup
  default.
- `atlas_scan` and `atlas_watch_once` reject `path` values that resolve outside
  the selected project root, with or without `project_path`.
- `atlas_settings` reports the selected project.
- Project-state response deserializes into typed structs/enums.
- Strict string-contract lint tests pass.

Integration tests:

- Create two temporary ProjectAtlas repositories, repo A and repo B.
- Start or instantiate one MCP server with repo A as startup default.
- Verify `atlas_overview` without override returns repo A.
- Call `atlas_set_project_path` with repo B.
- Verify `atlas_overview` without override returns repo B.
- Verify `atlas_overview` with `project_path` repo A returns repo A and leaves
  active state as repo B.
- Verify `atlas_files`, `atlas_search`, and `atlas_slice` use the selected
  root, not the startup root.
- Verify a read-only tool with `project_path` pointing at an unindexed directory
  reports a missing index and does not create `.projectatlas`.
- Verify `atlas_scan { path: <repo-b> }` while active repo is A routes to repo B
  only after repo B already has `.projectatlas/projectatlas.db`; an unindexed
  repo B must fail cleanly.
- Verify a write-capable operation with `project_path` repo B touches repo B
  state and not repo A state.
- Verify MCP tool discovery includes `atlas_set_project_path` and schemas show
  optional `project_path` on normal tools.

Suggested local gates with explicit timeouts:

```powershell
cargo fmt --check
cargo test -p projectatlas-cli mcp_multi_project -- --nocapture
cargo run -p projectatlas-lints --bin cargo-projectatlas-lints -- strict-strings
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

If an end-to-end stdio JSON-RPC smoke exists, add one scenario that launches
one `projectatlas mcp` process and switches between two temp repos without
restarting the process.

## Acceptance Criteria

The issue is complete when:

- One stdio ProjectAtlas MCP server can answer repo A and repo B requests in
  the same process.
- `atlas_set_project_path` selects repo B as active after startup in repo A.
- Per-call `project_path` can query repo A while active state remains repo B.
- Normal tools use the selected project's database, config, root-relative path
  validation, and telemetry.
- Write-capable tools cannot accidentally mutate the startup project when
  `project_path` selects another project.
- Root/path parameters cannot make ProjectAtlas read or index outside the
  selected project except for the bounded already-indexed-root compatibility
  route. Out-of-project files require explicit project selection or normal
  filesystem tools.
- Tool schemas expose `project_path` consistently.
- Existing clients with no selector and no `project_path` keep startup-bound
  behavior.
- Generated project-local MCP configs remain valid.
- No global Codex MCP registry overwrite is needed for normal repo switching.
- The strict string-contract lint requirement is covered.
- The focused multi-project tests and standard Rust gates pass.

## Rollback And Mitigation

Compatibility fallback remains the existing model:

- Generated project-local MCP configs can still launch one server bound to one
  repository.
- Users can still register distinct MCP server names per repository when they
  need hard process isolation.
- Existing clients that never call `atlas_set_project_path` and never pass
  `project_path` keep startup-bound behavior.

If multi-project routing introduces a regression:

- Revert only the selector/override routing layer while preserving existing
  startup-bound MCP behavior.
- Keep project-local MCP config generation as the operational mitigation.
- Document that hosts should temporarily register one ProjectAtlas MCP server
  per repository until issue #273 is fixed again.

Do not delete existing index data as part of rollback. The feature changes MCP
routing, not the ProjectAtlas database format.
