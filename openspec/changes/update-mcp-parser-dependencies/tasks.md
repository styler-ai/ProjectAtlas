## 1. Compatibility contract

- [x] 1.1 Define the intended dependency versions, unchanged MCP and parser compatibility boundaries, release ownership, non-goals, and required consumer proof in the proposal, design, and specification.

## 2. Dependency update

- [x] 2.1 Refresh the locked rmcp and rmcp-macros resolution to 3.2.0 while retaining the existing manifest requirement and features; pin tree-sitter-language to 0.1.8 and tree-sitter-language-pack to 1.16.1 on accepted main, limiting manifest and lockfile changes to the intended update.
- [x] 2.2 Preserve bounded diagnostics for failed parallel parser construction commands, verify MCP identity and stdio behavior, built-in parser precedence, optional parser loading and typed refusal compatibility using the existing owning tests, and complete the affected local, dependency-policy, and hosted platform checks while reconciling the specification with the delivered behavior.

## Verification

Run `openspec validate update-mcp-parser-dependencies --strict --no-interactive` and the native IssueOps gate. Use the normal affected-proof planner and pre-push hook for required locked Rust, documentation, dependency-policy, and platform checks. Focused compatibility commands include `cargo test --locked -p projectatlas-cli --all-features --test e2e_navigation mcp_tools_list_preserves_frozen_contracts_without_index_state -- --exact`; on macOS, run `cargo test --locked -p projectatlas-cli --all-features --test optional_parser_worker_platform`; on supported Linux and Windows hosts with the required parser-pack archive, run `optional_parser_worker_failure`. Confirm nonzero executed test counts and inspect the existing optional parser-pack hosted routes for affected proof.
