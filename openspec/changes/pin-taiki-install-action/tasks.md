## 1. Immutable workflow pin

- [x] 1.1 Update the existing `taiki-e/install-action` uses in `.github/workflows/ci.yml` and `.github/workflows/release.yml` to the verified `v2.87.5` commit `5bf6ce016fd2e72eefc647cbca1e4213f65955b8`, preserve `cargo-deny@0.20.2` and all surrounding workflow behavior, and verify the exact two-use source diff plus the affected repository checks.

## Verification

Run the exact workflow source and upstream tag checks, `openspec validate pin-taiki-install-action --strict --no-interactive`, the repository IssueOps/checklist gate, and the normal affected pre-push and hosted CI checks. The repository affected-proof planner and normal hook select required checks; do not infer exemptions from the absence of Rust source changes.
