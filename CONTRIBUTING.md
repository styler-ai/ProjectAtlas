# Contributing

Thanks for the interest in ProjectAtlas. At the moment, we are not accepting external code contributions.

If you spot a bug or have a suggestion, please open an issue with clear reproduction steps or a concrete proposal.

## Internal workflow

- Feature work lands on `dev`.
- Releases are merged from `dev` into `main` via PR.
- Ensure `dev` includes the latest `main` changes before releasing.
- Update the Cargo workspace version in `Cargo.toml` when preparing a release.
- Release tags must match the Cargo version, for example `v0.3.1`.
- Use the `02-Release` workflow for release publication; it validates the Rust workspace, builds Linux/macOS/Windows archives, creates the tag, and uploads the artifacts to the GitHub Release.
- CI checks must pass before merge.
- PR titles or bodies must reference a GitHub issue (for example `#123`).
- Install git hooks by copying or linking files from `.githooks/` into `.git/hooks/`.
- Apply `type:*`, `priority:*`, and `status:*` labels to every issue.
- Keep public issues/PRs/release notes free of private or internal-only details.
- Anonymize benchmark corpora, fixtures, reproduction paths, and public issue examples before committing or referencing them publicly.
- The pre-push hook runs the Rust verification stack: format, check, clippy, tests, rustdoc, map, and lint.
