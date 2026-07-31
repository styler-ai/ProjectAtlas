## 1. Harness Policy

- [x] 1.1 Split the test-only launch/admission no-progress allowance from the existing hostile operation allowance, retaining the short bound for deliberate pre-ready, admission, and progress stalls without retries or production changes.
- [x] 1.2 Add deterministic positive and negative classification coverage while preserving exact typed failure assertions, mandatory cleanup checks, and production admission constants.

## 2. Routing and Verification

- [x] 2.1 Map issue #397 to this change, synchronize its live checklist, pass `openspec validate stabilize-hostile-parser-fixture-admission --strict`, and run `.github/scripts/issue-checklists.py`.
- [x] 2.2 Pass `cargo fmt --check`, warnings-denied workspace Clippy, focused policy tests, and repeated complete Windows `parser_supervisor_adversarial` runs with explicit hard timeouts.
