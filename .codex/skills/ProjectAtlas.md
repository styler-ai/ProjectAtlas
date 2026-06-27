# ProjectAtlas (Codex skill)

## Goal

Use ProjectAtlas as the atlas-first orientation layer before broad search, full-file reads, or symbol-level
inspection. The desired workflow is folder first, then file, then compressed details, then exact source.

## When To Use

- At the start of work in a repo that has `.projectatlas/config.toml`.
- When adopting ProjectAtlas in a new repository.
- After creating, moving, or deleting folders.
- After adding new source files.
- Before large refactors or cleanup decisions where folder/file intent matters.
- When the user asks how many tokens ProjectAtlas saved.

## First-Time Setup

1. Establish the project root first. ProjectAtlas stores one project-local index at `.projectatlas/projectatlas.db`.
2. Install the Rust binary if it is missing: `cargo install --path crates/projectatlas-cli --locked`.
3. Initialize the target repo with `projectatlas init --seed-purpose`.
4. Run `projectatlas scan`.
5. Add or import one-line purpose records for important folders and files.
6. Add summaries for non-source files to `.projectatlas/projectatlas-nonsource-files.toon` when needed.
7. Run `projectatlas map --force`.
8. Run `projectatlas lint --strict-folders --report-untracked` and fix every reported issue.

## Startup Workflow

1. Run ProjectAtlas from the established project root.
2. Run `projectatlas scan` or `projectatlas map --force` when the index may be stale.
3. Run `projectatlas overview`.
4. Run `projectatlas folders <query>` to choose the correct part of the repository.
5. Run `projectatlas files <query> --folder <path>` to choose target files; use `projectatlas files --file-pattern <glob>` when the file/path pattern is already known.
6. Run `projectatlas summary <file> --limit 25` before opening full source.
7. Run `projectatlas outline <file>` if the structured summary is not enough.
8. Run `projectatlas search <pattern> --file-pattern <glob>` for filtered text matches.
9. Run `projectatlas slice <file> --start-line <n> --end-line <m>` for exact source slices.
10. Run `projectatlas health-check` before cleanup/refactor decisions.
11. Escalate to broad source reads only after the selected files or slices are known.
12. Run `projectatlas token` when token-savings reporting is requested; use `projectatlas token --view tui` only for a human terminal dashboard.
13. Run `projectatlas lint --strict-folders --report-untracked` before finishing structural changes.

Token savings estimate avoided wrong-folder exploration, wrong-file opens, and unnecessary full-code reads caused by the atlas-first workflow. Agent and MCP surfaces should stay structured by default; the TUI dashboard is explicit terminal UI.

## Map Interpretation

- `overview` gives repository scale and purpose coverage.
- `folders` helps choose the working area by path and purpose.
- `files` narrows the file set within a folder and can use `--file-pattern` for direct glob discovery.
- `summary` gives structured deterministic file facts and purpose state.
- `outline` gives compressed source context and a token estimate.
- `search` finds literal, regex, or fuzzy text matches inside indexed files with optional path filters; it is case-insensitive by default for agent discovery.
- `slice` returns exact line ranges after a file is selected.
- `health-check` reports missing purposes, duplicate purposes, repeated temp/generated folders, and cleanup signals.
- `settings` and `watch-status` report local index/config/cache state; `reset-index --dry-run` previews local index/cache cleanup before `reset-index --apply`.
- `token` reports structured saved-token telemetry; `token --view tui` renders the human dashboard.
- Set `PROJECTATLAS_NO_TELEMETRY=1` for read-only review or CI smoke runs that must not write usage rows into `.projectatlas/projectatlas.db`.
- Generated compatibility output lives at `.projectatlas/projectatlas.toon`; durable non-source input lives at `.projectatlas/projectatlas-nonsource-files.toon`.

## Local Gates

For ProjectAtlas itself, run:

```bash
cargo fmt --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo test --doc --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo run -p projectatlas-cli -- map --force
cargo run -p projectatlas-cli -- lint --strict-folders --report-untracked
```

## References

- ProjectAtlas repository: https://github.com/styler-ai/ProjectAtlas
- `docs/projectatlas-3-architecture.md` for the target architecture.
- `docs/agent-integration.md` for AGENTS.md startup snippets.
- `docs/format.md` for TOON schema.
- `docs/workflow.md` for troubleshooting.
