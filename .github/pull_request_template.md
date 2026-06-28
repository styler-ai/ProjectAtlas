## Summary

Describe what this change does and why.

## Issue

Closes #NNN

## Checklist

- [ ] `projectatlas map` updated (map is current).
- [ ] `projectatlas lint --strict-folders --report-untracked` clean.
- [ ] `cargo fmt --check` clean.
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings` clean.
- [ ] `cargo test --workspace --all-features` clean.
- [ ] `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features` clean.
- [ ] Tests updated or added where behavior changed.
- [ ] PR text contains no private or internal-only details (release notes are generated from PR text).
