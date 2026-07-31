## Context

`normal_reads_do_not_serve_offline_stale_index_state` sent six MCP requests through a batch helper that closed stdin immediately. RMCP 1.8.0 treats EOF as transport closure and gives unfinished handlers a fixed five-second drain; gate load can therefore make a varying accepted response miss that incidental grace period. The all-tools inventory and advertised-tool helpers used the same close-immediately lifecycle, and the exact v0.4.2 Windows release package job later reported `MCP response 2 is missing` after the persistent #409 test passed. The test module already has `McpContractSession`, which keeps one child and stdin owner alive while waiting for each response.

## Goals / Non-Goals

**Goals:**

- Observe every accepted stale-read response before closing the test transport.
- Observe the all-tools inventory and every advertised-tool contract response before closing their transports.
- Preserve one server, session, and project binding across all six adapter checks.
- Retain exact fail-closed assertions and bounded explicit shutdown on success and failure.

**Non-Goals:**

- Change production MCP, RMCP, freshness, cancellation, or storage behavior.
- Add a concurrent-read requirement to a test that owns adapter parity.
- Change the generic EOF-bounded batch helper used by compatibility tests that intentionally submit a complete input batch.
- Add another session abstraction or change the existing one-process-per-contract isolation.

## Decisions

Reuse `McpContractSession` and issue the six stale-read tool calls sequentially. Route the all-tools inventory and each advertised-tool contract call through the same concrete session owner, retaining their existing one-process-per-contract isolation while completing initialization and the required request before closing stdin. This preserves each owned release contract while eliminating the accidental dependency on concurrent completion after EOF.

Capture the fallible request/assertion loop result, invoke the existing bounded `shutdown` through one test-harness completion function, then return the primary request/assertion error before any shutdown error. `Drop` remains the non-reporting last-resort cleanup path. A focused fault check makes the cleanup attempt and error precedence executable without changing the session abstraction.

Do not change the production server, dependency timeout, or generic batch helper. The failure occurs after these release clients close their transports, and increasing a grace period would only move the race. Correct both release-contract owners so the stale-read and all-tools gates cannot leave accepted work behind.

## Risks / Trade-offs

- [A tool check is omitted during conversion] → Keep the existing tool names, arguments, and shared assertion predicate in one explicit table.
- [The session leaks after a request or assertion failure] → Attempt the existing bounded explicit shutdown after the loop result on every path; retain `Drop` as fallback and fault-test the attempt.
- [A cleanup error hides the primary failure] → Return the captured primary error before the shutdown result and fault-test simultaneous errors.
- [The all-tools table no longer matches the advertised inventory] → Retain the exact inventory equality and schema-digest assertions after moving only its session lifecycle.
- [A tool response is still lost after EOF] → Route both the inventory and the shared advertised-tool call owner through the persistent session and repeat the complete all-tools contract.
- [Generic compatibility batches change behavior] → Leave their existing EOF-bounded helper unchanged and run the complete E2E target.
- [A production defect is hidden] → Preserve all typed payload and stale-content assertions; change only when stdin closes.
