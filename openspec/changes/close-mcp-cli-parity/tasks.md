## 1. Inventory

- [x] 1.1 Add or update a central CLI-to-MCP parity inventory that includes the safe command families and reviewed exceptions.
- [x] 1.2 Verify `projectatlas mcp`, continuous `projectatlas watch`, and terminal `projectatlas token --view tui` are the only parity exceptions in #281 scope.

## 2. Implementation

- [x] 2.1 Add MCP tools for `atlas_init`, root diagnostics/binding, `atlas_config`, ignore management, `atlas_lint`, `atlas_runtime_info`, `atlas_mcp_config`, and `atlas_map`.
- [x] 2.2 Reuse existing CLI service/runtime helpers instead of duplicating behavior in MCP handlers.
- [x] 2.3 Add `project_path` support to each root-sensitive parity tool.
- [x] 2.4 Add e2e/MCP tests for read-only tools and isolated-temp-project smoke tests for mutating tools.

## 3. Documentation

- [x] 3.1 Update AGENTS/template guidance to prefer MCP parity tools before CLI fallbacks.
- [x] 3.2 Update agent integration docs and plugin skill instructions with parity tool names and reviewed exceptions.

## 4. Verification

- [x] 4.1 `cargo fmt --check`
- [x] 4.2 `cargo check --workspace --all-targets --all-features`
- [x] 4.3 `cargo test --workspace --all-features`
- [x] 4.4 MCP parity/e2e tests fail on unreviewed command-family gaps.
