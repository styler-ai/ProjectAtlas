## ADDED Requirements

### Requirement: Local navigation does not require the Git executable
ProjectAtlas SHALL keep structural/local initialization, scan, overview, summary, and MCP navigation operational in a valid checkout when the Git executable cannot be found.

#### Scenario: Installed CLI has no Git in PATH
- **WHEN** an initialized checkout is addressed by the installed candidate with PATH containing no Git executable
- **THEN** scan and overview succeed against the exact local project/database and do not create, reset, substitute, or select sibling state

#### Scenario: Persistent MCP has no Git in PATH
- **WHEN** one persistent stdio MCP process starts for that checkout without Git
- **THEN** session brief, overview, and file summary respond before clean shutdown while keeping stdin and project routing isolated

#### Scenario: Missing index without Git
- **WHEN** a valid checkout has no project-local index and Git cannot start
- **THEN** navigation returns the existing typed `init_required` guidance and does not treat Git absence as permission to reuse another database

### Requirement: Optional Git probe failures stay precisely classified
ProjectAtlas SHALL treat only executable-not-found during the optional effective-config probe as Git unavailable and SHALL preserve all other spawn, permission, timeout, output, malformed-response, wait, and cleanup failures.

#### Scenario: Git executable is absent
- **WHEN** process creation returns executable-not-found
- **THEN** the optional effective-config result is unavailable and local non-VCS operation continues

#### Scenario: Git cannot start for another reason
- **WHEN** process creation fails with permission denial or another non-not-found error
- **THEN** ProjectAtlas returns the typed failure and does not misreport it as ordinary Git absence

#### Scenario: Git child stalls or emits invalid output
- **WHEN** the child exceeds the existing deadline, inherits no usable stdin, emits malformed output, or fails during output/cleanup
- **THEN** the existing #409 deadline, null-stdin, diagnostic, and cleanup behavior remains fail-closed

### Requirement: VCS-only operations degrade explicitly
ProjectAtlas SHALL preserve typed VCS-unavailable evidence rather than claiming impact or repository facts that require native Git.

#### Scenario: Impact request needs Git
- **WHEN** VCS impact analysis is requested with no Git executable
- **THEN** the operation returns typed Git-unavailable evidence while the last valid local atlas remains unchanged and readable
