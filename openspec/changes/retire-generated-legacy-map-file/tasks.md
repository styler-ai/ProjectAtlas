## 1. Contract and release graph (#627)

- [x] 1.1 Validate the fileless-map OpenSpec contract and map #627 into the RC3 milestone, task authority, native parent/blocker graph, and release-owner acceptance.

## 2. Preserve map access while retiring the generated file (#627)

- [x] 2.1 Trace the map writer, CLI/MCP callers, output/flag contracts, legacy-purpose import, configuration, and all file consumers; confirm the minimal fileless response boundary.
- [x] 2.2 Return the current map through CLI/MCP TOON or JSON without writing a TOON snapshot at default or configured `map_path`; retain explicit JSON sidecar behavior, compatibility flags, selected-root isolation, read-only legacy import, and explicit response limits.
- [x] 2.3 Add causal CLI/MCP positive, independent format/JSON flags, CI map response versus skipped/forced JSON sidecar, wrong-root/missing-index, oversized, existing-file-preservation, and legacy-import regressions; exercise the relocated candidate on Windows, Linux, macOS x64, and macOS arm64, and wire the same regression to #492's installed-package release proof.
- [x] 2.4 Reconcile docs/skill/config/report semantics, run the owning Rust, OpenSpec, IssueOps, formatting, lint, and platform gates, and resolve independent review findings.
