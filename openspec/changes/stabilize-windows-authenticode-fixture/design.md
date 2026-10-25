## Context

The existing Windows E2E test extracts the installer identity functions into a child PowerShell script. Its result currently varies with the parent shell or inherited PowerShell state, and one combined assertion hides which trust invariant failed. The production installer path is already module-qualified and must not change for a test-harness defect.

## Goals / Non-Goals

**Goals:**

- Make the existing child fixture own every shell-sensitive input it needs.
- Preserve and diagnose every current Authenticode and stable-image fail-closed check.
- Prove the same fixture in focused, serialized, normal parallel, and hosted Windows execution.

**Non-Goals:**

- Changing installer trust policy or obsolete-MCP handoff behavior.
- Adding retries, suite-wide serialization, a general PowerShell harness, a dependency, or a product configuration knob.

## Decisions

1. Keep the fix in the existing Windows E2E owner unless causal evidence proves a product defect. This is the smallest boundary and prevents test setup from changing production security behavior.
2. Establish required child PowerShell command/module inputs explicitly before exercising the extracted production functions. Parent-shell inheritance is not valid test input.
3. Assert lookup, shadow, signature, and digest outcomes separately. Causal errors make a recurrence actionable without adding a new evidence framework.
4. Retain the existing architecture view. The trusted Codex-parent and obsolete-MCP handoff ownership is unchanged; the implementation and review task must reconfirm that no diagram update is needed.

## Risks / Trade-offs

- Controlled inputs could bypass the production lookup path. -> Keep malicious local shadows active and require the existing module-qualified route to reach the controlled trusted cmdlet.
- Focused execution could hide host-wide interference. -> Require both shell entry points, the serialized subgroup, the normal parallel suite, and hosted Windows proof.
- A product defect could be mistaken for fixture drift. -> Stop and respecify before changing installer code if causal diagnostics point outside the test owner.

## Migration Plan

No product migration is required. Replace the fixture behavior, run the required Windows proof, and revert the test-only commit if it does not remain hermetic.

## Dependencies / Cross-Issue Impact

The former #544 prerequisite is satisfied on `main`; no active direct blocker remains for #547. It is one direct child and blocker of release owner #492 and a genuine test-gate prerequisite for publishing the already accepted local heads for #477 and #484 and for completing #518 because their mandatory Windows gate reproduces this fixture failure. The issues refresh onto `main` after #547 lands; no product, schema, crate, or public compatibility dependency is introduced.

## Open Questions

None.
