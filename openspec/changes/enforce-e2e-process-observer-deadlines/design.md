## Context

`McpContractSession::shutdown`, `run_mcp_stdio_with_env`, and `wait_for_plugin_installer_output` each poll a child under an explicit deadline. Their current loops accept `try_wait` completion before testing the deadline, so a descheduled observer can wake after the deadline and report success for a completion it did not establish in time. The helpers already own the necessary child, clock, cleanup, status, and output behavior; this is an ordering defect, not a missing process framework.

The existing [CLI E2E contract ownership split](../../../docs/v050-release-architecture.md#cli-e2e-contract-ownership-split) remains accurate: these are shared test-process helpers consumed by MCP, installer, and release contracts. No architecture diagram changes are required.

## Goals / Non-Goals

**Goals:**

- Make an observation at or after the absolute deadline a timeout before completion can be accepted.
- Preserve child termination/reaping, captured output, exit-status validation, diagnostics, and in-time success. If termination fails and a re-probe proves the exact child is still live, release owned stdin, return the existing timed-out class with the deadline reason and termination/re-probe cause, and explicitly detach the exact child/output readers instead of synchronously waiting or joining. This exceptional unreaped disposition is reported as incomplete cleanup; it is not a successful cleanup path.
- Prove the ordering causally by delaying the observer independently from child completion.

**Non-Goals:**

- New retry, locking, serialization, timeout configuration, dependency, or shared process abstraction.
- Product runtime, CLI/MCP protocol, installer behavior, or database changes.
- Changes to #518's Windows Codex-owner readiness and cleanup helpers.

## Decisions

### Check the deadline before the acceptance poll

Each owning loop will derive one absolute deadline and classify `Instant::now() >= deadline` before a completed status can enter its success path. An expired branch may still use `try_wait` to decide whether termination is necessary, but it must preserve the timeout class and reason. Successful termination or an observed-exit race reaps the exact child and joins owned readers. If termination fails and a re-probe proves the exact child is still live, the branch releases stdin, reports the timeout reason plus the termination/re-probe cause, explicitly detaches the exact child/readers, and returns without an unbounded wait or join; it must not retry or hide the unreaped disposition.

Checking only the elapsed time after `try_wait`, or adding scheduler slack, was rejected because either keeps completion authoritative after the contract expired or merely makes the race less frequent.

### Keep the correction local to the three concrete owners

The helpers have different return values and cleanup/output obligations, so each receives the same small ordering correction at its existing ownership boundary. A generic polling framework or trait would add indirection without reducing the three distinct state transitions.

### Add test-only causal observation delays

Narrow test-only seams will delay the first completion observation while fixtures complete promptly. Regressions will assert timeout classification plus exact child reaping, alongside compatible in-deadline success. Ordinary sleeping children alone were rejected because they do not distinguish child lateness from observer lateness.

## Risks / Trade-offs

- **Exact deadline-edge behavior becomes stricter** → Specify and test `>=` so equality cannot depend on loop ordering.
- **A timed-out child may already be complete** → Probe only for cleanup disposition, never acceptance, then reap it and retain timeout classification.
- **The operating system refuses termination** → Re-probe the exact child; preserve `TimedOut` plus the deadline reason and cause, release stdin, and explicitly detach a proven-live child/readers rather than retrying or blocking on `wait` or reader joins. Report the cleanup as incomplete and unreaped.
- **Test seams could leak into production behavior** → Keep them private to the E2E test module and reuse the normal helper paths.
- **Shared-file conflict with #518** → Start implementation only after the #518 amendment no longer owns a mutable `e2e.rs` boundary; rebase onto accepted `main` before proof.

## Migration Plan

No state migration is required. Land the three local corrections and causal regressions together; rollback is the ordinary commit revert if the exact release proof exposes a compatibility regression.

## Dependencies / Cross-Issue Impact

#518's accepted shared `e2e.rs` baseline is already on `main`. #523 owns only the three named generic observers and their causal regressions. #525's Windows runtime-bound proof consumes the corrected installer observer's outer deadline, so its declarative release-graph entry is blocked by #523. If #487's accepted test split lands afterward, it must refresh onto #523 and retain exactly one final helper owner without weakening these assertions. #523 remains one direct child and blocker of release owner #492; no product, schema, installer, or MCP dependency is introduced.

## Open Questions

None.
