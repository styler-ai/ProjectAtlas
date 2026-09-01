## Context

`serve_release_assets` starts a nonblocking loop with a fixed one-minute deadline, then its callers launch an installer and join the server thread. The deadline includes all scheduler and installer startup work before the archive and checksum requests. A full parallel workspace run exhausted that lifetime, while the exact focused test passed in 68 seconds. The fixture therefore has two unrelated clocks and can fail before the owned operation proves whether requests were missing.

## Goals / Non-Goals

**Goals:**

- Make the server lifetime follow the owned installer operation and one bounded completion contract.
- Retain exact archive/checksum path and payload validation and deterministic thread/process cleanup.
- Causally cover success, delayed startup, installer failure, and missing or invalid requests.

**Non-Goals:**

- New process infrastructure, dependencies, retries, suite serialization, or larger timeouts.
- Product installer, runtime, PATH, MCP, database, CLI, or payload changes.
- A generic local HTTP server abstraction.

## Decisions

### Reuse the existing owned-operation boundary

Keep the current local listener and exact two-request validation. Give the server the smallest owner signal needed to distinguish an installer that is still running from one that has completed without satisfying the request contract. The owner applies one explicit overall bound and always joins the server before returning.

This removes the independent pre-request deadline instead of increasing it or adding a retry.

### Preserve original failure and cleanup truth

When installer execution fails, stops, or completes without both valid requests, stop the server causally and report both the initiating failure and any server/request-validation failure without leaving a thread or process behind. Successful callers still require both exact requests.

### Keep the existing ownership boundary

The change remains in the current Windows delivery E2E owner and follows #487's accepted move if that structural branch lands first. No product module, shared framework, workflow, schema, or architecture diagram change is expected.

## Risks / Trade-offs

- [Risk] The server waits forever after installer completion. -> Use the existing owner completion path and one explicit overall operation bound, then always join.
- [Risk] Cleanup hides the initiating installer failure. -> Retain both failures in the test diagnostic instead of replacing the first error.
- [Risk] A partial request sequence is accepted. -> Keep the existing exact archive-plus-checksum completion condition unchanged.

## Migration Plan

No data or schema migration. Land the causal fixture lifetime and its focused failure coverage together; rollback is a direct revert of the test-only change.

## Dependencies / Cross-Issue Impact

The source currently shares `e2e.rs` with #518 and #525, so implementation must start from their accepted merged baseline or the final accepted #487 test-owner move. This is source ownership sequencing, not a product prerequisite.

## Open Questions

None.
