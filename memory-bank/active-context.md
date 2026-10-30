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

Current source checkpoint b9b0ab5d34e9dc48fa5caa72a6f7411541ed8b27 advances
DER20/SEM20. Qualified C++ function bodies inherit
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
implementation progress is8/9: tasks1.1 and2.1–2.7 are complete. Task3.1 and all
five acceptance rows remain open pending required native/hosted gates and fresh
feedback. The qualified C++ review thread remains open until source20 is published
and verified. No issue is closed; public HEAD remains f00.

## Current20 verified evidence

Independent bounded source review is approved. Full445 symbols plus one ignored,
workspace Clippy and strict strings pass. The42-case owner matrix includes35
compiler-valid shapes and seven deliberate invalid/ambiguous unresolved controls.
Regular and inline1300/1500 class tests prove shared name admission, including
anonymous/inline composition and cancellation checkpoints. Nested/parenthesized
source declarations retain their true function name.

Optimized default locked build passed53.61s. Immutable artifact SHA256
02aa18e5cf561196294503f5b97c064ae6b07aafc10f51e087f36248e09477ea binds127
production/toolchain files to committed sourceb9b0ab5. The serialized campaign is
TERMINAL PASS all10 tasks; no continuing build/index/timing lease. Optimized
permanent container CLI/MCP selector passed29.51s, including14 qualified fixtures,
exact canonical targets and called-by summaries, refresh/full/identity/no-op.

All16 native baselines (stable0.5.0/contracts5–19) pass unchanged-source repair,
full convergence, all-approved-purpose/project-identity preservation, no-op,
early deadline/publication preservation, compatible rollback and re-upgrade.
Resource19→20 repair/full/normal-call/no-op also passes. Earlier DER20 fingerprint
and after-staging cancellation unit passed; the required native suite reruns it.
Prior19 semantic digest seeded in the repair unit is
`d662ae01fce4a173b5b0e520a7cf0465d7b7184528b914afa24b9145c3efe8ed`.

All72 initialization samples (36 baseline/candidate pairs over12 fixtures) and
fresh exact Cargo-root/baseline-anchor/all63 host caller probes pass. Current
compact cost docs bind current source/artifact; historical14 allocation evidence
is labelled separately. C++ qualified bodies cost0.1710→0.1738s median versus19,
whose bindings are incorrect. Forwarding cost3.6517→2.0353s versus16, roughly
unchanged max RSS. No general speedup or issue708 SLO/issue715 acceptance waiver.

Next: independent current whole-issue/proof review; commit docs/tasks, pass strict
OpenSpec and exact scoped IssueOps, and normally push the clean candidate with
mandatory native gates. Verify published head, current hosted OS/Rust/repository/
IssueOps/CodeQL checks and fresh feedback. Complete independent five acceptance
areas only when implementation and required gates are complete. Merge normally
and verify #731 and #737 CLOSED before #741. Final integration derivation must
exceed20 while retaining SEM20. No bypass, force push, reset or stable promotion.

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
