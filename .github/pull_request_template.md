## Summary

Describe what this change does and why.

## Issue

Closes #NNN

## Checklist

- [ ] `projectatlas scan` run when indexed context changed.
- [ ] `projectatlas lint --report-untracked --purpose-level low` passes, and `projectatlas purpose queue` has been reviewed for touched high-value paths.
- [ ] `cargo fmt --check` clean.
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings` clean.
- [ ] `cargo test --workspace --all-features` clean.
- [ ] `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features` clean.
- [ ] Tests updated or added where behavior changed.
- [ ] All OpenSpec changes are mapped in `openspec/issue-map.json`, all task checklists are mirrored into linked GitHub issues, every OpenSpec task is checked off before merge, and `.github/scripts/issue-checklists.py` passes.
- [ ] README, Markdown docs, and GitHub Pages/rustdoc-facing content are updated or confirmed current for this change.
- [ ] PR text contains no private or internal-only details (release notes are generated from PR text).
