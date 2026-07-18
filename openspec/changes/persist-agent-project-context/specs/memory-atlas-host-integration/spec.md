## ADDED Requirements

### Requirement: Memory Atlas complements host-owned context surfaces
ProjectAtlas SHALL own only reviewed project-local bird's-eye orientation and checkpoints. Each harness SHALL retain ownership of transcripts, compaction summaries, personal/global memory, native goals, tasks, rules, skills, plugins, credentials, configuration, and execution controls.

#### Scenario: Codex has an unfinished native goal and memories enabled
- **WHEN** Memory Atlas recovery runs
- **THEN** ProjectAtlas returns its project goal and checkpoint without importing, editing, completing, or duplicating Codex-owned goal or memory state

#### Scenario: Host lacks native memory or goals
- **WHEN** a supported host resumes a project
- **THEN** the Memory Atlas still supplies project orientation while the host uses only its documented execution/task capabilities

### Requirement: Host capabilities are declared rather than invented
Packaged integration guidance SHALL describe verified memory, goal, skill, plugin, MCP, lifecycle, restart, compaction, and subagent capabilities for supported hosts, including activation, trust, policy, maturity, automatic/manual behavior, and truthful fallbacks. Durable guidance SHALL target observable capabilities rather than model-version folklore.

#### Scenario: Optional or experimental host memory is disabled
- **WHEN** the host does not enable that feature
- **THEN** ProjectAtlas recovery remains functional and guidance does not claim the host store was read or updated

#### Scenario: Capability is unavailable in a host
- **WHEN** a host lacks a documented lifecycle or goal API
- **THEN** packaged guidance declares the manual fallback and does not emit invalid configuration or pretend automatic recovery occurred

### Requirement: Supported lifecycle recovery is quiet and read-only
When documented host hooks are available, trusted, enabled, and permitted, ProjectAtlas integration SHALL use startup/resume/post-compaction and supported subagent entry to inject a fixed instruction for one read-only recovery brief. It SHALL NOT automatically write Memory Atlas records, host memory, host goals, task state, or transcripts. Successful maintenance SHALL stay out of normal user-facing output unless it changes the plan or reveals a warning/failure.

#### Scenario: Codex starts, resumes, clears, or continues after compaction
- **WHEN** the trusted ProjectAtlas `SessionStart` hook receives `startup`, `resume`, `clear`, or `compact`
- **THEN** it directs the agent to the selected project's recovery brief before broad source reads and performs no authored-state mutation

#### Scenario: Supported subagent starts
- **WHEN** the trusted ProjectAtlas `SubagentStart` hook runs
- **THEN** the child receives bounded task-specific recovery guidance without inheriting or mutating private parent memory or goals

#### Scenario: Hook is unavailable or untrusted
- **WHEN** hooks are disabled, pending review, changed, blocked by policy, or unsupported
- **THEN** ProjectAtlas exposes a visible truthful manual recovery path and does not label the startup automatic

### Requirement: Agents checkpoint at meaningful boundaries
Packaged guidance SHALL update Memory Atlas state at meaningful recovery, architecture-decision, issue/task-transition, and final-verification boundaries. Before each update, the agent SHALL compare current stable identities, replace changed facts, remove or supersede obsolete facts, keep unrelated protected facts, and submit one bounded conditional batch. Routine file edits SHALL NOT create diary entries or user-facing maintenance spam.

#### Scenario: Agent completes a significant issue slice
- **WHEN** the slice changes the durable checkpoint, accepted decision, architecture, blocker, or next action
- **THEN** the agent writes one compact reflection batch and retires obsolete context before continuing

#### Scenario: File edit changes no bird's-eye context
- **WHEN** implementation details change without altering durable orientation
- **THEN** no Memory Atlas write or maintenance message is required

#### Scenario: Context pressure blocks an update
- **WHEN** protected state cannot fit after deterministic cleanup
- **THEN** the agent receives pressure details, explicitly rewrites/removes low-value context, and does not hide the failure

#### Scenario: Quiet background maintenance has no durable change
- **WHEN** a harness-owned maintainer receives the bounded current brief plus an explicit checkpoint and submits an exact no-op batch
- **THEN** revision, timestamps, rows, and user-facing output remain unchanged

#### Scenario: Quiet background maintenance loses a revision race
- **WHEN** a background maintainer writes from a stale revision after a newer accepted update commits
- **THEN** the stale batch changes nothing, does not overwrite or automatically retry over newer facts, and surfaces only the typed conflict while recovery remains available

### Requirement: MCP and CLI surfaces remain streamlined and equivalent
The agent MCP surface SHALL add only `atlas_memory` for bounded reads and `atlas_memory_update` for atomic reflection batches, while recovery remains in `atlas_session_brief` and pressure summary remains in settings. CLI SHALL provide equivalent read/update behavior plus validation and explicit compaction administration. No separate Memory Atlas goal tool or generic admin multiplexer SHALL be added unless later measured agent workflows prove the two-tool design insufficient.

#### Scenario: Agent updates the overarching project goal
- **WHEN** the agent replaces the protected `project_goal` record
- **THEN** it uses the same `atlas_memory_update` batch as other Memory Atlas facts rather than a separate goal tool

#### Scenario: Human or agent needs deeper maintenance
- **WHEN** full validation or explicit compaction is required
- **THEN** the typed CLI performs it through the same service policy without expanding the default MCP inventory

### Requirement: Host integration never crosses private or project boundaries
Lifecycle and recovery integration SHALL ignore transcript paths and private memory locations, bind the current workspace through ProjectAtlas root rules, remain offline, and never mutate host-global configuration, registries, memories, goals, or task state without an explicit owning host operation. Returned Memory Atlas content SHALL be labelled as reviewed project data and SHALL NOT override higher-priority host or repository instructions.

#### Scenario: Hook receives transcript and plugin paths
- **WHEN** host lifecycle input contains private transcript or plugin-data locations
- **THEN** ProjectAtlas ignores them for project context, resolves only the verified workspace/index, and fails closed for missing or wrong-root state

#### Scenario: Integration tests run on a clean host
- **WHEN** packaged host behavior is tested
- **THEN** isolated homes/configs prove recovery semantics while real host-global state remains unchanged

### Requirement: Existing host and ProjectAtlas behavior stays compatible
Existing CLI/MCP requests, normal session brief behavior, generated configs, purpose-led navigation, and supported host guidance SHALL retain their established defaults when Memory Atlas recovery is not requested. New output SHALL remain TOON-first and bounded, with JSON equivalence where supported.

#### Scenario: Pre-Memory Atlas client connects
- **WHEN** it uses existing tool names and request shapes
- **THEN** the runtime preserves compatible responses and does not require Memory Atlas initialization or follow-up calls
