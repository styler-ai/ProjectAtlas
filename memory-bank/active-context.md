# Active context

## Goal and serial delivery

Finish v0.5.1-rc1 and independently verify the non-draft prerelease with Latest
disabled; v0.5.0 remains Latest. Stable publication and milestone closure need a
later instruction. Finish and normally merge one issue, verify both issue and PR
CLOSED, then start the next. Only #731 / PR #737 is active; no issue closed in this
execution. Primary implements; independent Luna Max reviews read-only.

## Current #731 work

Checkout `.worktrees/issue-731`, branch `fix/static-bindings-731`. Public committed
HEAD is f00cf1ea2aeb47ca0168b1c52ca5162a06f00a0f, source
f7daa7a4fb96ec083322afd2acabe992f31bbccb (DER19/SEM19). Its required native push
passed; CodeQL and PR-state passed. Hosted source CI was deliberately cancelled
following a confirmed C++ defect that needs a replacement candidate.

Current source advances DER20/SEM20. Qualified C++ function bodies inherit
an exact same-file namespace/class/struct/union environment. Unknown, ambiguous,
alias and template owners retain local/parameter facts behind a lookup barrier.
Qualified definitions do not inject bare file names or coalesce their separate raw
Function and Method prototype selectors. Nested declarators keep the terminal
function name. Using/base lookup remains conservative. Inline contributors retain charged name
blockers through nested/anonymous namespaces, preserving explicit direct paths
and unrelated global names without inferring promotion. Using-directive opacity
starts at its source position, preserving earlier calls. Typedef/using/namespace
aliases hide outer owners, including secondary typedef declarators.

The exact missing-only review example was already unresolved in stable0.5.0;
broader compiler-valid homonym cases exposed wrong global targets in stable0.5.0
and contract19. This confirmed stable defect must be fixed for RC1 under #731.
Canonical selector collisions remain conservative, including indistinguishable
same-line nested declarations. Grammar-ambiguous expression recovery is outside
new declaration authority; local-shadow proof uses a parsed auto declaration.

The accepted OpenSpec change is `normalize-supported-import-bindings`. Local/live
implementation progress is 5/9: 2.4/2.6/2.7 reopened, 3.1 and all five acceptance
rows remain open. Current plan validation and scoped IssueOps passed. One review
thread remains open for qualified C++ definitions. No publication/merge readiness
claim until current source, artifact, upgrades, costs and required gates pass.

## Verified evidence and required remaining proof

Historical source19: 443 symbols tests plus one ignored, all53 navigation tests,
required workspace/docs/scan/lint and native push passed. Immutable optimized19
binary SHA256 is cf89e368c04bba0f5578167ca565756342edc312c14915b53c433d452ba31fbf.
All15 native baselines (stable0.5.0/contracts5–18), repair/identity/approved-purpose/
full/no-op/deadline/compatible rollback and exact target probes passed. Its66
initialization measurements are33 pairs over11 fixtures, not66 pairs. These are
historical baseline evidence for20, not final20 proof or an issue708 SLO waiver.
Prior19 semantic digest for the new repair seed is
`d662ae01fce4a173b5b0e520a7cf0465d7b7184528b914afa24b9145c3efe8ed`.

Current20 work includes a42-case C++ owner matrix, regular/inline1300/1500 class name-admission
controls, and14 qualified-body additions to the existing permanent container
CLI/MCP selector. The compiler matrix has35 valid cases and seven deliberate
invalid/ambiguous unresolved controls. The earlier20 source pass had445 symbols
plus one ignored; required final checks follow review fixes. Independent review
found source-order opacity, typedef and inline-contributor blockers; all are
implemented. A compiler-valid parenthesized declarator reproduced the same wrong
global fallback, and its grammar-owned bounded unwrap is implemented. Final
source review is approved; optimized boundary, upgrade/cost proof and hosted
readiness remain pending.

Next: finish affected checks and independent review; commit20 source; build an
immutable optimized artifact; prove all16 native baselines through contract19,
current real CLI/MCP/refresh/full/no-op, repair/cancellation/identity/approved
purpose/rollback and affected cost/target cases. Reconcile support docs/inventories
and the exact live task mirror as each task passes. Final release integration
must use a derivation newer than20 and retain SEM20.

Then normally push the clean candidate with mandatory native gates, verify the
published head and all required hosted OS/Rust/repository/IssueOps/CodeQL checks,
resolve fresh feedback and independently complete five acceptance areas. Merge
normally and verify #731 and #737 CLOSED before touching #741. No gate bypass,
force push, database reset or stable promotion.

## Continuity and parked owners

Use installed version-matched stable0.5.0 ProjectAtlas before broad reads. Its
exact731 brief currently returns known policy_drift/full/changed0, tracked under
#739; preserve the private DB and use verified exact owning-file evidence instead
of repeating full orientation scans. Read the full skill after compaction.

#741 is next only after #731 closure; read its memory first. Preserve its owned
map/config edits. Preserve #739 root/watch/map changes and #738 security work.
#708 / PR #728 owns cooperative indexing performance and combined-source SLO
proof; existing failed/partial warm measurements remain visible without waiver.
#715 owns final integrated installed/hosted release acceptance. RC1 children are
708/718/721/722/731/732/738/739/741; 718/721/722/732 are closed. Accepted main
3ef603aad39c435cf130545a555f8d746ec745ba includes future-release graph work; do not
revert unrelated ownership or overwrite sibling working trees/private databases.
