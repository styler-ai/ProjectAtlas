## ADDED Requirements

### Requirement: Readiness is independent from mutation policy
The Windows installer SHALL treat Codex plugin and MCP registry skip flags only as mutation controls. When Codex is available, it SHALL observe structured plugin and registry JSON before reporting complete convergence or retiring an obsolete MCP process. Plugin readiness SHALL require a list containing exactly one matching installed plugin, contract-typed fields, the target manifest, and the target skill digest. Registry readiness SHALL require contract-typed fields and an argument list containing only strings.

#### Scenario: Skipped mutation with missing plugin
- **WHEN** plugin mutation is skipped and the target plugin manifest or skill is missing or stale
- **THEN** plugin readiness is false, convergence remains partial, and every existing ProjectAtlas process remains alive

#### Scenario: Skipped mutation with missing registry
- **WHEN** registry mutation is skipped and no global `projectatlas` registration exists
- **THEN** registry readiness is false, convergence remains partial, and every existing ProjectAtlas process remains alive

#### Scenario: Malformed or duplicate integration state
- **WHEN** plugin or registry JSON has a wrong field type, the plugin collection is not a list, more than one matching plugin exists, or registry arguments are not a string list
- **THEN** readiness is false and every existing ProjectAtlas process remains alive

#### Scenario: Current marketplace ref has a stale skill
- **WHEN** the official marketplace already names the target release but its installed ProjectAtlas skill is missing or has the wrong digest
- **THEN** the installer removes and re-adds that plugin before evaluating readiness

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
The Windows installer SHALL inspect child and parent observations from one `Win32_Process` snapshot with a five-second operation timeout. It SHALL retire at most one obsolete stable-mirror process whose observed parent has an absolute `codex.exe` image, matching absolute command identity, and a creation time no later than the child's. Signature inspection SHALL resolve the module-qualified `Microsoft.PowerShell.Security\Get-AuthenticodeSignature` cmdlet from the trusted `$PSHOME\Modules\Microsoft.PowerShell.Security` tree and reject session command shadowing; the cmdlet SHALL report `Valid`, `SignatureType = Authenticode`, and signer simple name `OpenAI OpCo, LLC`. The installer SHALL capture the parent image digest and retire only after final child and parent creation-time, image-path, complete-command, relationship, and image-digest revalidation against held process handles, together with child MCP-mode and observed-version revalidation.

#### Scenario: Exact obsolete MCP owner
- **WHEN** exactly one obsolete stable-mirror process is running in MCP mode under an authentic observed Codex parent, the target runtime/plugin/registry are ready, and every final child and parent identity field still matches
- **THEN** the installer terminates only that process and proceeds to the bounded mirror retry

#### Scenario: Final identity changes
- **WHEN** either process's creation time, image path, or command differs, or the child's mode, observed obsolete version, or image digest differs at final revalidation
- **THEN** the installer reports the corresponding typed partial state and leaves the process alive

#### Scenario: Owner is absent, current, inaccessible, or ambiguous
- **WHEN** no exact obsolete owner exists, the owner already runs the target version, inspection is denied, the parent is non-Codex, unsigned, signed by another signer, incomplete, or more than one candidate matches
- **THEN** the installer reports a typed partial state and does not terminate any ProjectAtlas or Codex process

#### Scenario: Observed parent was created after the child
- **WHEN** an otherwise matching parent process has a creation time later than the MCP child
- **THEN** the installer reports `unsafe_owner` and leaves both processes alive

#### Scenario: Replacement readiness changes before retirement
- **WHEN** the target runtime digest, any of the three generated-config digests, the parent signature or digest, or late plugin/registry readiness differs from the captured ready state
- **THEN** the installer reports `replacement_readiness_changed`, leaves every process alive, and keeps convergence partial

#### Scenario: Runtime or generated config is not ready at final reporting
- **WHEN** the target runtime cannot be reverified or any generated config is missing, unsafe, unreadable, or differs from its validated digest
- **THEN** the installer keeps convergence partial, reports `runtime_mcp_configs_ready=false`, emits no generated-config integration-verified claim, and directs the operator to rerun the installer

#### Scenario: Generated config validation and digest share one snapshot
- **WHEN** the installer captures replacement readiness for a generated Codex, Claude Code, or OpenCode config
- **THEN** it validates that config's semantics and computes its SHA-256 from the same bytes before later digest drift revalidation

### Requirement: Handoff convergence is bounded and truthful
After an exact child retirement or an actual no-such-process/observed-exit result, the installer SHALL retry stable-mirror synchronization once, verify the resulting target runtime, and report complete or partial convergence without escalating process scope. Bounded JSON probes SHALL emit a parsed payload only after process and temporary-file cleanup succeeds; cleanup uncertainty SHALL return an unready result rather than abort the installer. Inspection and identity failures SHALL NOT be classified as `exited`, and the Codex parent SHALL never be terminated.

#### Scenario: Probe cleanup cannot be verified
- **WHEN** a bounded runtime or integration probe returns valid JSON but its owned process or temporary-file cleanup cannot be verified within the bound
- **THEN** the probe yields no ready payload and the installer reports a partial/unready result without terminating unexpectedly

#### Scenario: Retry succeeds
- **WHEN** the exact obsolete owner exits and the single mirror retry installs and verifies the target runtime
- **THEN** the installer reports completed handoff and complete convergence

#### Scenario: Retry fails
- **WHEN** the exact owner is retired but the single mirror retry cannot install and verify the target runtime
- **THEN** the installer reports `retry_failed`, keeps convergence partial, and preserves the verified versioned runtime and generated project-local configs

#### Scenario: Inspection failure is not an exit
- **WHEN** process inspection or identity revalidation fails without an observed child exit
- **THEN** the installer preserves every process, reports a typed partial state other than `exited`, and does not attempt the stable-mirror retry

#### Scenario: Real-host release proof
- **WHEN** release readiness is evaluated for this handoff
- **THEN** an external installed-Codex test SHALL prove parent survival, obsolete-child replacement, exact target version, and successful MCP initialization; local fixtures alone are insufficient
