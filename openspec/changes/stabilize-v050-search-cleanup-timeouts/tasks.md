## 1. Contract and planning

- [x] 1.1 Specify the three confirmed defects, compatibility and failure boundaries, independent issue ownership and RC4 release acceptance; verify complete proposal, design and scenario artifacts with strict OpenSpec validation.

## 2. Unicode search (#697)

- [x] 2.1 Correct shared literal/fuzzy Unicode case matching and conservative storage candidate admission; prove accented Latin and Cyrillic matches, case-sensitive negatives, exact lines, early stop and accelerated/fallback equivalence with real SQLite regressions.
- [x] 2.2 Pass the issue's affected local and hosted checks and installed CLI/atlas/MCP reproduction; reconcile documentation and specification with the final search behavior.

- [x] 2.3 Reuse the existing prepared-statement cache for repeated persisted-text hydration after repeatable optimized CLI/MCP measurement; prove same-connection reads observe replacement, rollback and deletion while preserving search outputs and bounds.

## 3. Parser profile cleanup (#698)

- [x] 3.1 Consume one explicit temporary-profile cleanup attempt before fallible execution and classify cleanup timeout operationally; prove attempt count, complete timeout/reap/drain bounds, successful disarming and early-error fallback with deterministic regressions.
- [x] 3.2 Pass affected local and hosted checks and Windows installed matching-archive verify/install/enable/update/disable/remove proof, including failure, containment, safe retry and no partial publication; reconcile documentation and specification.

## 4. Indexing timeout contract (#699)

- [x] 4.1 Align MCP descriptions/generated schema, applicable CLI help and agent guidance with cooperative whole-operation phases, 30-minute default/maximum and clamping; prove the pre-parser deadline preserves the last valid index, identity and approved purposes and permits recovery.
- [x] 4.2 Pass affected local and hosted checks and installed schema/help/refusal controls; reconcile documentation and specification without changing timeout or publication policy.
