## Why

The hosted Windows lifecycle gate repeatedly times out before its completed-parent fixture reaches the intended state, although the unchanged test passes locally. The proof must distinguish fixture startup from cleanup of a completed parent's pipe-inheriting descendant so a shared release gate tests the intended behavior causally.

## What Changes

- Reproduce the fixture's parent, intermediate child, descendant, and inherited-output lifecycle at the existing delivery-test boundary.
- Use the smallest existing platform mechanism that establishes the intended completed-parent state and proves prompt retirement of the exact owned descendant.
- Preserve successful parent status, strict late-observation classification, inherited-output cleanup, and all existing operation and workflow deadlines.

## Capabilities

### New Capabilities

- `completed-parent-fixture-proof`: causal proof of successful parent exit and cleanup of pipe-inheriting descendants in the installer test harness.

### Modified Capabilities

None.

## Impact

Owning issue #561. Implementation is confined to CLI delivery-test infrastructure and its existing verification contracts. The PHP work in #477 remains separate and preserved. This proposal is ready for bounded investigation; implementation follows the observed process-state cause.

## Non-Goals

No timeout inflation, retry-only acceptance, suite serialization, new process/server framework, product changes, or unrelated CI routing changes.
