## 1. Root Selection

- [x] 1.1 Route every CLI default-root command through one resolver that validates an implicit conventional database root before SQLite access.
- [x] 1.2 Preserve explicit `--db`, explicit config, MCP `project_path`, generated MCP config, wrong-root, and missing-index behavior.

## 2. Non-Mutation and Compatibility

- [x] 2.1 Add future-schema E2E coverage proving typed implicit `worktree_required`, byte invariance, no new WAL/SHM sidecars, and truthful explicit schema refusal.
- [x] 2.2 Cover absent, compatible, older-supported, malformed, and pre-existing-sidecar root state without changing database, config, backup, purpose, or telemetry data.
- [x] 2.3 Preserve bare repository, common Git directory, linked-worktree isolation, CLI/MCP parity, and no-sibling-selection behavior.

## 3. Verification and Release Control

- [ ] 3.1 Run Rust formatting, warnings-denied Clippy, CLI unit tests, and complete CLI E2E tests on Windows.
- [ ] 3.2 Validate OpenSpec and issue checklist parity, then require behavior-relevant Linux, Windows, macOS x64, and macOS ARM64 hosted checks before closure.
