## 1. Baseline and Cache Contract

- [x] 1.1 Record the exact Linux and Windows cold construction timings and define the dependency-layer, invalidation, trust, and clean-release boundaries.
- [x] 1.2 Add focused workflow-policy checks for every key-input category, exact-key restore, trusted-only save, and explicit clean-construction bypass.

## 2. Reusable Cargo Layer

- [x] 2.1 Validate restored Cargo target trees with fixed path, entry-type, entry-count, and byte bounds, quarantining invalid state before a clean rebuild.
- [x] 2.2 Remove all seven ProjectAtlas-owned package artifacts before restored state is used and before dependency state is saved, including the non-Cargo Windows broker.
- [x] 2.3 Cover valid reuse plus path-indirection, unexpected-entry, entry-limit, byte-limit, and quarantine fallback behavior in the existing construction diagnostics suite.

## 3. Workflow Integration

- [x] 3.1 Pin official GitHub cache restore/save actions and derive exact target keys from Rust, native toolchain/SDK, lockfile, manifests/features, and cache-policy inputs.
- [x] 3.2 Add cache-free dispatch mode, restore-only pull-request behavior, trusted successful save, and bounded per-target disposition receipts.
- [x] 3.3 Preserve exact-candidate, offline construction, deterministic assembly, digest, native-import, license, containment, lifecycle, package, and fresh-runner verification.

## 4. Documentation and Proof

- [x] 4.1 Add the CI dependency-layer and clean-release flow to the owning Mermaid architecture views, render it with Mermaid CLI, and inspect semantic and visual correctness.
- [ ] 4.2 Run the construction diagnostics, workflow syntax/policy checks, Rust formatting/check/Clippy/tests affected by the Linux launch fix, strict OpenSpec validation, and IssueOps synchronization.
- [ ] 4.3 Run trusted cold and unchanged-input Linux/Windows constructions, publish the cache receipts and timing comparison, and keep reuse only where contained construction improves by at least 60 percent.
- [ ] 4.4 Record one successful explicit empty-cache Linux and Windows construction on the final v0.4.0 candidate before release acceptance.
