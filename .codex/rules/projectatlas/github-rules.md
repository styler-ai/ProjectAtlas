# ProjectAtlas GitHub Rules

Purpose: Document the ProjectAtlas GitHub workflow so agents follow the same release and issue discipline.

## Branches
- `dev` is for active development.
- `main` is for stable releases only.
- All changes flow through PRs; direct pushes to protected branches are blocked.

## Issues and milestones
- Every PR must reference a GitHub issue (`#NNN`) in the title or body.
- Any issue referenced by a PR must be assigned to the target release milestone (CI enforces this).
- Apply `type:*`, `priority:*`, and `status:*` labels to every issue.

## Release workflow
- Use `python scripts/prepare_release.py --issue <NNN> --bump patch` to prepare a release from `dev`.
- Merge the release PR into `main` after CI is green; auto-release publishes the tag with generated notes.
- The post-release PR bumps `dev` to the next `.dev` version.

## Local verification
- Run `python scripts/check_all.py` before pushing (pre-push hook enforces this).
- If checks fail, fix issues before pushing or opening a PR.

## Public info hygiene
- Keep public issues/PRs/release notes free of private or internal-only details.
