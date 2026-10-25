## Context

The existing installer observer owns a Windows job or Unix process group, drains stdout and stderr concurrently, and retires descendants after observing parent exit. Its lifecycle test currently launches two PowerShell interpreters before a finite pipe-holding descendant. Two hosted Windows runs failed before observing the intended successful parent exit; local runs pass. A standalone local probe establishes parent success in 0.391 seconds with output retained until 15.578 seconds, but does not reproduce the hosted cause.

## Goals / Non-Goals

**Goals:** Establish the intended completed-parent state causally and prove prompt owned-descendant cleanup, successful status preservation, and strict late-observation behavior through the existing lifecycle test.

**Non-Goals:** Product, database, protocol, dependency, CI routing, timeout-policy, or process-framework changes; retry-only acceptance; new serialization or global locks.

## Decisions

- Keep the existing `PluginInstallerProcess`, job/process-group ownership, concurrent output readers, and observer. The defect is isolated at the fixture boundary; a replacement process framework would duplicate proven ownership and failure handling.
- Replace the two PowerShell launches with `cmd.exe /D /C` running `cmd.exe /D /C start /B ping.exe -n 16 127.0.0.1`. The unquoted executable needs no empty `start` title or nested quote escaping. Independent probes establish prompt parent success and approximately 15 seconds of inherited-pipe retention, including with `CREATE_NO_WINDOW`. The hosted logs establish failure to reach the intended parent-exited state, but do not identify the operating-system scheduling cause; hosted verification remains required.
- Add one no-cleanup control through the existing injected termination closure. Require successful parent exit at that checkpoint and measure subsequent pipe drainage against the existing ten-second cleanup bound. Then execute the same fixture with real job/process-group cleanup and require prompt completion. This directly distinguishes an absent or prematurely exited pipe holder from effective cleanup, without marker files, reader-scheduling assumptions, or another process API.
- Exercise both ordinary successful completion and deliberately late observation with the same fixture. Reuse the existing bounded exit synchronization when separating fixture readiness from observation is necessary. Keep cleanup failure diagnostics and existing deadline values intact.
- Use a fixed small process tree and bounded output. No persistent state, data migration, pool, new synchronization service, or cross-crate abstraction is required. Retaining nested PowerShell adds interpreter startup without contributing to the state under test; a recursive Rust fixture or native process-query API adds unnecessary machinery when the existing shell and injection boundary suffice.

## Risks / Trade-offs

- A fixture may pass without creating the intended descendant -> establish successful parent exit with a still-retained output pipe before cleanup.
- Shell quoting or interpreter startup may test the wrong state -> inspect native command arguments and isolate startup from descendant retirement.
- A readiness adjustment may weaken timeout semantics -> retain the strict late-observation case and existing live-parent timeout and failure-cleanup cases.
- Local success may conceal the hosted failure -> require the affected hosted Windows gate on the accepted fix, plus existing selected platform compatibility checks.
- The no-cleanup control adds approximately 15 seconds -> retain the finite descendant lifetime and existing outer test deadline; this cost proves the missing causal condition and does not extend an installer deadline.

## Migration Plan

Update only the owning test fixture and any required frozen test-source identity. Run focused causal checks, required local gates, independent review, and affected hosted checks. Merge the shared baseline before refreshing the preserved PHP branch. No runtime or database migration is involved.

## Dependencies / Cross-Issue Impact

#561 belongs to release owner #492 and follows the already completed lifecycle work in #533. It has no open implementation prerequisite. Its shared gate currently prevents #477 acceptance; the PHP source remains separately owned and preserved. #560 owns the separate metadata-event routing defect.

## Open Questions

None.
