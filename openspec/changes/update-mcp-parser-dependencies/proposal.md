## Why

ProjectAtlas's MCP server and optional parser loader need the maintenance updates already proposed in PR #566 to be accepted with explicit release ownership and compatibility proof before the v0.5.0 candidate.

## What Changes

- Update locked rmcp and rmcp-macros from 3.1.4 to 3.2.0, tree-sitter-language from 0.1.7 to 0.1.8, and tree-sitter-language-pack from 1.15.0 to 1.16.1.
- Preserve dependency features, MCP identity and stdio behavior, built-in parser precedence, and optional parser artifact trust and platform boundaries.
- Verify existing consumers through their owning tests and required affected local and hosted checks.
- Preserve bounded command diagnostics when parallel parser construction fails, so the required archive-backed compatibility proof can identify its failing command without weakening construction isolation.
- Correct the stale accepted-capabilities checksum sidecar to match the unchanged committed manifest so construction can enforce its existing input integrity check.

## Capabilities

### New Capabilities

- `mcp-parser-dependency-compatibility`: Acceptance requirements for the maintained MCP and parser dependencies.

### Modified Capabilities

None; existing product behavior is preserved.

## Impact

Ready for implementation under issue #568 and release owner #492. The dependency update is limited to Cargo.toml and Cargo.lock. The existing contained-construction script and its diagnostics test also retain bounded failure evidence after both supported construction jobs exposed a missing command diagnostic. Existing MCP and parser-worker tests supply compatibility proof; the owning OpenSpec and issue-map retain scope authority.

## Non-Goals

No new MCP transport, parser language, feature flag, dependency framework, parser artifact refresh, supported-platform expansion, unrelated dependency update, or runtime redesign.
