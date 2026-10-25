## ADDED Requirements

### Requirement: Compiler configuration accepts an optional UTF-8 BOM

ProjectAtlas SHALL decode `tsconfig.json` and `jsconfig.json` equivalently when valid UTF-8 JSONC begins either at byte zero or immediately after the exact `EF BB BF` prefix.

#### Scenario: TypeScript configuration begins with a BOM

- **WHEN** a valid `tsconfig.json` begins with exact UTF-8 BOM bytes
- **THEN** `init`, `scan`, `watch --once`, and MCP refresh load the same compiler configuration as the equivalent non-BOM file

#### Scenario: JavaScript configuration begins with a BOM

- **WHEN** a valid `jsconfig.json` begins with exact UTF-8 BOM bytes
- **THEN** the shared loader produces the same configured module resolution as the equivalent non-BOM file

### Requirement: BOM handling preserves strict failure and work controls

ProjectAtlas MUST strip only one exact leading UTF-8 BOM before decoding and parsing while retaining complete-file byte accounting, source-currentness checks, deadline/cancellation behavior, and typed malformed or non-UTF-8 failures.

#### Scenario: JSONC is malformed after a BOM

- **WHEN** an applicable compiler configuration has an exact leading UTF-8 BOM followed by malformed JSONC
- **THEN** refresh returns the existing typed compiler-configuration parse failure without publishing a partial generation

#### Scenario: Input is not valid UTF-8

- **WHEN** a compiler configuration contains non-UTF-8 bytes with or without a leading UTF-8 BOM
- **THEN** refresh returns the existing typed non-UTF-8 failure

#### Scenario: Prefix is partial or misplaced

- **WHEN** BOM-like bytes are incomplete or do not begin at byte zero
- **THEN** they are not stripped and the existing strict decoder or parser behavior is preserved

### Requirement: Refresh adapters share BOM behavior

CLI and MCP refresh MUST use the same compiler-configuration loader and preserve equivalent generation-bound graph publication for BOM and non-BOM input.

#### Scenario: Configuration changes between encoding forms

- **WHEN** a CLI or MCP refresh observes an otherwise equivalent compiler configuration changed between BOM and non-BOM UTF-8
- **THEN** refresh completes through the existing full or incremental publication path without changing alias-resolution semantics
