## Summary

Describe what this change does and why.

## Issue

Refs #NNN

OpenSpec-Issue: #NNN
OpenSpec-Task: change-id/1.1

Repeat `OpenSpec-Issue` and `OpenSpec-Task` rows only for additional authoritative issues or disjoint task ranges changed by this PR. Use `change-id/1.1..1.4` for an inclusive range.

## Checklist

- [ ] `projectatlas scan` run when indexed context changed.
- [ ] `projectatlas lint --report-untracked --purpose-level low` passes, and `projectatlas purpose queue` has been reviewed for touched high-value paths.
- [ ] `cargo fmt --check` clean.
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings` clean.
- [ ] `cargo test --workspace --all-features` clean.
- [ ] `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features` clean.
- [ ] Tests updated or added where behavior changed.
- [ ] Declared OpenSpec task ranges are checked, evidence-current, exactly mirrored in their authoritative issues, and `.github/scripts/issue-checklists.py --pr <number>` passes; unrelated milestone tasks may remain open.
- [ ] README, Markdown docs, and GitHub Pages/rustdoc-facing content are updated or confirmed current for this change.
- [ ] PR text contains no private or internal-only details (release notes are generated from PR text).
