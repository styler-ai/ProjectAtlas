# Purpose: Document ProjectAtlas local workflow troubleshooting and verification commands.

# Workflow and Troubleshooting

ProjectAtlas is designed to run locally with a project-local SQLite atlas and optional TOON exports.

## Recommended workflow

1. `projectatlas init` (first-time setup, initial scan/index, generated MCP configs, and purpose handoff).
2. Run `projectatlas scan` or `projectatlas watch --once` later when you need to refresh the SQLite index.
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
cargo fmt --all --check
cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
cargo test --doc --workspace --all-features --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --locked
cargo run --locked -p projectatlas-lints --bin cargo-projectatlas-lints -- strict-strings
cargo deny --locked --all-features check -D warnings
python3 .github/scripts/issue-checklists.py --self-test
python3 .github/scripts/issue-checklists.py --repo "$(gh repo view --json nameWithOwner --jq .nameWithOwner)" --root . --issue-map openspec/issue-map.json
cargo run --locked -p projectatlas-cli -- --format json scan .
cargo run --locked -p projectatlas-cli -- lint --report-untracked --purpose-level low
```

## Rust dependency management

The root `[workspace.dependencies]` table owns every direct dependency version used by the seven ProjectAtlas workspace crates. Member manifests inherit those entries with `workspace = true`; fixture manifests remain independent because they model repositories ProjectAtlas scans rather than packages ProjectAtlas builds.

Use Cargo and the committed `Cargo.lock` as the complete dependency inventory:

```bash
cargo metadata --locked --offline --format-version 1
cargo tree --workspace --all-features --locked
cargo tree --duplicates --workspace --all-features --locked
cargo deny --locked --all-features check -D warnings
```

The offline metadata command is a deterministic inspection step after the normal locked fetch or build path. It is not a network bootstrap command, and its output is not committed as a second dependency ledger.

For a bounded manual update or Dependabot review:

1. Trace ownership with `cargo tree -i <crate> --workspace --all-features --locked`.
2. Change the version once in root `[workspace.dependencies]`, or run `cargo update -p <crate> --precise <version>` when only the lockfile resolution changes.
3. Review `git diff -- Cargo.toml Cargo.lock` and every affected member manifest.
4. Check the repository Rust toolchain and the dependency's MSRV, default and added features, license, advisories, registry or Git source, duplicate paths, upstream changelog, and breaking changes.
5. Run the focused repository-policy E2E test, the dependency inventory and deny commands above, and the ordinary locked workspace gates.

Weekly Cargo and GitHub Actions Dependabot updates target `dev`; only Cargo minor and patch updates are grouped, majors remain separate, and no repository automation merges them. Configuration merged only into `dev` is validated but does not become hosted-active until the later normal verified integration into the default branch. GitHub may originate a security-update pull request against the default branch; leave it unmerged until the same dependency change has been routed through and proven on `dev`.

## Lean implementation and IssueOps

ProjectAtlas uses the implementation loop that produced v0.3.26:

1. Implement a meaningful compiling behavior slice.
2. Add or update the smallest focused unit, integration, E2E, smoke, or validation test appropriate to the risk. One coherent test may cover several related tasks.
3. Run the ordinary locked Rust/workspace gates.
4. Commit and integrate significant working slices into `dev`.
5. Use normal CI and review, then synchronize the local OpenSpec checklist with its mapped GitHub issue.

Task completion does not require unique test identifiers, task-level verification plans or ledgers, commit receipts, rendered evidence comments, hosted links per checkbox, or post-merge issue sealing. GitHub Actions already records the commit and outcome of the normal checks.

Open mapped issues keep the concise v0.3.26 #305 planning shape: `Why`, `What Changes`, `Capabilities`, `Architecture Diagrams`, `Release Scope`, `Non-Goals`, `Pre-Mortem`, and one authoritative OpenSpec task section. `Architecture Diagrams` contains at least one durable HTTPS link to a versioned `dev/docs/*.md` view in this repository; reuse an existing architecture document when the change does not need a new one, and do not substitute `N/A`, a commit/SHA permalink, an external repository, or a duplicate PDF. The pre-mortem lists likely failures and visible mitigation checkboxes. Each mitigation ends with its owning task IDs, for example `(OpenSpec tasks: 2.1, 4.3)`, and is checked exactly when all referenced tasks are checked. This reuses the implementation checklist; it does not create mitigation-specific tests, receipts, or evidence artifacts.

Ordinary pull requests require exact local/GitHub checklist synchronization but do not require the whole release milestone to be complete. Full milestone checklist completion is a release-only gate. SHA-pinned Actions, locked Cargo commands, least privilege, parser/package/signature/digest validation, release checksums, and other executable integrity controls remain independent of task bookkeeping.

## Issue hygiene

- Every issue should carry a `type:*` label plus a `priority:*` and `status:*` label.
- Use `status:backlog` for unscheduled work.
- Any issue referenced by a PR must be assigned to the target release milestone (CI enforces this).
- Keep public issues/PRs/release notes free of private or internal-only details (release notes are generated from PR text).
- Keep issue completion free of commit/SHA permalink evidence and OpenSpec commit-link blocks; immutable Action pins and release checksums/signatures remain required integrity controls.

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
- Release archives are published with a `SHA256SUMS` asset. Verify manually with `sha256sum -c SHA256SUMS` or `shasum -a 256 -c SHA256SUMS` from a directory containing the downloaded archives.
- If a publish run fails after creating a tag or leaves a GitHub release with missing or stale assets, rerun the release workflow for the same version. The publish job recovers when the existing tag points at the current commit, creates the missing release when needed, and uploads release assets with replacement enabled for repair runs.

## CI behavior

- GitHub Actions runs Rust source, dependency, unit, E2E, documentation, and packaging checks. ProjectAtlas scan, purpose, parity, and lint maintenance run locally against the developer or agent's current source state, not against the hosted Actions checkout.
- `projectatlas lint` checks purpose/header health, non-source declarations, and untracked files; it does not require or validate the optional compatibility TOON export.
- `projectatlas lint --purpose-level low` is the default first-pass agent gate: duplicate and repeated temporary-folder findings fail, while missing/suggested purpose curation for folders plus high-impact files remains advisory. Use `projectatlas purpose queue` for the actionable curation list, `--purpose-level medium` when all source files must be agent-reviewed, and `--purpose-level strict` only when every indexed file and folder must be agent-reviewed.
- PRs must reference a GitHub issue and have a milestone.
- Ordinary PRs may reference an issue without closing it; use `Closes #NNN` only when the issue's complete checklist is ready to close.
- Active OpenSpec task lists must be mapped in `openspec/issue-map.json`, and their authoritative GitHub task sections must exactly mirror local text, order, ownership, and checked state.
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
projectatlas purpose review --from-file reviewed-purposes.json --apply
```

The purpose queue is source-focused and folder-first by default, so binary assets, asset-only roots, and low-priority source files do not dominate the next-action list. Pass `--include-low-priority-files` only when intentionally doing broad file-purpose cleanup, and pass `--include-assets` only when intentionally curating non-source files. Generated purpose suggestions remain review-required until an agent approves or corrects them.

Purpose entries live in SQLite and are preserved across normal scans and deep index refreshes. Re-scanning preserves accepted purpose text and approval across content, symbol, summary, and graph changes; automation never invalidates or overwrites it. Deleted/excluded paths become inactive while their path-owned accepted purpose remains dormant, and a rename does not transfer approval automatically. Use the purpose queue to approve missing/generated suggestions, or use the existing purpose APIs for an explicit correction when an agent, reviewer, or user finds an accepted purpose wrong. If a repository needs reproducible strict lint from a fresh checkout, keep a reviewed batch input in the repo and replay it with `projectatlas purpose review --apply`; do not edit the SQLite database by hand.

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
