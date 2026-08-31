## 1. Contract And Issue Alignment

- [x] 1.1 Map `enforce-e2e-process-observer-deadlines` to GitHub issue #523, keep it a direct child and blocker of release owner #492 with no fabricated prerequisite, synchronize the exact issue/OpenSpec packet and `v0.5.0-00` milestone, and pass strict OpenSpec validation.
- [x] 1.2 Reconcile the issue and design against the existing CLI E2E contract-ownership diagram, update that durable view if its shared-process ownership is inaccurate, or retain it unchanged with a reasoned semantic review.

## 2. Deadline Classification And Cleanup

- [ ] 2.1 Make `McpContractSession::shutdown`, `run_mcp_stdio_with_env`, and `wait_for_plugin_installer_output` classify `Instant::now() >= deadline` before accepting completion while preserving exact-child termination/reaping after successful termination or an observed-exit race; on a proven-live termination failure, release owned stdin, preserve the timed-out reason/cause, explicitly detach exact owned child/readers, report incomplete unreaped cleanup, and avoid any unbounded wait, output, status, diagnostics, or in-deadline compatibility regression without a new process framework.
- [ ] 2.2 Add narrow test-only observer-delay and infallible cleanup-capture seams and causal late-observation regressions for all three helpers, including prompt child completion, timeout classification, synchronous exact-resource capture for test-owned cleanup, normal in-time success, and no retries, locks, serialization, background workers, or scheduler slack.

## 3. Verification And Delivery

- [ ] 3.1 Run the owning causal regressions, affected MCP/installer E2E, formatting and diff checks, and the normal parallel locked workspace suite with explicit timeouts; retain failure/status/output compatibility and bounded CPU, memory, process, and wall-time behavior.
- [ ] 3.2 Rebase onto the accepted shared `e2e.rs` baseline after #518, pass strict OpenSpec and live IssueOps, resolve every exact-head review finding, pass hosted checks, and reconcile the final implementation, specification, architecture link, issue tasks, and release graph without weakening #492's holistic release proof.
