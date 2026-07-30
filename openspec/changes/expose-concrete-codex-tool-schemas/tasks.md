## 1. Reproduction and Contract

- [x] 1.1 Reproduce the v0.4.0 raw-stdio local-reference schema and identify the Codex bridge compatibility boundary.
- [x] 1.2 Audit all advertised MCP inputs for local `$defs` and `$ref` use and record the complete affected set.

## 2. Concrete MCP Schemas

- [x] 2.1 Inline the affected schemas at their nested typed-schema boundary without changing Serde or runtime request types.
- [x] 2.2 Add packaged raw-stdio coverage that rejects any advertised local input reference and verifies concrete purpose-review required and optional fields.
- [x] 2.3 Preserve and verify runtime rejection, conditional-write admission, wrong-root, missing-index, and no-implicit-mutation behavior.

## 3. Verification

- [x] 3.1 Run focused MCP schema and purpose-review tests, including the missing-`path` negative case and compatibility inventory.
- [x] 3.2 Run formatting, workspace check, Clippy, rustdoc, OpenSpec, issue-map/checklist diagnostics, and diff-quality gates proportionate to the change.
- [x] 3.3 Synchronize the GitHub checklist and obtain final integrated, packaged-Codex/hosted, and live review proof.
