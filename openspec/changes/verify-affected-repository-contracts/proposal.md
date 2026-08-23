## Why

ProjectAtlas currently pays for the complete repository proof chain on every pull request even when a closed, reviewable impact contract can prove that only a smaller set of existing checks is affected. The v0.5.0 CI path needs the leanest contract-complete proof without weakening any unit, integration, E2E, platform, security, dependency, packaging, installer, OpenSpec, IssueOps, Mermaid, or release boundary.

## What Changes

- Add one checked-in closed impact contract and one Python-standard-library planner for all human and Dependabot pull requests.
- Derive Rust reverse dependencies from `cargo metadata`; declare only cross-contract edges Cargo cannot know for database/schema, CLI/MCP, Python, Node/Mermaid, documentation/OpenSpec/IssueOps, installers/packages, fixtures/generated inputs, and platform gates.
- Run only affected build, test, and quality contracts for ordinary additions and modifications on pull requests. Every rename or deletion, plus unknown, shared, planner-owning, stale, or malformed input, fails closed to full proof even when all observed paths are otherwise classified.
- Always run full proof on the default branch, scheduled drift checks, and release paths.
- Keep every stable required context present by aggregating either affected success or trusted plan-bound not-applicable state; missing, skipped, canceled, stale, malformed, or mismatched evidence fails.
- Reuse #341's digest-addressed dependency layer and #366's input-based proof contract without adding a second classifier, build system, dependency graph framework, or proof ledger.
- Cancel only superseded read-only CI; never cancel IssueOps, release, deployment, or another mutation path.
- Apply the same planner and quality bar to Dependabot pull requests without a bot exemption. #372 remains the separate narrow timeout owner.

Non-goals:

- Removing, weakening, renaming away, or replacing any existing proof boundary.
- Adding a Rust crate, database/schema, service, provider/plugin framework, third-party Python package, or generalized build system.
- Reusing a plan across a different base/head, changed impact contract, workflow identity, platform, toolchain, or pull-request event.
- Making partial pull-request proof sufficient for default-branch, scheduled, candidate, or release acceptance.

The reviewed exact #497 body may be published while the planning pull request remains open solely so normal unfiltered IssueOps/CI can validate the real packet. That temporary body-to-`main` architecture-link gap authorizes no readiness or implementation. This specification remains candidate-only backlog planning until the planning artifacts and later objective repository mechanism are accepted on `main`, the exact published evidence is read back, and the separately promoted authoritative graph admits #497 after hosted bootstrap.

## Capabilities

### New Capabilities

- `affected-repository-contract-verification`: fail-closed affected-contract planning, execution, and stable required-context aggregation for human and Dependabot pull requests.

### Modified Capabilities

None.

## Impact

- Future Luna implementation is limited to the existing GitHub Actions workflows, repository policy scripts/tests, one checked-in impact-contract data file, and their documentation.
- Rust product code, the seven-crate boundary, CLI/MCP schemas, SQLite, installers, packages, and release artifacts remain unchanged; existing checks are selected, not replaced.
- Pull-request runner time and redundant builds decrease when a trusted plan proves a smaller affected set; worst-case work remains the current full proof.
