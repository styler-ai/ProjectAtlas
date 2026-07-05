## Context

ProjectAtlas already uses small Python scripts under `.github/scripts/` for GitHub CI orchestration where the main cost is GitHub API I/O, not local computation. This gate follows that pattern. Rust remains the right tool for ProjectAtlas product logic, source parsing, repository walking, and local performance-sensitive checks.

## Contract

The CI gate SHALL:

- run on pull request checks before merge,
- query the current PR's GitHub review threads,
- fail when a review thread is unresolved and at least one thread comment author is a GitHub Codex bot,
- print the path, line, author, outdated state, and thread/comment URL for every failing thread,
- pass only when there are no unresolved Codex-authored review threads.

The GitHub issue and PR surfaces SHALL:

- prompt bugs, improvements, and chores to carry an OpenSpec change id and visible `OpenSpec Tasks` section before planned implementation,
- remind PR authors that `openspec/issue-map.json` and mirrored issue task checklists must pass `.github/scripts/issue-checklists.py`, with every OpenSpec task checked before merge,
- require the canonical `OpenSpec Tasks` or `OpenSpec Task Checklist` section in the issue body, not a generic checklist heading or follow-up comment, for release/milestone readiness,
- rely on the CI checklist gate for enforcement instead of treating template text as sufficient.

The default Codex bot logins SHALL include:

- `chatgpt-codex-connector`
- `chatgpt-codex-connector[bot]`

## Implementation Notes

- Use `gh api graphql` so GitHub authentication and API endpoint configuration stay delegated to the GitHub CLI.
- Query `PullRequest.reviewThreads` with pagination.
- Query additional thread comments with pagination if a thread has more than the first page of comments.
- Keep the script small and self-tested with `--self-test`.
- Wire the check into `01-CI` as a PR-only step before Rust format/check/clippy/test work.
- Keep the OpenSpec template fields as comments/placeholders, not fake unchecked tasks, so milestone gates fail until real tasks are mirrored.
- Parse only exact OpenSpec task headings from issue bodies. Do not satisfy release readiness from comments or generic `Task Checklist` headings, because those can be stale or unrelated to the mapped OpenSpec change.

## Edge Cases

- GraphQL returns `chatgpt-codex-connector`, while REST can display `chatgpt-codex-connector[bot]`; both must match.
- Unresolved outdated threads still fail because outdated is not the same as considered and resolved.
- Non-actionable Codex comments should be replied to and resolved in GitHub; the script should not parse magic words.
- If GitHub adds a new Codex bot login, the script can accept `--bot-login` overrides without code changes.
- Resolution events may not always retrigger full PR CI; maintainers can rerun the check after resolving a thread.

## Pre-Mortem

Risk: the check becomes too broad and blocks unrelated human review threads.
Mitigation: filter only threads that contain a configured Codex bot author.

Risk: the check is easy to bypass because it lives only in PR prose.
Mitigation: make it a CI step whose failure fails `01-CI / verify`.

Risk: issue-template placeholder checkboxes satisfy or pollute the OpenSpec checklist gate.
Mitigation: use non-checkbox placeholders and let `issue-checklists.py` require real mirrored tasks before release.

Risk: a stale comment or unrelated generic task checklist satisfies milestone release readiness.
Mitigation: only parse exact OpenSpec task sections in the issue body; comments and generic task-checklist headings do not count.

Risk: the script times out on large PRs.
Mitigation: page at 100 threads/comments and do no local source scanning.

Risk: the gate leaks review comment body text into logs.
Mitigation: print only path, line, author, outdated state, and URL.
