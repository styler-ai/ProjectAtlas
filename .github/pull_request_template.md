## Summary

Describe what this change does and why.

## Issue

Refs #NNN

Use `Closes #NNN` only when this pull request completes the issue.

## Checklist

- [ ] `projectatlas scan` run when indexed context changed.
- [ ] `projectatlas lint --report-untracked --purpose-level low` passes, and `projectatlas purpose queue` has been reviewed for touched high-value paths.
- [ ] `cargo fmt --all --check` clean.
- [ ] `cargo check --workspace --all-targets --all-features --locked` clean.
- [ ] `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` clean.
- [ ] `cargo test --workspace --all-features --locked` and stable workspace doctests clean.
- [ ] `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --locked` clean.
- [ ] Tests updated or added where behavior changed.
- [ ] All active OpenSpec changes are mapped in `openspec/issue-map.json`, linked issue `Implementation Tasks` exactly mirror local task text and state, and the canonical `Acceptance and Review Tasks` remain truthful. `.github/scripts/issue-checklists.py` passes. Full issue and milestone completion is required only when closing or releasing, not for every incremental pull request.
- [ ] README, Markdown docs, and GitHub Pages/rustdoc-facing content are updated or confirmed current for this change.
- [ ] PR text contains no private or internal-only details (release notes are generated from PR text).
