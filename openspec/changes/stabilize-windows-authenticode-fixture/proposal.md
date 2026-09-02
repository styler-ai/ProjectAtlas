## Why

The required Windows E2E gate currently gives different Authenticode-fixture results under PowerShell, `cmd.exe`, and the full parallel suite. That shell-sensitive test blocks unrelated accepted changes and cannot serve as release evidence until its child PowerShell environment and failures are deterministic.

## What Changes

- Run the existing Authenticode trust fixture with controlled command/module inputs that do not depend on the parent shell.
- Preserve the production module-qualified trusted cmdlet route and all signer, status, type, digest, unsigned-image, and malicious-shadow checks.
- Replace the combined assertion with causal invariant diagnostics.
- Prove the same behavior from PowerShell, `cmd.exe`, the serialized installer subgroup, and the normal parallel E2E/workspace gate without retries or global serialization.

Non-goals are changing product installer trust or obsolete-MCP handoff behavior, adding a general PowerShell harness, retrying the test, or serializing the suite. The change is ready for implementation for issue #547 and the `v0.5.0-00` release.

## Capabilities

### New Capabilities

- `windows-authenticode-fixture-hermeticity`: Deterministic installer trust-boundary proof across supported Windows shell and process environments.

### Modified Capabilities

- None.

## Impact

The expected implementation is confined to the existing Windows CLI E2E fixture in `crates/projectatlas-cli/tests/e2e.rs`. Product installer PowerShell, Rust runtime behavior, persistence, public CLI/MCP contracts, dependencies, and non-Windows behavior remain unchanged unless causal evidence proves a separate product defect.
