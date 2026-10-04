# Contributing

Thanks for the interest in ProjectAtlas. At the moment, we are not accepting external code contributions.

If you spot a bug or have a suggestion, please open an issue with clear reproduction steps or a concrete proposal.

## Internal workflow

- Feature work uses short-lived branches targeting `main`.
- Keep change branches current with `main` before merge.
- Release candidates and stable releases publish only from the exact verified `main` head.
- Update the Cargo workspace version in `Cargo.toml` when preparing a release.
- Release tags must match the Cargo version, for example `v0.3.1`.
- Use the `02-Release` workflow for release publication; it validates the Rust workspace, builds Linux/macOS/Windows archives, creates the tag, and uploads the artifacts to the GitHub Release.
- CI checks must pass before merge.
- Give each PR one owning open issue and the same milestone. Prefer one standalone `Closes #123` line for complete delivery or `Refs #123` for partial delivery. Incidental changelog links do not override that explicit owner. Conflicting owners or mixed closing and non-closing declarations are rejected; older PRs without an explicit line retain the single-reference compatibility rule.
- Before every merge, independently verify the current PR head and the trusted PR-state workflow path, event, revision, API result, and actual required-gate readiness. The native `pr-state` job runs read-only validation from protected-base `pull_request_target` code without executing PR-head code; issue changes rerun that eligible source. A temporary unprivileged CI bridge may project the verified trusted publisher result during deployment only; remove it and the obsolete publisher before acceptance, then enforce the final event policy permitting only `pull_request_target` and `issues` for `pr-state.yml`. A green `workflow_run` API check alone does not prove GitHub evaluates the required gate. GitHub branch protection identifies `pr-state` by its check name and the shared GitHub Actions app; another workflow can emit the same name, so a green context alone does not establish trusted provenance.
- Install git hooks by copying or linking files from `.githooks/` into `.git/hooks/`.
- Apply `type:*`, `priority:*`, and `status:*` labels to every issue.
- Keep public issues/PRs/release notes free of private or internal-only details.
- Anonymize benchmark corpora, fixtures, reproduction paths, and public issue examples before committing or referencing them publicly.
- The pre-push hook validates the exact clean candidate and runs only the affected repository, Rust package or test-target, dependency, and ProjectAtlas checks; unknown or shared authority changes select the complete fallback.
