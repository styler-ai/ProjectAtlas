## ADDED Requirements

### Requirement: Readiness is independent from mutation policy
The Windows installer SHALL treat Codex plugin and MCP registry skip flags only as mutation controls. When Codex is available, it SHALL observe the installed plugin manifest/skill and global MCP registration before reporting complete convergence or retiring an obsolete MCP process.

#### Scenario: Skipped mutation with missing plugin
- **WHEN** plugin mutation is skipped and the target plugin manifest or skill is missing or stale
- **THEN** plugin readiness is false, convergence remains partial, and every existing ProjectAtlas process remains alive

#### Scenario: Skipped mutation with missing registry
- **WHEN** registry mutation is skipped and no global `projectatlas` registration exists
- **THEN** registry readiness is false, convergence remains partial, and every existing ProjectAtlas process remains alive

### Requirement: Codex registry verification is exact and structured
The Windows installer SHALL parse `codex mcp get projectatlas --json` and require the exact `stdio` command plus complete ordered launch arguments. It SHALL normalize only executable, database, and config path values for Windows path comparison.

#### Scenario: Exact target registration
- **WHEN** the JSON registration command and ordered `--require-version`, `--db`, optional `--config`, and final `mcp` arguments exactly match the verified target
- **THEN** registry readiness is true

#### Scenario: Substring or ordering false positive
- **WHEN** a command or argument merely contains an expected value, an argument is reordered, or an extra argument exists
- **THEN** registry readiness is false

#### Scenario: Registration targets another project
- **WHEN** the registration names the target runtime but its database or config path belongs to another project root
- **THEN** registry readiness is false and the installer does not implicitly initialize, mutate, or reuse that other project's index

### Requirement: Obsolete process retirement is narrow and handle-bound
The Windows installer SHALL retire at most one obsolete stable-mirror process and only after target readiness plus final creation-time, image-path, complete-command, MCP-mode, observed-version, and image-digest revalidation against the held process handle.

#### Scenario: Exact obsolete MCP owner
- **WHEN** exactly one obsolete stable-mirror process is running in MCP mode, the target runtime/plugin/registry are ready, and every final identity field still matches
- **THEN** the installer terminates only that process and proceeds to the bounded mirror retry

#### Scenario: Final identity changes
- **WHEN** creation time, image path, command arguments or mode, observed obsolete version, or image digest differs at final revalidation
- **THEN** the installer reports the corresponding typed partial state and leaves the process alive

#### Scenario: Owner is absent, current, inaccessible, or ambiguous
- **WHEN** no exact obsolete owner exists, the owner already runs the target version, inspection is denied, or more than one candidate matches
- **THEN** the installer reports a typed partial state and does not terminate any ProjectAtlas or Codex process

### Requirement: Handoff convergence is bounded and truthful
After an eligible retirement the installer SHALL retry stable-mirror synchronization once, verify the resulting target runtime, and report complete or partial convergence without escalating process scope.

#### Scenario: Retry succeeds
- **WHEN** the exact obsolete owner exits and the single mirror retry installs and verifies the target runtime
- **THEN** the installer reports completed handoff and complete convergence

#### Scenario: Retry fails
- **WHEN** the exact owner is retired but the single mirror retry cannot install and verify the target runtime
- **THEN** the installer reports `retry_failed`, keeps convergence partial, and preserves the verified versioned runtime and generated project-local configs

#### Scenario: Real-host release proof
- **WHEN** release readiness is evaluated for this handoff
- **THEN** an external installed-Codex test SHALL prove parent survival, obsolete-child replacement, exact target version, and successful MCP initialization; local fixtures alone are insufficient
