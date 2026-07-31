## Context

`normal_reads_do_not_serve_offline_stale_index_state` currently sends six MCP requests through a batch helper that closes stdin immediately. RMCP 1.8.0 treats EOF as transport closure and gives unfinished handlers a fixed five-second drain; the freshness owner serializes same-project reconciliation, so gate load can make a varying accepted response miss that incidental grace period. The test already has access to `McpContractSession`, which keeps one child and stdin owner alive while waiting for each response.

## Goals / Non-Goals

**Goals:**

- Observe every accepted stale-read response before closing the test transport.
- Preserve one server, session, and project binding across all six adapter checks.
- Retain exact fail-closed assertions and bounded explicit shutdown on success and failure.

**Non-Goals:**

- Change production MCP, RMCP, freshness, cancellation, or storage behavior.
- Add a concurrent-read requirement to a test that owns adapter parity.
- Add another session abstraction or one child process per tool.

## Decisions

Reuse `McpContractSession` and issue the six tool calls sequentially. Sequential calls directly cover the owned contract—each normal-read adapter returns the current typed stale state—while eliminating the accidental dependency on concurrent completion after EOF.

Capture the fallible request/assertion loop result, invoke the existing bounded `shutdown` through one test-harness completion function, then return the primary request/assertion error before any shutdown error. `Drop` remains the non-reporting last-resort cleanup path. A focused fault check makes the cleanup attempt and error precedence executable without changing the session abstraction.

Do not change the production server, dependency timeout, or generic batch helper. The failure occurs after this test client closes its transport, and increasing a grace period would only move the race.

## Risks / Trade-offs

- [A tool check is omitted during conversion] → Keep the existing tool names, arguments, and shared assertion predicate in one explicit table.
- [The session leaks after a request or assertion failure] → Attempt the existing bounded explicit shutdown after the loop result on every path; retain `Drop` as fallback and fault-test the attempt.
- [A cleanup error hides the primary failure] → Return the captured primary error before the shutdown result and fault-test simultaneous errors.
- [Sequential execution stops covering concurrency] → Concurrency was incidental and is not part of the stale-read adapter contract.
- [A production defect is hidden] → Preserve all typed payload and stale-content assertions; change only when stdin closes.
