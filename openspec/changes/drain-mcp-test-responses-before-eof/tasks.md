## 1. MCP Test Client Lifecycle

- [x] 1.1 Replace the stale-read block's close-immediately batch helper with the existing persistent MCP contract session, retain one server/project binding, attempt bounded explicit shutdown after both successful validation and request/assertion failure, and fault-test primary-error precedence.
- [x] 1.2 Preserve exact typed `refresh_required` assertions for the summary, search, relations, files, slice, and deleted absolute-selector read paths without production or dependency changes.

## 2. Routing and Verification

- [x] 2.1 Map issue #416 to this change, synchronize its live checklist, pass `openspec validate drain-mcp-test-responses-before-eof --strict`, and run `.github/scripts/issue-checklists.py`.
- [x] 2.2 Pass formatting, warnings-denied workspace Clippy, repeated focused `normal_reads_do_not_serve_offline_stale_index_state` E2E runs, the complete E2E target, and the repository pre-push gate with explicit hard timeouts.
