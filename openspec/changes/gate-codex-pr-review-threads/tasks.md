## 1. Spec and Issue Setup

- [x] 1.1 Create the OpenSpec proposal, design, spec delta, and task list with pre-mortem risks.
- [x] 1.2 Create GitHub issue #299, assign it to v0.3.25, and map `gate-codex-pr-review-threads` in `openspec/issue-map.json`.
- [x] 1.3 Mirror this task list into #299 under `OpenSpec Tasks`.

## 2. CI Implementation

- [x] 2.1 Add a self-tested GitHub script that queries GraphQL review threads and fails unresolved Codex-authored threads.
- [x] 2.2 Wire the gate into `01-CI` for pull request verification before expensive Rust gates.
- [x] 2.3 Ensure the rule is scoped only to GitHub Codex review threads, not Claude Code or OpenCode installer behavior.
- [x] 2.4 Add OpenSpec task checklist prompts to bug, improvement, chore, and PR templates without fake checkbox tasks.
- [x] 2.5 Tighten `issue-checklists.py` so release readiness only counts exact OpenSpec task sections in issue bodies, not comments or generic task checklist headings.

## 3. Verification

- [x] 3.1 Run the script self-test locally.
- [x] 3.2 Run the script against PR #298 and confirm it detects the current unresolved Codex thread.
- [x] 3.3 Run OpenSpec validation and issue checklist validation.
- [x] 3.4 Add/update self-test coverage proving comments and generic task checklist headings do not satisfy the OpenSpec checklist gate.
