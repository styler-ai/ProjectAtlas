## 1. Central Cargo ownership and inventory

- [x] 1.1 Move every direct dependency version used by an owned workspace member, including `assert_cmd` and `predicates`, into root `[workspace.dependencies]`; use workspace inheritance in members while preserving member-specific features and excluding fixture manifests.
- [x] 1.2 Keep `Cargo.lock` committed and make locked Cargo metadata, full-tree, and duplicate-tree commands the documented direct/transitive inventory without adding a generated dependency ledger or hardcoded graph totals.

## 2. Automated updates and dependency policy

- [x] 2.1 Add weekly root Cargo Dependabot updates targeting `dev`, group only minor/patch updates, keep major updates separate, keep repository auto-merge disabled with no repository-owned auto-merge workflow, align GitHub Actions updates with `dev`, and distinguish configuration validated on `dev` from hosted scheduling activated after normal default-branch integration.
- [x] 2.2 Enable and verify repository Dependabot alerts and security updates through authenticated GitHub administration; document that any default-branch security PR remains unmerged until routed through and proven on `dev`.
- [x] 2.3 Pin the hosted `cargo-deny` version; run `cargo deny --locked --all-features check -D warnings`; include development-dependency duplicate edges; and make advisories, yanked crates, licenses, wildcards, sources, and duplicate families fail-closed or narrowly excepted by exact version with a reason and upstream-removal condition.

## 3. Agent workflow and behavior proof

- [x] 3.1 Document the Cargo-native manual and Dependabot update loop, including manifest/lockfile review, toolchain/MSRV, features, licenses, advisories, sources, duplicate paths, and upstream breaking changes.
- [x] 3.2 Extend one behavior-named repository-policy E2E test to reconcile all owned dependency table forms, workspace version inheritance, Dependabot and repository-owned auto-merge policy, the syntactically exact `cargo-deny` pin, locked/all-feature/dev-duplicate deny settings, the committed lockfile, and locked metadata resolution without duplicating the chosen tool version or adding per-task tests/evidence receipts.
- [x] 3.3 Run the focused policy test, `cargo metadata --locked --offline --format-version 1`, `cargo deny --locked --all-features check -D warnings`, format, locked workspace check, strict Clippy, workspace tests, stable doctests, warning-free rustdoc, strict source lints, OpenSpec validation, and lean IssueOps synchronization.
