## Context

`windows_installer_fresh_path_probe_respects_machine_precedence` currently calls the private PowerShell runtime validator and infers that the one-MiB live output limit fired when the whole call returns in less than four seconds. The owned `.cmd` fixture first launches another PowerShell process, so hosted-runner startup delay can consume that assertion budget before output begins. The production helper already has separate output-limit and five-second-timeout branches, but its nullable payload result does not expose which rejection branch the test exercised.

## Goals / Non-Goals

**Goals:**

- Make the Windows E2E assert the actual output-limit disposition instead of an arbitrary pre-launch wall-clock proxy.
- Preserve the same five-second timeout, one-MiB ceiling, exact owned-process stop/reap behavior, cleanup, and nullable production result.
- Causally cover output-limit and true-timeout branches under ordinary parallel execution.

**Non-Goals:**

- New process infrastructure, dependencies, retries, suite serialization, or larger resource bounds.
- Installer, PATH, MCP, database, CLI, or public payload behavior changes.
- Cross-platform abstraction for one Windows-only fixture.

## Decisions

### Expose one optional private probe disposition for tests

Extend the existing private `Invoke-ProjectAtlasBoundedJsonCommand` helper with one optional test-observation sink, omitted by every production caller. Set it immediately at the existing output-limit or timeout decision before the same owned-process stop path runs. The normal nullable payload and all production calls remain unchanged.

This is smaller and more causal than adding scheduler slack, inferring the branch from total elapsed time, preserving random temporary probe files, or introducing a process-test framework.

### Make the fixture finite and branch-specific

Use the existing owned Windows fixture to emit more than the existing output ceiling and then remain alive. Assert the explicit output-limit disposition, rejected payload, exact child cleanup, and probe-file cleanup. Keep a separate non-flooding owned process for the true timeout disposition. A test-owned startup delay may reproduce contention, but it is not itself the success criterion.

### Preserve the existing ownership boundary

The test remains in the current Windows delivery E2E owner and follows #487's accepted move when that branch lands. No product module, shared test framework, workflow, schema, or architecture diagram changes.

### Keep the aggregate watchdog by deleting duplicate work

The hosted gate must retain its 30-second outer watchdog. Remove the older full-timeout rejection probe now superseded by the causal timeout-disposition fixture, and collapse duplicate valid-runtime invocations where one result can prove both payload compatibility and the optional observation seam. This preserves the direct output-limit and timeout branches while reducing nested process startup work instead of inflating time limits or adding retries.

## Risks / Trade-offs

- [Risk] A test observation changes production control flow. -> Make it optional, assign only at existing branch points, and prove identical production-facing results with the sink omitted.
- [Risk] The fixture still passes without exceeding the ceiling. -> Emit a finite payload strictly larger than the existing bound before blocking and assert the output-limit disposition.
- [Risk] The fix races ongoing E2E ownership changes. -> Start implementation only from the accepted shared baseline and preserve the final single #487 owner.
- [Risk] A disposition string becomes a new public contract. -> Keep it private to the installer script/test boundary and do not serialize or document it as product output.
- [Risk] Redundant nested process launches make the aggregate Windows proof fail under ordinary hosted startup contention. -> Keep one causal probe per distinct behavior and retain the existing 30-second outer watchdog.

## Migration Plan

No data or schema migration. Land the private observation and causal Windows fixtures together; rollback is a direct revert of that script/test change.

## Dependencies / Cross-Issue Impact

#523 is a genuine prerequisite because this proof uses the corrected generic installer observer's outer deadline. If #487 lands first, refresh onto its accepted test-ownership move; #518 and #523 remain separate E2E boundaries.

## Open Questions

None.
