# Canonical ignore matcher input observation

This directory retains `ignore` **0.4.34**, authored by Andrew Gallant and
contributors, from the crates.io package with SHA-256
`484be5657551d7868d67587df967fb4fcbe9d266a761cc855aaa148fda0b3cc7`.
The package records upstream revision
`91f5d22ef48bde459d90a8c7dbeab416e97d0f2b`, path `crates/ignore`, in
[BurntSushi/ripgrep](https://github.com/BurntSushi/ripgrep).
All original package files were verified byte-for-byte against that archive
before applying the local changes. Original copyright and dual-license files
`COPYING`, `LICENSE-MIT` and `UNLICENSE` are retained. Redistribution uses the MIT
option; native runtime archives include `IGNORE-LICENSE-MIT` and this notice.

The local patch adds `IgnoreFileSource` and `IgnoreFileKind`, and forwards the
optional source through `GitignoreBuilder`, `WalkBuilder` and their existing
private hierarchy. It observes the actual rule and linked-checkout pointer
streams, plus absent candidates skipped by existing filesystem optimizations.
Parsing, glob matching, precedence, cache ownership and default missing/partial
error behavior remain with the canonical library. Without an explicit source,
the existing filesystem path remains active.

ProjectAtlas disables the library's automatic global Git configuration discovery
and explicitly adds the global rules selected by its own bounded direct-config
parser. That parser records its own exact consumed buffers and environment.
The new hook does not observe the library's automatic global configuration
selection; callers needing complete selection evidence must own that selection
separately. Repository-derived worktree exclusions are likewise captured and
revalidated by their existing ProjectAtlas filesystem owner.

The hook is necessary because matching endpoint policy files can bracket a
different policy consumed by a walk. Metadata-only fences, a precompiled global
matcher or a callback queue leave confirmed local/linked-worktree cases uncovered.
The upstream public API has no equivalent read observer or immutable matcher
injection. This patch avoids replacing the canonical matcher or performing a
second complete source walk.

Keep the patch auditable against the exact original archive. Its original-file
changes are limited to `src/lib.rs`, `src/gitignore.rs`, `src/dir.rs` and
`src/walk.rs`, plus an empty `Cargo.toml` workspace boundary for standalone
upstream tests inside a worktree. `src/input.rs` is additive. Upgrade/removal requires an official
equivalent consumed-input API and passing canonical-library tests, ProjectAtlas
policy/alias/resource regressions and all native release acceptance targets.
