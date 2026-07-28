## 1. Release Ownership

- [x] 1.1 Land the bounded installer trust-boundary correction with focused Windows and POSIX behavior tests, the complete local gate, affected hosted CI, and all live review feedback resolved.
- [x] 1.2 Map `complete-v040-release-readiness` to issue #311, replace the obsolete #308 split with this exact checklist and its pre-mortem ownership, make milestone IssueOps reject open issues, and pass ordinary IssueOps synchronization.
- [x] 1.3 Create a dedicated non-milestone v0.4.0 post-release issue that owns publication verification and safe ProjectAtlas branch, worktree, and external-checkout cleanup.

## 2. Final Candidate

- [x] 2.1 Complete issue #341's explicit empty-cache Linux and Windows construction, reconcile and land its OpenSpec and GitHub checklist state, resolve live feedback, and close #341.
- [ ] 2.2 Merge every remaining v0.4.0 readiness change into `dev`, confirm #314 remains outside the release, and lock one corrected release-content head with no uncommitted source changes or unresolved review feedback.
- [ ] 2.3 Run the complete pre-push gate, strict OpenSpec validation, ProjectAtlas candidate scan and low lint, and IssueOps synchronization on the locked release-content head; confirm the correction adds no crate, dependency, schema, migration, SQLite write path, CLI/MCP schema, graph authority, or second release path.

## 3. Prepublication Proof

- [ ] 3.1 Run `01-CI` and require Rust verification plus source-built CLI/MCP E2E smoke on Linux, Windows, macOS x64, and macOS arm64; dispatch `optional-parser-pack` with `clean_construction=true` and `target=all` when its behavior-relevant inputs changed, otherwise reuse the matching successful handoff; require complete Linux and Windows construction, fresh-runner, runtime, aggregate proof, clean receipts, and the release handoff.
- [ ] 3.2 Run `02-Release` for `v0.4.0` with `prepublish_only=true` and the input-compatible clean optional-parser run; require every package, optional-parser release-asset, and installer-smoke job to succeed on its supported platform.
- [ ] 3.3 Verify the prepublish run created no tag or GitHub release; reconcile package, optional-parser archive, installer, runtime, plugin, MCP, CLI, documentation, benchmark-input digest, and release-note identities; leave published checksum verification to #346; prepare a merge-commit `dev`-to-`main` promotion without merging it, with `main` as an ancestor and squash/rebase refused; re-audit PR threads plus Codex and Dependabot feedback; and keep `main`, tags, releases, and post-release issue #346 unchanged.

## 4. Release Review Corrections

- [x] 4.1 Make explicit clean all-platform optional-parser construction emit one bounded release handoff; require `02-Release` and `03-Auto-Release` to bind the successful run to unchanged behavior-relevant release inputs, validate aggregate proof, clean receipts, versions, archive sizes and digests, and stage both supported archives plus aggregate proof as versioned release assets; update and render the owning architecture flow.
- [x] 4.2 Restrict detailed and analysis federated rendezvous to exact typed external identities reached by the primary anchored traversal in the requested direction; bind every family read to the earlier caller or service deadline; use allocation-free exact serialized-byte accounting; and cover retained outbound matches, unrelated outbound exclusion, empty inbound rendezvous, both public routes, bounded work, deadline, cancellation, freshness, and compatibility without a schema, index, migration, transaction, or SQLite write change.
- [x] 4.3 Replace the stale MCP composition raw-input SHA-256 in both published evaluation representations and make the existing release gate compare them with the actual raw file.
- [ ] 4.4 Run the focused workflow validator, benchmark-integrity, federated unit/integration, direction, negative, deadline, byte-accounting, analysis, and architecture-render checks; retain #308 task 7.4's system-scale and task 7.6's agent-navigation publications only when their behavior-relevant inputs and measured runtime, skill, tool, and artifact identities remain valid, rerun any affected campaign while retaining every scheduled or failed row, and disposition every live PR #351 Codex and Dependabot thread before locking the release candidate.
- [x] 4.5 Preserve all compatible cumulative token-impact history across the released v0.3.26-to-v0.4.0 database upgrade and later supported migrations; prove exact before/after overview and trend totals, reopen durability, and atomic rollback from a real released-schema fixture without resetting or replacing the project database.
- [x] 4.6 Keep the token-impact TUI limited to real persisted token, lookup, file-read, and modeled directory-walk data; show source-reconciled file reads avoided, their observed and modeled split, and persisted folder walks avoided; remove version, plain-control, and repeated-work comparisons; render one bounded connected, clustered, depth-cued non-interactive mini atlas from real resolved SQLite graph relations only in the wide layout using the existing indexed readers and one bounded deterministic one-shot force layout; support dark, light, and terminal-background themes; and pass deterministic arithmetic, data-source, graph-bound, connectedness, narrow-layout, and real-terminal visual review.

## 5. Finite Reconciliation

- [ ] 5.1 After tasks 1.1 through 4.6 are true, check the completed tasks and this reconciliation task, mirror #311 exactly, rerun cheap IssueOps, OpenSpec, review, topology, and policy gates, and reuse the clean optional-parser handoff unless its behavior-relevant inputs changed.
