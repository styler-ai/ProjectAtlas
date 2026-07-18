## Context

ProjectAtlas has seven owned workspace crates. The root manifest already owns nearly all direct crate versions through `[workspace.dependencies]`, member crates inherit production dependencies with `workspace = true`, and the committed `Cargo.lock` resolves the full graph. CI uses `--locked` and runs `cargo deny check`, but Cargo version updates are not configured in Dependabot, repository Dependabot security updates are disabled, two CLI dev dependencies still own local versions, the hosted `cargo-deny` executable is not version-pinned, and duplicate-version policy can drift without failing a focused contract test.

This is repository maintenance for agents and maintainers, not a product runtime feature. Cargo, Cargo.lock, Dependabot, and `cargo-deny` remain the authorities.

## Goals / Non-Goals

**Goals:**

- Give every owned direct dependency one version declaration in the root workspace manifest.
- Make the complete resolved dependency graph inspectable and deterministic through standard Cargo commands and the committed lockfile.
- Route routine dependency updates through reviewed pull requests against `dev` and enable GitHub's advisory/security-update facilities.
- Fail unsafe, unlicensed, yanked, unapproved-source, wildcard, or unexplained duplicate dependency drift.
- Keep the update loop understandable to an agent and protected by one coherent behavior test plus ordinary workspace checks.

**Non-Goals:**

- No new crate, updater binary, service, registry, Renovate installation, or custom dependency database.
- No automatic merge of dependency updates.
- No versions centralized from fixture manifests that intentionally model external repositories.
- No static total for resolved packages or duplicate families.
- No change to ProjectAtlas runtime, MCP, database, or TOON contracts.

## Decisions

### 1. The root Cargo workspace owns direct dependency versions

Every direct dependency used by an owned member, including dev dependencies, is declared once under root `[workspace.dependencies]`. Members use workspace inheritance and may add only consumer-specific features or options that Cargo permits alongside workspace inheritance. Internal ProjectAtlas dependencies continue to use path plus workspace version ownership at the root.

Fixture manifests are excluded because their purpose is to represent external repositories rather than participate in the owned build graph.

**Alternative considered:** allow one-off member dev dependency versions. This is valid Cargo, but it defeats the requested one-place dependency inventory and makes automated version review less reliable.

### 2. Cargo metadata, Cargo tree, and Cargo.lock are the inventory

`Cargo.lock` remains committed. `cargo metadata --locked --offline --format-version 1` provides the machine-readable resolved graph after the normal locked fetch/build path; `cargo tree --workspace --all-features --locked` and `cargo tree --duplicates --workspace --all-features --locked` provide agent-readable ownership and duplicate paths. Cargo.lock supplies source identities and exact registry versions and checksums where applicable.

No generated dependency ledger or hardcoded inventory count is committed. Tests derive current facts from the manifests, lockfile, and Cargo output.

**Alternative considered:** maintain a second JSON/TOON dependency inventory. It would duplicate Cargo's source of truth and create drift without adding capability.

### 3. Dependabot owns hosted update discovery

Add a weekly Cargo update configuration for `/` with `target-branch: dev`. Minor and patch updates may be grouped; major updates remain separate so breaking changes receive focused review. The existing GitHub Actions update entry follows the same `dev` integration policy. Repository auto-merge remains disabled, and no repository-owned workflow or action auto-merges Dependabot pull requests.

Enable Dependabot alerts and security updates in repository settings. GitHub may originate security-update pull requests against the default branch independently of the version-update `target-branch`; such pull requests are never auto-merged and must be routed through and proven on `dev` before `main` changes.

Dependabot reads `.github/dependabot.yml` from the default branch. The configuration can be implemented and validated on `dev`, but weekly hosted scheduling becomes active only after the later normal verified `dev`-to-`main` integration. This change does not move or modify `main` early to activate it.

**Alternative considered:** add another update bot. Dependabot already owns the repository's Actions updates and satisfies the needed Cargo scheduling without another service or credential boundary.

