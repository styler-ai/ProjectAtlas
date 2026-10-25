# Tasks

## 1. Contract and specification

- [x] 1.1 Validate the OpenSpec contract and map #645 to milestone v0.5.0-00, release parent/blocker #492, prerequisite #620, and the bounded startup/recovery scope.

## 2. Agent skill recovery

- [x] 2.1 Verify the actual supported host's subagent startup and manual/automatic compaction lifecycle in isolated configuration, documenting tested context delivery, trust, and checkout selection boundaries.
- [x] 2.2 Reuse the existing trusted read-only reminder for supported subagent startup, with causal hook regression checks and aligned package guidance while preserving root SessionStart behavior.
- [x] 2.3 Implement verified supported compaction recovery context or explicit persistent/manual fallback, and verify documented skill rereading and root reselection with RC3 CLI-first guidance without claiming unsupported automatic delivery.
- [x] 2.4 Prove real-host parent and two-subagent skill reads and correctly rooted Atlas navigation across supported startup/recovery, including wrong-root, missing-index, disabled/untrusted hook, and no-implicit-mutation compatibility boundaries.
- [x] 2.5 Reconcile changed package source contracts and integration documentation, then pass affected repository-native and hosted platform gates; use cargo fmt --check and affected Rust regression checks if Rust changes.
