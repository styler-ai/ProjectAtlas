## 1. Notify Watch Readiness

- [x] 1.1 Replace the fixed startup sleep in `notify_watch_refreshes_symbols_after_file_change` with one bounded readiness loop that requires the fixture's exact initial symbol in the selected database while the watch child remains live, reports a specific early-exit or readiness-deadline diagnostic, writes the changed source only after readiness, and preserves clean cycle-two exit plus changed-symbol assertions.

## 2. Routing and Verification

- [x] 2.1 Map issue #422 to this change, synchronize its live checklist, pass `openspec validate await-observable-watch-readiness --strict`, and run `.github/scripts/issue-checklists.py`.
- [x] 2.2 Pass `cargo fmt --check`, warnings-denied workspace Clippy, repeated focused Linux `notify_watch_refreshes_symbols_after_file_change` runs, the complete E2E and repository pre-push gates, the accumulated #409 persistent MCP and all-advertised-tool regressions, and hosted v0.4.2 prepublication proof with explicit hard timeouts.
