## 1. Configuration

- [x] 1.1 Add `projectatlas mcp --nearest-project`.
- [x] 1.2 Add `projectatlas mcp-config --nearest-project`.
- [x] 1.3 Add generated-config/root-set support so the setting can be persisted for MCP hosts.
- [x] 1.4 Add per-call `nearest_project` overrides for path-bearing MCP tools.

## 2. Routing Behavior

- [x] 2.1 Keep explicit `project_path` isolation ahead of nearest-project routing.
- [x] 2.2 Discover the nearest ancestor `.projectatlas/projectatlas.db` without creating databases.
- [x] 2.3 Reject missing DBs, partial `.projectatlas/` folders, invalid DB files, DB/config root mismatches, and symlink/junction ambiguity.
- [x] 2.4 Return normal filesystem guidance when no valid indexed ProjectAtlas DB exists.
- [x] 2.5 Add selected-project audit metadata to routed cross-project read responses.

## 3. Verification

- [x] 3.1 Add focused MCP routing tests for default-off, startup-on, per-call overrides, explicit `project_path`, nested indexed roots, and out-of-project fallbacks.
- [x] 3.2 Add read-only DB probe tests proving nearest discovery does not create WAL/SHM sidecars.
- [x] 3.3 Update agent documentation and plugin skill guidance for opt-in nearest routing and fallback behavior.
- [x] 3.4 Run full Rust gates, OpenSpec validation, visual token TUI inspection, and ProjectAtlas lint.
