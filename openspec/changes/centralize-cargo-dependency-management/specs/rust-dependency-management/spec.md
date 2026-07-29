## ADDED Requirements

### Requirement: Root workspace owns every direct dependency version
ProjectAtlas SHALL declare every direct internal and third-party dependency version used by an owned workspace member exactly once under root `[workspace.dependencies]`, including normal, development, build, and target-specific dependency tables. Owned member manifests SHALL inherit those dependencies through Cargo workspace inheritance, while member-specific feature selection remains allowed and external-repository fixture manifests remain outside this invariant.

#### Scenario: Owned manifests use centralized dependency versions
- **WHEN** the root and all owned member manifests are inspected
- **THEN** each direct dependency version has one root workspace declaration and no owned member carries a local version literal

#### Scenario: Fixture manifests preserve external examples
- **WHEN** dependency ownership is checked across the repository
- **THEN** manifests used only as external-language or repository fixtures are excluded from the owned workspace invariant

### Requirement: Cargo provides the complete deterministic inventory
ProjectAtlas SHALL keep the repository-root `Cargo.lock` committed and SHALL use standard Cargo metadata and tree commands to derive the current direct and transitive graph. Normal build, check, test, documentation, packaging, and policy commands SHALL use the committed resolution where Cargo supports `--locked`, and no committed secondary dependency inventory SHALL duplicate Cargo's graph.

#### Scenario: Agent inspects the resolved graph
- **WHEN** an agent runs locked offline Cargo metadata after the normal locked fetch or build path
- **THEN** Cargo returns the complete resolved workspace graph from the committed manifests and lockfile without rewriting either file

#### Scenario: Agent traces ownership and duplicates
- **WHEN** an agent runs the documented locked Cargo tree and duplicate-tree commands
- **THEN** the output identifies the dependency paths responsible for each resolved crate and duplicate family

### Requirement: Hosted dependency updates are automated and dev-first
ProjectAtlas SHALL configure weekly Dependabot version updates for Cargo at the repository root and SHALL target the `dev` integration branch. Minor and patch updates MAY be grouped, major updates SHALL remain individually reviewable, repository auto-merge SHALL remain disabled, and no repository-owned workflow or action SHALL auto-merge Dependabot pull requests. The existing GitHub Actions update configuration SHALL follow the accepted `dev` integration policy.

#### Scenario: Routine Cargo update is proposed
- **WHEN** Dependabot detects eligible Cargo version updates
- **THEN** it opens a reviewed pull request for the repository root against `dev`, groups only configured minor or patch updates, and leaves major updates separate

#### Scenario: Dependency update reaches main
- **WHEN** a dependency update is ready for delivery
- **THEN** it has first passed the ordinary checks on `dev` and is not merged directly into `main` by automation

#### Scenario: Dependabot configuration exists only on dev
- **WHEN** the configuration has passed local and hosted checks on `dev` but has not reached the default branch
- **THEN** ProjectAtlas reports the configuration as validated but does not claim that weekly hosted scheduling is active

### Requirement: Advisory and security-update facilities are enabled
ProjectAtlas SHALL enable repository Dependabot alerts and security updates. Security-update pull requests SHALL remain review-driven and SHALL pass through the `dev` integration workflow before a corresponding change reaches `main`, including when GitHub originates the pull request against the default branch because of platform behavior.

#### Scenario: Vulnerable dependency is reported
- **WHEN** GitHub identifies a supported vulnerable dependency in the committed graph
- **THEN** the repository exposes the alert and security-update facility without auto-merging a change

#### Scenario: Security PR originates against the default branch
- **WHEN** GitHub cannot apply the version-update target-branch policy to a security update
- **THEN** the change remains unmerged until it is routed through and verified on `dev`

### Requirement: Dependency policy is pinned and fail-closed
ProjectAtlas SHALL install an exact reviewed `cargo-deny` version in hosted CI and SHALL run `cargo deny --locked --all-features check -D warnings`. The policy SHALL enforce advisories, yanked releases, license allowlisting, wildcard bans, registry/source restrictions, and duplicate versions including development-dependency edges. Every accepted duplicate family SHALL have a narrow exact-version exception with a reason and upstream-removal condition; a new unexplained duplicate family SHALL fail the repository policy contract.

#### Scenario: Safe dependency graph passes policy
- **WHEN** the committed graph contains only allowed licenses and sources, no disallowed advisory or yanked crate, no wildcard dependency, and only reviewed duplicate families
- **THEN** the exact pinned, locked, all-feature `cargo-deny` policy gate succeeds with no warnings

#### Scenario: Dependency policy drifts
- **WHEN** a dependency introduces a disallowed advisory, yanked release, license, wildcard, source, or unexplained duplicate family
- **THEN** CI fails with a diagnostic that identifies the dependency policy violation

### Requirement: Agents have one lean update and verification loop
ProjectAtlas SHALL document how an agent inspects, changes, and reviews a dependency using Cargo-native commands. Review SHALL cover resolved versions, the lockfile diff, Rust toolchain/MSRV compatibility, default and added features, licenses, advisories, sources, duplicate paths, and upstream breaking changes. One coherent behavior-named repository-policy test MAY prove several related checklist tasks; no per-task test or evidence receipt SHALL be required.

#### Scenario: Agent performs a targeted update
- **WHEN** an agent updates one dependency manually or reviews a Dependabot pull request
- **THEN** the documented loop produces a bounded manifest and lockfile change and runs the focused policy test, `cargo deny --locked --all-features check -D warnings`, and ordinary locked workspace gates

#### Scenario: Several policy tasks share one behavior test
- **WHEN** the repository-policy test validates the manifests, Dependabot configuration, policy-tool pin, deny policy, lockfile, and locked metadata behavior
- **THEN** that coherent test is sufficient behavior proof for the related tasks without unique test identifiers, task ledgers, or SHA receipts
