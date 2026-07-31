## 1. MCP Test Client Lifecycle

- [x] 1.1 Replace the stale-read block's close-immediately batch helper with the existing persistent MCP contract session, route the all-tools inventory and advertised-tool call owner through that session so every required response arrives before shutdown, retain one server/project binding per contract, attempt bounded explicit shutdown after both successful validation and request/assertion failure, and fault-test primary-error precedence.
- [x] 1.2 Preserve exact typed `refresh_required` assertions for the summary, search, relations, files, slice, and deleted absolute-selector read paths without production or dependency changes.

## 2. Routing and Verification

- [x] 2.1 Map issue #416 to this change, synchronize its live checklist, pass `openspec validate drain-mcp-test-responses-before-eof --strict`, and run `.github/scripts/issue-checklists.py`.
- [x] 2.2 Pass formatting, warnings-denied workspace Clippy, repeated focused `normal_reads_do_not_serve_offline_stale_index_state` and `mcp_advertised_tools_own_their_real_sqlite_effects` E2E runs, the complete E2E target, the repository pre-push gate, and the exact Windows release package regression with explicit hard timeouts.
