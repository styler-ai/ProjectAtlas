## 1. Contract And Issue Alignment

- [x] 1.1 Map `stabilize-windows-runtime-output-bound-proof` to GitHub issue #525, synchronize the exact issue/OpenSpec packet and `complexity:medium` classification, and keep milestone, hierarchy, dependency, and `status:ready` mutations deferred until the packet is published on `main`.
- [x] 1.2 Reconcile the issue and design with the existing CLI E2E contract-ownership diagram and retain that durable view unchanged only if its Windows delivery-test and shared-process ownership remains accurate.

## 2. Causal Output-Bound Proof

- [x] 2.1 Add one optional private test-observation sink at the existing Windows bounded-runtime-probe helper so output-limit and timeout decisions are distinguishable while every production caller retains the same nullable payload, five-second timeout, one-MiB ceiling, strict validation, and exact cleanup.
- [x] 2.2 Replace the pre-launch four-second proxy with causal owned fixtures for delayed startup plus finite over-ceiling output and for true timeout; prove explicit disposition, rejected payload, exact process-tree reaping, probe-file cleanup, and normal valid-runtime compatibility without retry, serialization, global locks, or resource-bound changes.

## 3. Verification And Delivery

- [ ] 3.1 Run PowerShell syntax/static checks, the focused Windows installer E2E, repeated ordinary parallel locked workspace proof with explicit timeouts, formatting/diff checks, and all affected Rust/workspace gates; preserve bounded CPU, memory, output, process, and wall-time behavior.
- [ ] 3.2 Refresh onto the accepted shared E2E baseline, pass strict OpenSpec and live IssueOps, resolve every exact-head review finding, pass hosted Windows and required cross-platform checks, and reconcile the final source, test owner, issue tasks, architecture link, and #492 release graph.
