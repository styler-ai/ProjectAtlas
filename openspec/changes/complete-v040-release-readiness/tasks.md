## 1. Release Ownership

- [x] 1.1 Land the bounded installer trust-boundary correction with focused Windows and POSIX behavior tests, the complete local gate, exact-head hosted CI, and all live review feedback resolved.
- [x] 1.2 Map `complete-v040-release-readiness` to issue #311, replace the obsolete #308 split with this exact checklist and its pre-mortem ownership, make milestone IssueOps reject open issues, and pass ordinary IssueOps synchronization.
- [x] 1.3 Create a dedicated non-milestone v0.4.0 post-release issue that owns publication verification and safe ProjectAtlas branch, worktree, and external-checkout cleanup.

## 2. Final Candidate

- [ ] 2.1 Merge every remaining v0.4.0 readiness change into `dev`, confirm #314 remains outside the release, and lock one exact candidate with no uncommitted source changes or unresolved review feedback.
- [ ] 2.2 Run the complete pre-push gate, strict OpenSpec validation, ProjectAtlas candidate scan and low lint, and IssueOps synchronization on the locked candidate; confirm this release-only change adds no Rust API, crate, dependency, schema, migration, SQLite write path, or runtime behavior.
- [ ] 2.3 Complete issue #341's explicit empty-cache Linux and Windows construction on the candidate, reconcile its OpenSpec and GitHub checklist, resolve live feedback, and close #341.

## 3. Prepublication Proof

- [ ] 3.1 Run exact-head `01-CI` and require Rust verification plus packaged CLI/MCP E2E smoke on Linux, Windows, macOS x64, and macOS arm64.
- [ ] 3.2 Run `02-Release` for `v0.4.0` with `prepublish_only=true` and require every package and installer-smoke job to succeed on its supported platform.
- [ ] 3.3 Verify the prepublish run created no tag or GitHub release and reconcile package, checksum, installer, runtime, plugin, MCP, CLI, documentation, and release-note identities against the exact candidate.

## 4. Promotion Readiness

- [ ] 4.1 Prepare an exact `dev`-to-`main` promotion, re-audit all PR threads plus Codex and Dependabot feedback, confirm every other `v0.4.0-00` issue is mapped, fully checked, and closed, and keep `main`, tags, and releases unchanged.
- [ ] 4.2 Reconcile the local and GitHub checklists after tasks 1.1 through 4.1 are complete, while keeping the post-release verification and cleanup issue open until v0.4.0 is published, independently verified, and safely consolidated.
