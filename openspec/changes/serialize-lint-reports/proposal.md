## Why

`projectatlas lint` bypasses the shared output serializer, ignores the documented global format, and writes its entire response to stderr. The same lint work is also composed independently for CLI and MCP as pre-rendered strings, leaving no shared machine-readable contract.

## What Changes

- Return one typed lint report from the shared lint owner for both CLI and MCP callers.
- Route CLI lint reports through the existing TOON/JSON serializer on stdout while preserving the lint exit code.
- Keep stderr for execution errors and diagnostics rather than successful or failing lint payloads.
- Prove JSON, TOON, clean, failing, CLI, and MCP behavior through the real adapters.

## Capabilities

### New Capabilities

- `lint-report-serialization`: Expose one deterministic typed lint result through CLI TOON/JSON output and MCP.

### Modified Capabilities

None.

## Impact

The change affects lint report construction in `projectatlas-cli`, the CLI output adapter, the MCP lint tool, and their contract tests. It adds no dependency, database schema, filesystem policy, global-argument parsing change, or platform-specific branch.

## Non-Goals

- Making `--format` valid after subcommands; that remains uniform Clap behavior.
- Changing lint rules, purpose strictness, untracked-file classification, or exit-code semantics.
- Adding a second serializer or a lint-specific transport framework.

This change is ready for implementation in `v0.4.5-rc3`.
