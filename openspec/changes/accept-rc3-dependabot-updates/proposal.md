## Why

Three open Dependabot PRs are blocked from RC3 acceptance by missing issue ownership, and the grouped Cargo update additionally fails warnings-as-errors on renamed RMCP API types. Upstream changelog issue numbers in bot PR bodies also confuse the shared PR-owner parser. The required PR-state workflow currently runs from a pull-request merge commit, so its pre-checkout step is not a trusted ownership gate.

## What Changes

- Bind one explicit local ownership reference before incidental upstream changelog references in IssueOps and PR-state, while retaining one-owner and milestone enforcement. Run the required owner/milestone check from protected base-branch workflow code and publish current-head status through a trusted default-branch follow-up that supports Dependabot permissions, without executing PR-head code in either privileged path.
- Accept the existing jsonc-parser, install-action, and grouped Cargo bot PRs under existing aggregate #499 after refreshing each onto accepted main and proving its affected behavior.
- Adapt the grouped update to supported RMCP server/client configuration types and validate MCP and optional parser-pack behavior without weakening warnings, tests, or platform gates.
- Reconcile the two issue/task owners, native parent/blocker relations, milestone, and release-owner installed acceptance before RC3 publication.

## Capabilities

### New Capabilities

- `explicit-pr-owner-binding`: unambiguous ProjectAtlas issue ownership for PRs whose bodies contain upstream changelog references.
- `rc3-dependency-update-admission`: issue-scoped acceptance of the three existing bot PRs with preserved parser, MCP, action-install, and parser-pack behavior.

### Modified Capabilities

None.

## Impact

IssueOps script and PR-state workflow; Cargo.toml/Cargo.lock, MCP server/client adapters, parser-pack package proof, and the pinned install-action workflow; OpenSpec issue map and RC3 release acceptance. No new product feature, database migration, removal of public commands, or stable promotion. This is ready for issue-scoped implementation after planning and release-graph synchronization.
