## Why

ProjectAtlas already uses Cargo workspace dependencies, a committed lockfile, locked builds, and `cargo-deny`, but the dependency lifecycle is incomplete: two owned dev dependencies remain locally versioned, Cargo updates are not automated, repository security updates are disabled, the hosted `cargo-deny` version floats, and current policy tests do not reconcile these contracts. A single Cargo-native maintenance contract is needed so agents can inspect, update, and verify every internal and third-party Rust dependency without adding another dependency manager.

This is a separately tracked backlog change. Its planning artifacts are ready for implementation, but implementation follows the lean #309 IssueOps restoration so dependency updates enter through the stabilized `dev` workflow.

## What Changes

- Make the root `[workspace.dependencies]` table the single version owner for every direct dependency used by an owned workspace crate, including dev dependencies, while preserving member-owned feature selection and excluding external-repository fixtures.
- Keep `Cargo.lock` committed and use Cargo metadata/tree commands as the authoritative direct and transitive inventory.
- Add weekly Cargo Dependabot updates targeting `dev`, keep major updates individually reviewable, and enable repository Dependabot alerts and security updates.
- Pin the hosted `cargo-deny` executable version and keep advisory, yanked, license, duplicate-version, and source policy explicit and current.
- Document the normal agent update/review loop and protect it with one coherent repository-policy test plus the ordinary locked Rust/workspace checks.
- Keep dependency updates review-driven; do not auto-merge them.

### Non-goals

- No custom dependency registry, updater service, bot, Rust crate, or second dependency manager.
- No automatic dependency PR merge and no bypass of ordinary CI or human/agent review.
- No hardcoded resolved-package or duplicate-version totals that drift from Cargo's authoritative graph.
- No product runtime or MCP behavior change.

## Capabilities

### New Capabilities

- `rust-dependency-management`: Central ownership, deterministic resolution, automated updates, policy enforcement, and review of ProjectAtlas Rust dependencies.

### Modified Capabilities

None.

## Impact

- Workspace manifests: root `Cargo.toml` and owned member `Cargo.toml` files.
- Deterministic resolution and policy: `Cargo.lock`, `deny.toml`, and the pinned Rust/tool versions used by CI.
- Automation: `.github/dependabot.yml`, repository Dependabot security settings, and `.github/workflows/ci.yml`.
- Agent workflow and behavior proof: `docs/workflow.md` and focused repository-policy E2E coverage in `projectatlas-cli`.
- No additional production or development dependency is expected.
