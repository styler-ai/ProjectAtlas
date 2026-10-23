## Why

The mandatory RC4 push gate now refuses rustls-webpki 0.103.15 and webpki-roots 1.0.9 for distrusted certificate intermediates (RUSTSEC-2026-0382 and RUSTSEC-2026-0381), and tokio 1.53.1 for I/O unsoundness (RUSTSEC-2026-0386). Reopen aggregate #499 for their compatible 0.103.16, 1.0.10 and 1.53.3 patches. Preserve prior accepted work, including the DOMPurify correction below, and deferred routine updates. This is a demonstrated dependency-policy failure; no ProjectAtlas exploit is claimed.

Earlier RC4 readiness reproduced the mandatory npm audit failure for locked DOMPurify 3.4.16: GHSA-cg2g-xh64-v4q7 is critical and GHSA-mrv2-3pqr-j5jr is high. The compatible 3.4.17 patch under #499 task 6 is accepted. Preserve this correction and earlier accepted dependency work; no deferred routine update enters RC4.

Three open Dependabot PRs are blocked from RC3 acceptance by missing issue ownership, and the grouped Cargo update additionally fails warnings-as-errors on renamed RMCP API types. Upstream changelog issue numbers in bot PR bodies also confuse the shared PR-owner parser. The required PR-state workflow currently runs from a pull-request merge commit, so its pre-checkout step is not a trusted ownership gate.

The earlier three updates are accepted. Final RC3 preparation exposed GHSA-p98j-92pf-mc4p in transitive DOMPurify 3.4.13; resolve it under the same aggregate #499 before publication.

The installed-package rehearsal subsequently failed the unchanged dependency gate
for rustls 0.23.45 (RUSTSEC-2026-0346 through RUSTSEC-2026-0349) and ruzstd 0.9.0
(RUSTSEC-2026-0344). Reopen #499 for compatible patches 0.23.46 and 0.9.1 before
refreshing #593 and restarting parser and installed-package acceptance.

Those patches are accepted. The next installed-package rehearsal refuses yanked
zip 0.6.6. The registry currently offers only non-yanked zip 9.0.3; replace the
pinned provider and adapt its existing consumers under #499 without weakening
the dependency gate or DOCX archive boundaries.

## What Changes

- Patch only rustls-webpki, webpki-roots and tokio to their compatible fixed versions for the RC4 dependency refusal; preserve manifest ranges and all unrelated locked packages, inspect consumer API compatibility, and prove the unchanged gate and affected platform/runtime boundaries before resuming RC4 acceptance.
- Bind one explicit local ownership reference before incidental upstream changelog references in IssueOps and PR-state, while retaining one-owner and milestone enforcement. Run the required owner/milestone check from protected base-branch workflow code and report a native current-head result from the eligible protected-base job, including Dependabot, without executing PR-head code in the protected validation job.
- Accept the existing jsonc-parser, install-action, and grouped Cargo bot PRs under existing aggregate #499 after refreshing each onto accepted main and proving its affected behavior.
- Adapt the grouped update to supported RMCP server/client configuration types and validate MCP and optional parser-pack behavior without weakening warnings, tests, or platform gates.
- Retain the accepted Mermaid lockfile correction to DOMPurify 3.4.16 and source-map-js 1.2.2, then advance only DOMPurify to compatible 3.4.17 for the RC4 audit refusal; prove parsing and dependency security under the existing aggregate without per-dependency issues.
- Retain the accepted rustls and ruzstd patches with their preserved manifest ranges, security policy and affected consumer proof.
- Replace yanked zip 0.6.6 with non-yanked 9.0.3 using the required compression features; adapt the existing DOCX reader and ZIP fixture consumers while preserving part identity, duplicate refusal, bounded expansion, and typed failures.
- Reconcile the two issue/task owners, native parent/blocker relations, milestone, and release-owner installed acceptance before RC3 publication.

## Capabilities

### New Capabilities

- `explicit-pr-owner-binding`: unambiguous ProjectAtlas issue ownership for PRs whose bodies contain upstream changelog references.
- `rc3-dependency-update-admission`: issue-scoped acceptance of the existing bot PRs, residual Mermaid/Rust security patches, and ZIP provider replacement with preserved DOCX, parser, token, MCP, action-install, and parser-pack behavior.

### Modified Capabilities

None.

## Impact

IssueOps script and PR-state workflow; Cargo.toml/Cargo.lock, MCP server/client adapters, parser-pack package proof, and the pinned install-action workflow; OpenSpec issue map and RC3 release acceptance. No new product feature, database migration, removal of public commands, or stable promotion. This is ready for issue-scoped implementation after planning and release-graph synchronization.
