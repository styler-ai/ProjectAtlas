# Purpose: Document ProjectAtlas local workflow troubleshooting and verification commands.

# Workflow and Troubleshooting

ProjectAtlas is designed to run locally with a project-local SQLite atlas and optional TOON exports.

## Recommended workflow

1. `projectatlas init` (first-time setup).
2. Run `projectatlas scan` to update the SQLite index.
3. Run `projectatlas config --print` when effective scan, purpose, or exclusion policy is unclear.
4. Run `projectatlas overview`, `projectatlas folders <query>`, and `projectatlas files <query>` before broad source reads; use `projectatlas files --file-pattern <glob>` for direct glob discovery.
5. Run `projectatlas summary <file> --limit 25` before opening full files.
6. Run `projectatlas outline <file>` when line-level compressed context is still needed.
7. Run `projectatlas lint --report-untracked --purpose-level low`.
8. Run `projectatlas map --force` only when a compatibility TOON snapshot is explicitly needed.
9. Open a PR that references the GitHub issue (CI requires `#NNN` in title or body).
10. Install git hooks by copying or linking files from `.githooks/` into `.git/hooks/`.

For long local sessions, run `projectatlas watch` from the project root. It uses event-backed `notify`
watching with debounce/exclude handling and falls back to portable polling when the platform watcher is
unavailable. Ordinary file edits use partial SQLite/symbol refresh; directory/root/ignore-rule events use a
full scan for correctness. For bounded agent refreshes after edits, use `projectatlas watch --once` or MCP
`atlas_watch_once`.

Exact line slices validate the file through the atlas database, then read the current file from disk. Symbol slices
use the stored symbol ranges, then read current disk content, so keep the watcher running during active edits if
symbol-level slices matter.

## One-command local verification

Run the full local check suite with Cargo:

```bash
cargo fmt --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo test --doc --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo run -p projectatlas-cli -- lint --report-untracked
```

## Issue hygiene

- Every issue should carry a `type:*` label plus a `priority:*` and `status:*` label.
- Use `status:backlog` for unscheduled work.
- Any issue referenced by a PR must be assigned to the target release milestone (CI enforces this).
- Keep public issues/PRs/release notes free of private or internal-only details (release notes are generated from PR text).

## Review expectations

- At least one approval is required before merging.
- Automated reviews (Codex/Copilot) should be checked via `gh pr view <PR> --comments`
  or `gh pr view <PR> --json reviews`.

## Documentation site

- `04-Docs` builds Rust API docs with `cargo doc` and deploys the generated `target/doc` artifact to GitHub Pages.
- GitHub Pages should be configured for GitHub Actions deployment.

## Branching

- `dev` for active development.
- `main` for stable releases only.
- Merge `dev` -> `main` via pull request after CI is green.
- Ensure `dev` includes the latest `main` changes before releasing.
- Update the Cargo workspace version in `Cargo.toml`.
- Pushes to `main` create a GitHub release when the Cargo version is release-eligible.
- The auto-release workflow generates GitHub release notes from merged PRs.

## CI behavior

- CI refreshes the SQLite index with `projectatlas scan` and validates source metadata with `projectatlas lint`.
- `projectatlas lint` checks purpose/header health, non-source declarations, and untracked files; it does not require or validate the optional compatibility TOON export.
- `projectatlas lint --purpose-level low` is the default first-pass agent gate: stale, duplicate, and repeated temporary-folder findings fail, while missing/suggested/agent-review purpose curation for folders plus high-impact files remains advisory. Use `projectatlas purpose queue` for the actionable curation list, `--purpose-level medium` when all source files must be agent-reviewed, and `--purpose-level strict` only when every indexed file and folder must be agent-reviewed.
- PRs must reference a GitHub issue and have a milestone.
- CI can be run manually via `workflow_dispatch` when checks do not auto-trigger.

Environment toggles:

- `PROJECTATLAS_ALLOW_UNTRACKED=1` allows local builds while still reporting untracked files.
- `PROJECTATLAS_NO_TELEMETRY=1` runs read/orientation commands without recording usage rows in the local SQLite index.

## Troubleshooting

### Optional compatibility map export

Only older integrations need a static `.projectatlas/projectatlas.toon` snapshot. Generate it explicitly:

```bash
projectatlas map --force
```

Normal ProjectAtlas 3 agent workflows should read from `.projectatlas/projectatlas.db` through the CLI or MCP tools.

### Missing or suggested purposes

Do not add new Purpose headers or `.purpose` files for ProjectAtlas 3. Inspect the folder/file through the atlas funnel and write the correct one-line purpose to SQLite:

```bash
projectatlas purpose queue --limit 20
projectatlas purpose set <path> "<one-line purpose>"
```

The purpose queue is source-focused and folder-first by default, so binary assets, asset-only roots, and low-priority source files do not dominate the next-action list. Pass `--include-low-priority-files` only when intentionally doing broad file-purpose cleanup, and pass `--include-assets` only when intentionally curating non-source files. Generated purpose suggestions remain review-required until an agent approves or corrects them.

Purpose entries live in SQLite and are preserved across normal scans and deep index refreshes. Re-scanning keeps existing reviewed purposes for unchanged paths, marks changed approved files stale for review, and deactivates deleted/excluded paths instead of recreating purpose noise. Use the purpose queue or health output to approve only new or stale entries.

### Legacy Purpose headers or .purpose files

Legacy Purpose headers and `.purpose` files are migration inputs. Import them with `projectatlas scan`, then remove them only through an explicit migration command:

```bash
projectatlas strip-legacy-purpose --dry-run
projectatlas strip-legacy-purpose --apply
```

### Untracked files

Use `--report-untracked` to list non-source files. Either:

- add to the SQLite purpose/index state or, for compatibility, the non-source file list (`.projectatlas/projectatlas-nonsource-files.toon`)
- add to allowlists/exclusions
- move into an approved asset root

## Schema reference

The TOON schema is documented in `docs/format.md`.
