## Context

`notify_watch_refreshes_symbols_after_file_change` starts `projectatlas watch`, sleeps 750 ms, and writes its changed fixture. The production watch path installs `RecommendedWatcher` before its initial refresh, but hosted scheduling can delay the child past the sleep. An early write is then consumed by the initial refresh, leaving `--max-cycles 2` waiting for a native event that already occurred.

The test uses a unique temporary repository and database. Production watcher code and SQLite publication behavior are already covered and are not the defect.

## Goals / Non-Goals

**Goals:**

- Establish watcher readiness from observable state instead of elapsed time.
- Bound readiness and preserve child ownership, cleanup, and useful failure diagnostics.
- Keep the post-readiness native-event and changed-symbol assertions intact.

**Non-Goals:**

- Change production watcher timing, retries, event filtering, or publication.
- Serialize the suite, retry the source write, or add a reusable readiness abstraction.
- Change any schema, query, transaction, dependency, API, or crate boundary.

## Decisions

### Require exact initial publication while the child is live

The test will poll the selected database read-only for exactly the fixture's `src/lib.rs::initial` symbol and verify the spawned watch child has not exited. Production installs the native watcher before publishing that initial scan, so this state is the smallest observable boundary that proves a later write can produce cycle two.

Waiting longer, checking only for the database file, parsing startup stdout, or retrying the write would retain a timing race or weaken the native-event assertion.

### Keep one bounded loop in the owning E2E

The readiness loop will tolerate missing or incomplete database state, retain the last observation, check child liveness, sleep briefly between reads, and stop at the existing 15-second style of hard deadline. Early exit and deadline paths will reap the child and report stdout, stderr, and the last database observation.

A helper, trait, production readiness protocol, or suite-wide synchronization mechanism has no second consumer and would add ownership without improving this test.

## Risks / Trade-offs

- A broad database condition could accept stale state -> use the test's unique database plus exact path and symbol identity.
- Polling could hide early child exit -> check `try_wait` every cycle and return an explicit diagnostic.
- A readiness timeout could leak the child -> kill when still live and always wait for collected output.
- Read polling could contend with initial publication -> use read-only opens, tolerate transient failure, and keep a bounded 200 ms interval.
- The fix could stop testing native notification -> preserve the write-after-readiness, clean cycle-two exit, notify-mode output, and changed-symbol checks.

## Migration Plan

Land the E2E-only change with repeated focused and accumulated release proof. Rollback is the single test diff; no production or stored state requires migration.

## Open Questions

None.
