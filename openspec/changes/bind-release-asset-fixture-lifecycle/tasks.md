## 1. Contract And Issue Alignment

- [x] 1.1 Map `bind-release-asset-fixture-lifecycle` to GitHub issue #533, synchronize the exact issue/OpenSpec packet and `complexity:medium` classification, and keep milestone, hierarchy, dependency, and `status:ready` mutations deferred until the packet is published on `main`.
- [x] 1.2 Reconcile the issue and design with the existing CLI E2E contract-ownership diagram and retain that durable view unchanged because this remains in its delivery-test and shared-process ownership boundary.

## 2. Causal Fixture Lifetime

- [ ] 2.1 Replace the release-asset server's independent pre-request deadline with the smallest causal lifetime tied to the owned installer operation and one explicit overall bound, preserving exact request and payload validation.
- [ ] 2.2 Cover delayed startup, archive/checksum success, invalid or missing request, installer failure, bounded thread/process cleanup, and unchanged installer behavior without retry, serialization, new dependencies, or timeout inflation.

## 3. Verification And Delivery

- [ ] 3.1 Run the focused Windows fixture, ordinary parallel locked workspace proof, formatting/diff checks, strict OpenSpec/IssueOps, exact-head review, hosted Windows and required cross-platform checks, and reconcile the final task, diagram, and release-graph state.
