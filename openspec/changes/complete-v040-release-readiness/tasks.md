## 1. Release Ownership

- [x] 1.1 Land the bounded installer trust-boundary correction with focused Windows and POSIX behavior tests, the complete local gate, exact-head hosted CI, and all live review feedback resolved.
- [x] 1.2 Map `complete-v040-release-readiness` to issue #311, replace the obsolete #308 split with this exact checklist and its pre-mortem ownership, make milestone IssueOps reject open issues, and pass ordinary IssueOps synchronization.
- [x] 1.3 Create a dedicated non-milestone v0.4.0 post-release issue that owns publication verification and safe ProjectAtlas branch, worktree, and external-checkout cleanup.

## 2. Final Candidate

- [x] 2.1 Complete issue #341's explicit empty-cache Linux and Windows construction, reconcile and land its OpenSpec and GitHub checklist state, resolve live feedback, and close #341.
- [ ] 2.2 Merge every remaining v0.4.0 readiness change into `dev`, confirm #314 remains outside the release, and lock one corrected release-content head with no uncommitted source changes or unresolved review feedback.
- [ ] 2.3 Run the complete pre-push gate, strict OpenSpec validation, ProjectAtlas candidate scan and low lint, and IssueOps synchronization on the locked release-content head; confirm the correction adds no crate, dependency, schema, migration, SQLite write path, CLI/MCP schema, or second release path.

## 3. Prepublication Proof

- [ ] 3.1 Run exact-head `01-CI` and require Rust verification plus source-built CLI/MCP E2E smoke on Linux, Windows, macOS x64, and macOS arm64; dispatch `optional-parser-pack` with `clean_construction=true` and `target=all` on the same locked release-content head and require complete Linux and Windows construction, fresh-runner, runtime, aggregate proof, clean receipts, and the release handoff.
- [ ] 3.2 Run `02-Release` for `v0.4.0` with `prepublish_only=true` and the exact clean optional-parser run; require every package, optional-parser release-asset, and installer-smoke job to succeed on its supported platform.
- [ ] 3.3 Verify the prepublish run created no tag or GitHub release; reconcile package, optional-parser archive, installer, runtime, plugin, MCP, CLI, documentation, benchmark-input digest, and release-note identities; leave published checksum verification to #346; prepare a merge-commit `dev`-to-`main` promotion without merging it, with `main` as an ancestor and squash/rebase refused; re-audit PR threads plus Codex and Dependabot feedback; and keep `main`, tags, releases, and post-release issue #346 unchanged.

## 4. Exact-Head Review Corrections

- [ ] 4.1 Make explicit clean all-platform optional-parser construction emit one bounded release handoff; require `02-Release` and `03-Auto-Release` to bind the successful run and identical candidate tree, validate aggregate proof, clean receipts, versions, archive sizes and digests, and stage both supported archives plus aggregate proof as versioned release assets; update and render the owning architecture flow.
- [ ] 4.2 Restrict detailed and analysis federated rendezvous to exact typed external identities reached by the primary anchored traversal in the requested direction; bind every family read to the earlier caller or service deadline; use allocation-free exact serialized-byte accounting; and cover retained outbound matches, unrelated outbound exclusion, empty inbound rendezvous, both public routes, bounded work, deadline, cancellation, freshness, and compatibility without a schema, index, migration, transaction, or SQLite write change.
- [ ] 4.3 Replace the stale MCP composition raw-input SHA-256 in both published evaluation representations and make the existing release gate compare them with the actual raw file.
- [ ] 4.4 Run the focused workflow validator, benchmark-integrity, federated unit/integration, direction, negative, deadline, byte-accounting, analysis, and architecture-render checks; then disposition every live PR #351 Codex and Dependabot thread against the corrected head before relocking the candidate.

## 5. Finite Reconciliation

- [ ] 5.1 After tasks 1.1 through 4.4 are true, create one commit whose only repository change is checking those completed tasks and this reconciliation task, mirror #311 exactly, verify the diff is task-state-only, and require ordinary exact-head closure gates plus a fresh clean optional-parser handoff on the resulting promotion head.
