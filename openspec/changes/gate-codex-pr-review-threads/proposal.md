## Why

GitHub Codex review comments on ProjectAtlas PRs can identify real release risks, but a manual reminder in chat, PR text, or AGENTS guidance is not a merge gate. The repository needs a CI-enforced check that fails while a Codex-authored GitHub review thread is still unresolved, so a PR cannot silently merge with an unhandled Codex review.

## What Changes

- Add a PR CI gate that queries GitHub pull request review threads and fails when an unresolved thread contains a GitHub Codex bot comment.
- Keep the rule separate from Claude Code and OpenCode installer convergence; this is only about GitHub PR review threads.
- Use GitHub GraphQL review-thread state because REST review comments do not expose the resolved/unresolved thread status needed for a hard gate.
- Add a self-tested script under `.github/scripts/` and wire it into `01-CI`.
- Surface the existing OpenSpec task-mirror requirement in PR and issue templates so future bugs, features, improvements, and chores are shaped for the same CI-enforced checklist flow.

## Capabilities

### New Capabilities
- `codex-pr-review-thread-gate`: Defines CI enforcement for unresolved GitHub Codex review threads.

### Modified Capabilities
- Pull request CI checks.
- GitHub issue and PR templates for OpenSpec task visibility.
- Release readiness because v0.3.25 PRs now include this gate before merge.

## Release Scope

This change is scheduled for v0.3.25 and applies to ProjectAtlas GitHub pull requests. It does not add product runtime behavior.

## Non-Goals

- Do not classify whether a Codex comment is actionable with natural language.
- Do not gate Claude Code or OpenCode installer behavior through this rule.
- Do not require all human review threads to be resolved.
- Do not use `pull_request_target` or execute untrusted fork code with elevated permissions.

## Pre-Mortem

Likely failure modes:
- The gate checks REST review comments and cannot tell whether a thread is resolved.
- The gate only runs on PR creation/update, so later review comments do not refresh status.
- The bot login differs between REST and GraphQL responses.
- Outdated-but-unresolved threads are skipped even though nobody resolved them.
- The gate is hidden in documentation and does not block merge.

Mitigations:
- Query GraphQL `reviewThreads` and read `isResolved`.
- Trigger CI for pull request, pull request review, and pull request review comment events.
- Match both `chatgpt-codex-connector` and `chatgpt-codex-connector[bot]`.
- Fail any unresolved Codex thread, including outdated threads, until the thread is resolved.
- Run the script as a named CI step before expensive Rust gates.
