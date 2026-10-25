## Context

The original Windows installer fixture repair replaced a five-second publication assumption with one named 30-second readiness deadline. It preserves the compiled Codex owner, obsolete MCP child, atomic PID/start-time/path publication and exact validation in the single delivery-test owner accepted through #487.

The RC3 parallel gate now fails the first positive case combining six-second publication and capture delays: only about 5.17 seconds remain when the delayed capture is observed, and the original 30-second deadline correctly refuses it. The earlier startup/publication/helper-creation costs are not separately measured. A second test propagates an ordinary teardown stop-helper timeout after exact fallback cleanup succeeds; that helper chain intentionally preserves its original failure, as its negative tests require. Neither observation establishes a production installer defect.

The fixture already owns native exact process capture and stop. Use those owners for routine readiness and teardown instead of launching PowerShell, while retaining the deliberate helper-failure paths and their diagnostics. Keep all existing budgets, identity checks and unrelated-process protection.

## Goals / Non-Goals

**Goals:**

- Make valid Codex-owner child identity publication reliable under the supported parallel Windows workspace load.
- Keep one named, bounded readiness budget and the existing 25 ms polling cadence.
- Preserve atomic publication, exact PID/start-time/executable-path validation, early parent-exit detection, and owned-process cleanup.
- Prove delayed success beyond five seconds and real bounded failure deterministically.
- Preserve the single delivery-test owner when #666 refreshes onto the accepted fix.

**Non-Goals:**

- Change production installer, MCP, process-retirement, or timeout behavior.
- Retry a failed release gate until it happens to pass.
- Serialize unrelated tests or the complete workspace suite.
- Add a dependency, generic fixture framework, product setting, schema, or cross-platform abstraction.

## Dependencies / Cross-Issue Impact

#547 is accepted on `main`; #518 remains a direct child and blocker of #492. Historical #487 already established the single delivery-test owner. The current #666 native-upgrade acceptance requires this shared Windows gate repair on accepted main before its worktree and affected proof resume. No product, schema, crate, package, or public compatibility dependency is introduced.

## Decisions

### Use one 30-second test-fixture readiness budget

Keep the existing named 30-second readiness constant and original absolute deadline unchanged. Routine native capture must check that same deadline before and after capture; a late result remains a failure. Retain the current cleanup budgets and polling cadence.

Alternatives rejected:

- Retrying the test or push would make acceptance nondeterministic.
- A workspace-global lock would hide contention and serialize unrelated tests.
- Keeping five seconds and adding sleeps before the broad suite would move the race rather than fix its owner.

### Reuse native exact identity for routine fixture work

Use the existing `windows_native_process::capture_exact` and `stop_exact` owners for ordinary readiness and teardown. Preserve exact PID, creation time and executable path, parent early exit, atomic publication, bounded wait and owned-child reaping. Keep deliberate delays under the same captured deadline without process-global environment mutation. Retain the PowerShell primary/fallback/native-final chain where a test explicitly exercises helper faults, late observations or recovery; successful recovery must not erase the original helper failure. Add no general framework, dependency or production behavior.

The accepted-owner routine teardown is the changed cleanup route. The existing negative-readiness recovery still signals and observes the held owner, validates normal or retained child identity when required, and retains its bounded helper/fallback/final-stop chain. This repair does not reroute that failure mechanism.

### Inject publication delay only into the fixture child process

The compiled owner fixture will accept an optional test-only delay through its own child-process environment. The Rust helper will set that value on the spawned fixture command, never through process-global environment mutation. Existing production-like callers use no delay; a focused regression requests a delay longer than five seconds but shorter than the 30-second budget.

This is the smallest causal seam: it exercises the real executable, child start, atomic identity file, polling loop, identity parser, and cleanup. A mocked clock/file-loop abstraction would add more code while proving less of the Windows process boundary.

### Preserve failure classification before checking readiness

Each poll continues to inspect whether the owner exited before accepting an identity file. A present identity file is parsed through the existing exact PID/start-time/executable-path validation. Timeout diagnostics include the elapsed budget and relevant owner, identity-file, and expected-runtime facts, while cleanup stays in the existing owned-process error path.

The delay seam must not relax malformed/mismatched identity rejection or allow unrelated-process termination. Existing negative paths are reused where they already prove those contracts; only missing causal coverage is added.

### Land before resuming dependent native-upgrade proof

#518 lands on `main` first. #666 then refreshes onto that accepted head and reruns affected native-upgrade proof, preserving both fixture changes and the single helper owner. Refresh frozen source/support inventory only as required by the actual source change.

## Risks / Trade-offs

- [Risk] A true fixture hang takes longer to fail. -> Keep the 30-second constant test-only, preserve early owner-exit detection, and assert a real timeout remains bounded and diagnostic.
- [Risk] The delay seam leaks into unrelated tests. -> Set it only on the spawned fixture `Command`; never mutate process-global environment.
- [Risk] #666 loses or duplicates the repair during refresh. -> Make #518 a native blocker of #666 and verify one final helper owner after integration.
- [Risk] Identity validation is weakened while changing startup timing. -> Reuse the existing identity reader and negative tests without repair, fallback, or guessed process matching.

## Migration Plan

1. Land the accepted test-only helper, deterministic delayed-publication proof, diagram, and gates on `main` through #518.
2. Hand off #666 refresh and affected native proof for execution after #518 merges; retain the historical #487 split's single helper owner.
3. Roll back #518 as one test-only commit series if the causal regression or broad workspace gate fails; no product or persistent-data migration exists.

## Open Questions

None.
