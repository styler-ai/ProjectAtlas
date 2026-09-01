## Context

`serve_release_assets` starts a nonblocking loop with a fixed one-minute deadline, then its callers launch an installer and join the server thread. The deadline includes all scheduler and installer startup work before the archive and checksum requests. A full parallel workspace run exhausted that lifetime, while the exact focused test passed in 68 seconds. The fixture therefore has two unrelated clocks and can fail before the owned operation proves whether requests were missing. The helper is shared by four Windows installer tests and `posix_release_binary_installer_rejects_checksum_mismatch`, so its lifecycle change has one cross-platform compatibility caller even though the reported release-gate bug is Windows-specific.

## Goals / Non-Goals

**Goals:**

- Make the server and installer follow one four-minute absolute operation deadline created before either is launched; keep the existing five-minute workflow step as an independent outer kill boundary without claiming a fixed cleanup reserve.
- Retain the current archive/checksum suffix routing, exact payload validation, two-request completion rule, and deterministic thread/process cleanup.
- Causally cover success, delayed startup, installer failure, missing or invalid requests, absolute-deadline cleanup, and the shared POSIX checksum caller.

**Non-Goals:**

- New process infrastructure, dependencies, retries, suite serialization, an independent pre-request timeout, or an unbounded local fallback to the five-minute CI step.
- Product installer, runtime, PATH, MCP, database, CLI, or payload changes.
- A generic local HTTP server abstraction.

## Decisions

### Use one absolute owner deadline

Create `RELEASE_ASSET_INSTALLER_OPERATION_TIMEOUT` as four minutes and compute its checked absolute deadline before binding the listener or spawning the installer. This removes the independent one-minute server clock. The existing five-minute workflow step starts before compilation and harness setup, so it remains an independent outer cap and does not guarantee a fixed cleanup reserve.

Keep the current local listener, suffix-based archive/checksum routing, exact payload validation, and two-request completion rule. Spawn the installer and pass only the absolute deadline's remaining duration to the existing `wait_for_plugin_installer_output` owner, rather than using synchronous unbounded `.output()`. Give the server one `std::sync::mpsc::sync_channel(1)` completion/cancellation signal so it can distinguish an installer that is still able to request assets from one that has completed or failed. The caller captures the installer result, signals owner completion on every terminal path, and always joins the server before returning.

This removes the independent pre-request deadline instead of increasing it or adding a retry. The five-minute workflow step remains an outer kill boundary, not the fixture's lifecycle authority, and may preempt the local deadline when earlier workflow work consumes the step budget.

### Preserve original failure and cleanup truth

When installer spawn, observation, execution, or cleanup fails, or the installer completes without both valid request kinds, signal the server causally and report both the initiating failure and any server/request-validation failure without leaving a thread or process behind. Deadline expiry uses the existing installer observer to terminate and reap the owned process before the server join. Successful callers still require the archive and checksum requests.

### Preserve the shared compatibility caller

Apply the same owner deadline and terminal-path join to all five current `serve_release_assets` callers. Keep the user-facing capability Windows-specific, but require `posix_release_binary_installer_rejects_checksum_mismatch` on Linux and macOS so the common helper cannot regress its POSIX checksum contract.

### Keep the existing ownership boundary

The change remains in the current CLI delivery E2E owner and follows #487's accepted move if that structural branch lands first. No product module, shared framework, workflow, schema, or architecture diagram change is expected.

## Risks / Trade-offs

- [Risk] The server or installer outlives the accepted local operation. -> Start one four-minute absolute deadline before both launches, use the existing installer observer with its remaining budget, then signal and join promptly after owner termination; keep the workflow timeout as an independent outer cap.
- [Risk] Cleanup hides the initiating installer failure. -> Retain both failures in the test diagnostic instead of replacing the first error.
- [Risk] A partial request sequence is accepted. -> Keep the existing archive-plus-checksum two-request completion condition unchanged.
- [Risk] A Windows fix regresses the helper's POSIX checksum caller. -> Run the exact POSIX checksum-mismatch fixture on Linux and macOS alongside the focused lifecycle and Windows installer proofs.

## Migration Plan

No data or schema migration. Land the causal fixture lifetime and its focused failure coverage together; rollback is a direct revert of the test-only change.

## Dependencies / Cross-Issue Impact

After publication, #533 is one direct child and blocker of release owner #492, has no direct blocker, and unlocks only #492. Current main already contains the accepted #518 and #523 shared-process baseline. Implementation must start only after the active shared-file owner resolves and use the latest accepted main; if #487 lands first, follow its accepted move into `e2e_delivery.rs`. Shared-file sequencing with #525 or #487 is operational ownership, not a native dependency edge or product prerequisite.

## Open Questions

None.
