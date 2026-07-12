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
cargo fmt --check
cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo nextest run --workspace --all-features --locked --profile ci
cargo test --doc --workspace --all-features --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --locked
cargo run --locked -p projectatlas-lints --bin cargo-projectatlas-lints -- strict-strings
cargo run --locked -p projectatlas-lints --bin cargo-projectatlas-lints -- test-quality policy --root . --policy test-quality.toml
cargo run --locked -p projectatlas-lints --bin cargo-projectatlas-lints -- test-quality configs --root . --policy test-quality.toml --nextest .config/nextest.toml --mutants .cargo/mutants.toml
cargo run --locked -p projectatlas-lints --bin cargo-projectatlas-lints -- test-quality tasks --root . --policy test-quality.toml --tasks openspec/changes/enforce-rust-test-quality-gates/tasks.md --plan openspec/task-verification.json --evidence openspec/task-evidence.json --expected-commit "$(git rev-parse HEAD)"
python3 .github/scripts/issue-checklists.py --self-test
python3 .github/scripts/issue-checklists.py --repo "$(gh repo view --json nameWithOwner --jq .nameWithOwner)" --root . --issue-map openspec/issue-map.json --verification-plan openspec/task-verification.json --evidence openspec/task-evidence.json
cargo run --locked -p projectatlas-cli -- --format json scan .
cargo run --locked -p projectatlas-cli -- purpose review --from-file .projectatlas/projectatlas-purpose-review.json --apply
cargo run --locked -p projectatlas-cli -- lint --report-untracked --purpose-level strict
```

## Rust test-quality gates

Install and verify the exact developer tools declared by `test-quality.toml`:

```bash
cargo install --locked cargo-nextest --version 0.9.140
cargo install --locked cargo-llvm-cov --version 0.8.7
cargo install --locked cargo-mutants --version 27.1.0
rustup component add llvm-tools-preview
cargo nextest --version
cargo llvm-cov --version
cargo mutants --version
```

`01-CI` reports nextest, stable doctests, LLVM coverage, and changed-source mutation as independent
blocking jobs. A pass in one dimension cannot replace another. Hosted command/job ceilings are read from
`test-quality.toml`: 20/25 minutes for nextest, 15/20 for doctests, 40/45 for coverage, and 45/50 for
changed-source mutation. Native nextest and cargo-mutants timeouts remain active inside those outer bounds.
Every job uploads its raw report, tool identity, outcome manifest, and diagnostics for 90 days with
`if: always()`; a successful upload never changes a failed quality conclusion.

Run local LLVM coverage without instrumenting unstable doctests:

```bash
mkdir -p target/projectatlas-quality/local-coverage
cargo llvm-cov clean --workspace
NEXTEST_PROFILE=ci cargo llvm-cov nextest --workspace --all-features --locked --json --output-path target/projectatlas-quality/local-coverage/coverage.json
cargo llvm-cov report --text --output-path target/projectatlas-quality/local-coverage/coverage.txt
```

Run changed-source mutation against a trusted merge base explicitly:

```bash
base="$(git merge-base HEAD origin/main)"
cargo mutants --config .cargo/mutants.toml --workspace --in-diff "$base..HEAD" --baseline run --timeout 180 --build-timeout 900 --output target/projectatlas-quality/local-changed-mutation
```

The expensive complete mutation run is not part of the pre-push hook. Dispatch the checked-in workflow,
which generates one unfiltered master inventory and executes exactly 16 native shards:

```bash
gh workflow run 05-full-mutation.yml --ref "$(git branch --show-current)" --field expected_sha="$(git rev-parse HEAD)"
```

Evidence lives below `target/projectatlas-quality/`. The committed verification plan and task ledger live at
`openspec/task-verification.json` and `openspec/task-evidence.json`. A task may be checked only after every
declared `TQG-UT-*` assertion has a current successful row for the tested implementation commit and covered-input
digest. The mapped GitHub checklist must then match `tasks.md` exactly; PR validation checks only the linked
`OpenSpec-Task` range, while release validation retains the full milestone gate.

The measured pre-gate snapshot was 286 runnable non-doctests across nine suites with zero ignored, 87.75% line,
84.90% region, and 86.28% function coverage with 3,369 missed production lines. The historical mutation listing
contained 4,911 candidates; the later unfiltered listing contained 4,931 after disabling native default call
skips. These are provenance-bound baselines, not current floors, 100% coverage, near-complete mutation strength,
or a no-bugs claim. Raw reports always remain visible. Narrow reviewed exceptions affect only the adjusted
denominator and require an owner, issue, approval, exact selector, rationale, source identity, and future expiry.
The hard v0.4 targets in `test-quality.toml` remain blocking even when a tracking issue exists.

Failure meanings are distinct: missing/mismatched tool, empty inventory, test failure, coverage below a floor or
target, mutation baseline failure, viable missed mutant, mutant timeout, command/job timeout, stale commit,
incomplete shard set, corrupt artifact, and IssueOps drift all fail closed. Reruns create a new run/attempt identity
and do not erase the earlier failure.

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
- Release archives are published with a `SHA256SUMS` asset. Verify manually with `sha256sum -c SHA256SUMS` or `shasum -a 256 -c SHA256SUMS` from a directory containing the downloaded archives.
- If a publish run fails after creating a tag or leaves a GitHub release with missing or stale assets, rerun the release workflow for the same version. The publish job recovers when the existing tag points at the current commit, creates the missing release when needed, and uploads release assets with replacement enabled for repair runs.

## CI behavior

- CI uses `projectatlas init` for first-run smoke coverage, refreshes the main ProjectAtlas repo index with `projectatlas scan`, replays the reviewed purpose batch with `projectatlas purpose review`, and validates source metadata with strict `projectatlas lint`.
- `projectatlas lint` checks purpose/header health, non-source declarations, and untracked files; it does not require or validate the optional compatibility TOON export.
- `projectatlas lint --purpose-level low` is the default first-pass agent gate: stale, duplicate, and repeated temporary-folder findings fail, while missing/suggested/agent-review purpose curation for folders plus high-impact files remains advisory. Use `projectatlas purpose queue` for the actionable curation list, `--purpose-level medium` when all source files must be agent-reviewed, and `--purpose-level strict` only when every indexed file and folder must be agent-reviewed.
- PRs must reference a GitHub issue and have a milestone.
- PRs declare their authoritative task scope with `OpenSpec-Task: <change>/<task-or-range>`; incomplete issues elsewhere in the milestone do not block an otherwise complete incremental PR.
- `05-Full-Mutation` runs weekly, manually, and from release with exactly 16 shards. It is intentionally not run by the local pre-push hook.
- `06-Task-Evidence-Render` runs only after same-repository `01-CI` pull-request runs, uses trusted default-branch code, validates run/artifact provenance, and never executes commands from issue or artifact content.
- Release runs require an exact main commit SHA and block package jobs until independent quality, full mutation, task evidence, and full-milestone checks all pass for that commit.
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

Purpose entries live in SQLite and are preserved across normal scans and deep index refreshes. Re-scanning keeps existing reviewed purposes for unchanged paths, marks changed approved files stale for review, and deactivates deleted/excluded paths instead of recreating purpose noise. Use the purpose queue or health output to approve only new or stale entries. If a repository needs reproducible strict lint from a fresh checkout, keep a reviewed batch input in the repo and replay it with `projectatlas purpose review --apply`; do not edit the SQLite database by hand.

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
