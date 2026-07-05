## Why

Cross-project MCP routing is path-sensitive and easy to get subtly wrong, especially on Windows. Rust can make this safer by representing selected roots, indexed roots, repository-relative paths, and absolute filesystem paths as distinct types, then backing the conversion rules with property tests.

## What Changes

- Add small typed wrappers for selected project roots, indexed project roots, repository-relative keys, and absolute filesystem paths at the MCP/runtime boundary.
- Move nearest-project conversion and selected-project checks behind typed helper APIs so tools cannot accidentally route across projects.
- Add property tests and table tests for Windows-style and Unix-style path cases, including nested projects, missing DBs, config root mismatches, absolute paths inside the selected root, and explicit `project_path` isolation.
- Defer the bounded async task-progress model to `openspec/changes/add-mcp-task-progress-model/` so this ready issue stays focused on routing safety.

## Capabilities

### New Capabilities
- `typed-project-path-routing`: Defines compile-time and test-backed safety boundaries for selected roots, indexed roots, repository-relative keys, and nearest-project routing.

### Modified Capabilities

## Impact

- Expected code touch points: `crates/projectatlas-cli/src/mcp.rs`, `crates/projectatlas-cli/src/main.rs`, `crates/projectatlas-cli/tests/e2e.rs`, and `crates/projectatlas-db/src/lib.rs`.
- Expected test touch points: unit tests plus MCP in-process/e2e tests; add property tests only if a future path parser needs a broader generated case space.
- This change is implementable in the current release because scope is limited to routing-safety types and tests.
