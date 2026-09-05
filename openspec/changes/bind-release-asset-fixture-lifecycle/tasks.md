## 1. Contract And Issue Alignment

- [x] 1.1 Map `bind-release-asset-fixture-lifecycle` to GitHub issue #533, synchronize the exact issue/OpenSpec packet and `complexity:medium` classification, declare #492 as its direct parent and the only issue it directly blocks with no direct blocker of its own, keep shared-file sequencing operational rather than a native dependency edge, and defer milestone, hierarchy, relationship, and `status:ready` mutations until the packet is published on `main`.
- [x] 1.2 Reconcile the issue and design with the existing CLI E2E contract-ownership diagram and retain that durable view unchanged because this remains in its delivery-test and shared-process ownership boundary.

## 2. Causal Fixture Lifetime

- [x] 2.1 Replace the release-asset server's independent pre-request deadline with one checked four-minute absolute operation deadline created before listener and installer launch, pass only its remaining budget to the existing `wait_for_plugin_installer_output`, use one bounded standard-library completion/cancellation signal, always join, and preserve current request routing, exact payload, two-request completion, and dual-error truth on every terminal path.
- [x] 2.2 Add `release_asset_server_lifecycle_is_causal_and_bounded` coverage for delayed live-owner startup, archive/checksum success, invalid, empty, partial, or missing requests, installer spawn/exit/timeout failure, dual diagnostics, absolute-deadline process/thread cleanup, all four Windows callers, and unchanged POSIX checksum-mismatch behavior without retry, a new fixture lock, broader serialization, new dependencies, or an independent timeout.

## 3. Verification And Delivery

- [x] 3.1 Run `release_asset_server_lifecycle_is_causal_and_bounded` and `windows_release_binary_installer_repairs_stale_mirror_without_registering_it`, `windows_release_binary_installer_uses_versioned_runtime_when_stable_mirror_is_locked`, `windows_release_binary_installer_rejects_checksum_mismatch`, and `windows_release_binary_only_rejects_invalid_runtime_without_fallback` on Windows; run `posix_release_binary_installer_rejects_checksum_mismatch` on Linux and macOS; then pass the ordinary parallel locked workspace proof, formatting/diff checks, strict OpenSpec/IssueOps, exact-head review, hosted Windows and required cross-platform checks, and reconcile the final task, unchanged diagram, and #492 release-graph state.
