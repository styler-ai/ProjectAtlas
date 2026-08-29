## Context

The Windows installer E2E compiles a small Codex-owner fixture, starts an obsolete ProjectAtlas MCP child, and waits for the owner to atomically publish the child's PID, start time, and executable path. `spawn_codex_owned_obsolete_mcp` currently gives that publication five seconds. During the required parallel workspace gate, the owner/child pair exceeded that startup window and the gate failed after 145 other E2E tests passed; the exact test then passed in isolation. The product installer and MCP runtime were not the failing boundary.

The repair must keep the test fail-closed. A larger wait is acceptable only when it remains named and bounded, preserves parent early-exit and exact identity validation, produces actionable failure diagnostics, and has deterministic proof beyond the former ceiling. #487 moves this owner from the monolithic E2E file into the delivery suite, so the accepted fix must land before that branch is refreshed.

## Goals / Non-Goals

**Goals:**

- Make valid Codex-owner child identity publication reliable under the supported parallel Windows workspace load.
- Keep one named, bounded readiness budget and the existing 25 ms polling cadence.
- Preserve atomic publication, exact PID/start-time/executable-path validation, early parent-exit detection, and owned-process cleanup.
- Prove delayed success beyond five seconds and real bounded failure deterministically.
- Preserve one final helper owner when #487 refreshes onto the accepted fix.

**Non-Goals:**

- Change production installer, MCP, process-retirement, or timeout behavior.
- Retry a failed release gate until it happens to pass.
- Serialize unrelated tests or the complete workspace suite.
- Add a dependency, generic fixture framework, product setting, schema, or cross-platform abstraction.

## Dependencies / Cross-Issue Impact

#518 has no implementation prerequisite and is a direct child and blocker of release owner #492. It must land before #476 is republished and before #487 is accepted because both required Windows gates exercise this fixture owner. The change introduces no product, schema, crate, package, or public compatibility dependency; after it is accepted on `main`, #476 and #487 refresh onto that baseline and rerun their affected proof.

## Decisions

### Use one 30-second test-fixture readiness budget

The owning helper will replace the inline five-second expression with a named 30-second readiness constant. Thirty seconds is bounded, remains small beside the owning installer E2E and workflow limits, and gives process creation enough headroom under parallel Windows CI contention without converting a hung fixture into an unbounded wait.

Alternatives rejected:

- Retrying the test or push would make acceptance nondeterministic.
- A workspace-global lock would hide contention and serialize unrelated tests.
- Keeping five seconds and adding sleeps before the broad suite would move the race rather than fix its owner.

### Inject publication delay only into the fixture child process

The compiled owner fixture will accept an optional test-only delay through its own child-process environment. The Rust helper will set that value on the spawned fixture command, never through process-global environment mutation. Existing production-like callers use no delay; a focused regression requests a delay longer than five seconds but shorter than the 30-second budget.

This is the smallest causal seam: it exercises the real executable, child start, atomic identity file, polling loop, identity parser, and cleanup. A mocked clock/file-loop abstraction would add more code while proving less of the Windows process boundary.

### Preserve failure classification before checking readiness

Each poll continues to inspect whether the owner exited before accepting an identity file. A present identity file is parsed through the existing exact PID/start-time/executable-path validation. Timeout diagnostics include the elapsed budget and relevant owner, identity-file, and expected-runtime facts, while cleanup stays in the existing owned-process error path.

The delay seam must not relax malformed/mismatched identity rejection or allow unrelated-process termination. Existing negative paths are reused where they already prove those contracts; only missing causal coverage is added.

### Land before refreshing dependent delivery branches

#518 lands on `main` first. #476 and #487 then refresh onto that exact accepted head; #487 preserves one delivery-suite helper and updates its frozen source/support inventory only as required by the real change. This avoids implementing the same repair in two divergent test layouts.

## Risks / Trade-offs

- [Risk] A true fixture hang takes longer to fail. -> Keep the 30-second constant test-only, preserve early owner-exit detection, and assert a real timeout remains bounded and diagnostic.
- [Risk] The delay seam leaks into unrelated tests. -> Set it only on the spawned fixture `Command`; never mutate process-global environment.
- [Risk] #487 loses or duplicates the repair during its split refresh. -> Make #518 a native blocker of #487 and verify one final helper owner after rebase.
- [Risk] Identity validation is weakened while changing startup timing. -> Reuse the existing identity reader and negative tests without repair, fallback, or guessed process matching.

## Migration Plan

1. Land the accepted test-only helper, deterministic delayed-publication proof, diagram, and gates on `main` through #518.
2. Refresh #476 and #487 onto the accepted #518 head; preserve one final helper owner and rerun their affected proof.
3. Roll back #518 as one test-only commit series if the causal regression or broad workspace gate fails; no product or persistent-data migration exists.

## Open Questions

None.
