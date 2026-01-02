# Contributing

Thanks for the interest in ProjectAtlas. At the moment, we are not accepting external code contributions.

If you spot a bug or have a suggestion, please open an issue with clear reproduction steps or a concrete proposal.

## Internal workflow

- Feature work lands on `dev`.
- Releases are merged from `dev` into `main` via PR.
- Use `python scripts/prepare_release.py --issue <NNN> --bump patch` to open the release PR.
- CI checks must pass before merge.
- PR titles or bodies must reference a GitHub issue (for example `#123`).
- Install git hooks with `python scripts/install_hooks.py` so commits require issue references.
- Apply `type:*`, `priority:*`, and `status:*` labels to every issue.
