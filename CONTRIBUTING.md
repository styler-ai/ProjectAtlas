# Contributing

Thanks for the interest in ProjectAtlas. At the moment, we are not accepting external code contributions.

If you spot a bug or have a suggestion, please open an issue with clear reproduction steps or a concrete proposal.

## Internal workflow

- Feature work lands on `dev`.
- Releases are merged from `dev` into `main` via PR.
- Ensure `dev` includes the latest `main` changes before releasing.
- Use `python scripts/prepare_release.py --issue <NNN> --bump patch` to create the release branch and PR.
- Add `--post-release` if you want the next `.dev` bump PR on `dev`.
- CI checks must pass before merge.
- PR titles or bodies must reference a GitHub issue (for example `#123`).
- Install git hooks with `python scripts/install_hooks.py` so commits require issue references.
- Apply `type:*`, `priority:*`, and `status:*` labels to every issue.
- Keep public issues/PRs/release notes free of private or internal-only details.
- The pre-push hook runs `python scripts/check_all.py`; install the `build` module first.
