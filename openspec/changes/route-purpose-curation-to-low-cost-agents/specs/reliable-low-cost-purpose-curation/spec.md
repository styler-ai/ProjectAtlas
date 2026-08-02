## ADDED Requirements

### Requirement: Purpose curation uses the lowest reliable host-supported tier
ProjectAtlas guidance and actionable handoffs SHALL instruct a capable host to use its lowest reliable reasoning and cost tier for bounded purpose creation or correction.

#### Scenario: Host exposes model and reasoning choices
- **WHEN** a host can select among reliable bounded subagents and reasoning levels
- **THEN** the handoff selects the lowest reliable cost/reasoning combination rather than a flagship tier

#### Scenario: Host exposes one fixed reliable bounded tier
- **WHEN** the host supports isolated bounded subagents but no model or reasoning selector
- **THEN** the handoff delegates at that fixed tier instead of falling back to the main agent

#### Scenario: Host has no bounded isolated subagent execution
- **WHEN** the host cannot run an isolated bounded subagent
- **THEN** the main agent processes the low-scope queue without expanding its scope

### Requirement: Model examples are conditional and non-binding
Packaged guidance MAY name current economical tiers as examples but SHALL keep the durable rule capability-based and SHALL remain correct when those names are unavailable or change.

#### Scenario: Codex offers Luna with low reasoning
- **WHEN** Luna-low is available and reliable for bounded structured purpose metadata
- **THEN** guidance may recommend it as a conditional example while retaining the generic fallback

#### Scenario: Claude Code offers an economical reliable tier
- **WHEN** Haiku or another reliable low-cost tier is available
- **THEN** guidance may recommend it conditionally without making it a ProjectAtlas requirement

#### Scenario: Named example is unavailable
- **WHEN** a host inventory does not contain a named example
- **THEN** the host follows `lowest_reliable_host_supported` and does not fail or select another provider-specific invariant

### Requirement: Handoffs remain bounded, exact, and host-owned
ProjectAtlas SHALL emit consistent selection guidance while preserving non-overlapping queue ownership, state tokens, approved API writes, quiet maintenance, compact payload bounds, and host ownership of agent execution.

#### Scenario: Compact session brief contains a purpose handoff
- **WHEN** a low-scope purpose batch is actionable in compact output
- **THEN** the selection instruction remains present within the 4 KiB compact bound without duplicating expanded prose

#### Scenario: Several curators are available
- **WHEN** the host partitions a large low-scope queue
- **THEN** each path has exactly one bounded owner and successful purpose API writes are immediately agent-approved

#### Scenario: ProjectAtlas runtime emits the handoff
- **WHEN** init or session brief reports purpose work
- **THEN** ProjectAtlas does not spawn an agent, edit SQLite directly, or mutate a wrong root or missing index implicitly