### 4. `cargo-deny` remains the dependency policy gate

CI installs one exact reviewed `cargo-deny` version and runs `cargo deny --locked --all-features check -D warnings`. Version-pin changes are ordinary reviewed dependency-tool updates.

Duplicate versions, including families reached through development-dependency edges, become fail-closed through `multiple-versions = "deny"` and `multiple-versions-include-dev = true` unless the repository records a narrow exact-version exception with a reason and an upstream-removal condition. Broad `skip-tree` exceptions remain disallowed unless an actual bounded transitive cascade justifies one. The focused repository-policy test derives and reconciles current reviewed duplicate families instead of hardcoding totals.

**Alternative considered:** run separate audit/license/source tools. That would overlap the existing policy owner and increase tool/version surface without a demonstrated gap.

### 5. One coherent policy test protects the repository contract

Extend the existing behavior-named workflow/repository-policy E2E coverage so one test can prove several related tasks. It parses all owned normal, development, build, and target-specific dependency tables and reconciles root version ownership, member inheritance, Dependabot routing/schedule/grouping, repository-owned auto-merge absence, the syntactically exact `cargo-deny` pin and required deny sections, the committed lockfile, and locked metadata resolution. The test reads the chosen tool version from its owning workflow rather than duplicating that version as a second expected literal.

This follows the v0.3.26 engineering loop: focused behavior proof, ordinary Rust/workspace gates, review, and OpenSpec/GitHub checklist synchronization. There is no test identifier, per-task test, task evidence ledger, or SHA receipt.

## Pattern Fit

- **Chosen mechanism:** existing Cargo workspace tables, Cargo.lock, Cargo metadata/tree, Dependabot, `cargo-deny`, and one repository-policy test.
- **Simpler alternative:** only add a Cargo Dependabot block. It would leave local version ownership, disabled security updates, floating policy tooling, and duplicate-policy drift unresolved.
- **More complex alternative:** add a custom updater, dependency registry, crate, or service. None is justified because the native tools own the required contracts.
- **Principal invariant:** each owned direct dependency version has one root declaration; every build uses the committed resolution; updates are reviewed through `dev` and pass deterministic build plus advisory/license/source policy.

## Risks / Trade-offs

- **Dependabot security PRs may originate against the default branch** → never auto-merge them; route the change through `dev` and document the GitHub platform constraint.
- **Dependabot configuration on `dev` is not hosted-active yet** → validate it on `dev` and report scheduling as active only after the later normal verified integration into the default branch.
- **Grouped updates can obscure the cause of a failure** → group only minor/patch updates and split a failing group during diagnosis; keep majors separate.
- **Strict duplicate policy can block unavoidable transitive graphs** → permit only narrow reviewed exceptions with a reason and upstream-removal condition, derived from the current graph rather than a total.
- **Offline metadata fails before dependencies have been fetched** → document that it is a deterministic post-fetch/build inspection command, not a network bootstrap command.
- **Tool pinning requires maintenance** → update the pin intentionally in the same reviewed dependency workflow.

## Migration Plan

1. Move the remaining owned member version declarations into root workspace dependency ownership without changing resolved versions.
2. Add Cargo and integration-branch Dependabot configuration, then enable repository alerts/security updates through authenticated GitHub administration.
3. Pin `cargo-deny`, include development-dependency edges, and reconcile the current duplicate graph into narrow exact-version reviewed policy.
4. Update the agent workflow documentation and the single repository-policy E2E test.
5. Run locked metadata, dependency policy, focused E2E, and ordinary workspace gates on `dev`.

Rollback is configuration-only: revert the change on `dev` and disable the repository security-update setting if it causes platform behavior that cannot be safely routed. Do not rewrite Cargo.lock or `main` as a rollback shortcut.

## Open Questions

None. GitHub's default-branch behavior for security-update pull requests is treated as an explicit routing constraint, not as permission to bypass `dev`.
