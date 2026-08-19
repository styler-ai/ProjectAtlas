## 1. Contract And Issue Alignment

- [x] 1.1 Map issue #469 to this change, synchronize its metadata and objective checklist, and keep RC2 scope limited to supported Linux placement proof without weakening the SQLite operating profile.
- [x] 1.2 Review the database-authority, WAL, and lifecycle architecture views against the fix; update and render Mermaid only if durable ownership or flow changes, otherwise record the reasoned N/A.

## 2. Linux Placement Resolution

- [x] 2.1 Keep `whichdisk::resolve` as the exact fast path and add a Linux-only dependency-inventory fallback for the specific no-device-mount result.
- [x] 2.2 Canonicalize the probe and select the unique longest component-boundary mount ancestor, rejecting missing or equally specific conflicting candidates as typed uncertainty.
- [x] 2.3 Feed fallback fields through the existing local/network/unknown filesystem classification and immediate canonical probe, mount, device, and type revalidation before every SQLite open.

## 3. Database And Adapter Regression Coverage

- [x] 3.1 Add deterministic resolver tests for device mismatch, ordinary exact resolution, nested mounts, similar string prefixes, conflicting candidates, missing inventory, supported Btrfs, network, unknown, and multibyte paths.
- [x] 3.2 Add real SQLite preflight/open tests for a missing database parent, writable WAL initialization, read-only reopen, path/mount swap refusal, rollback/error propagation, and no filesystem mutation during inspection.
- [x] 3.3 Prove shared CLI/MCP behavior for explicit project roots, initialized Btrfs placement, wrong root, missing index, no implicit mutation, cancellation, and persistent server reuse.
- [x] 3.4 Add a mandatory hosted Linux Btrfs initialization/open/reopen smoke and retain Windows/macOS filesystem-profile coverage.

## 4. Verification And Release Proof

- [x] 4.1 Run `cargo test -p projectatlas-db`, the owning CLI/MCP E2E filters, `cargo fmt --check`, `cargo check --workspace --all-targets --all-features`, warnings-denied workspace Clippy, full workspace/all-feature and doc tests, and warnings-denied docs with explicit timeouts.
- [x] 4.2 Inspect intended-scale lookup complexity, allocation/I/O, transaction/WAL ownership, migration/recovery, and query behavior; run strict OpenSpec, IssueOps parity, ProjectAtlas lint, and all live review/automated-feedback disposition.
- [ ] 4.3 Verify the exact packaged RC2 on a real Btrfs host plus Linux/Windows packaged agent workflows and independently read back release identity/assets while preserving `v0.4.4` Latest.
- [x] 4.4 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.
