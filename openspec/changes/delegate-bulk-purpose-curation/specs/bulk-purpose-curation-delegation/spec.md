## ADDED Requirements

### Requirement: Purpose Delegation Guidance
ProjectAtlas agent guidance and runtime handoffs SHALL recommend subagent delegation at the lowest reliable reasoning and cost tier supported by the host for planned folder and file purpose creation or correction when the host supports subagents.

#### Scenario: Planned purpose creation or correction is needed
- **WHEN** ProjectAtlas reports missing, stale, vague, or generic folder/file purposes for planned curation
- **AND** the current host supports a bounded subagent mechanism
- **THEN** the agent SHOULD delegate purpose creation or correction at the lowest reliable reasoning and cost tier supported by the host.

#### Scenario: Initial purpose creation is needed
- **WHEN** a project needs initial folder or file purpose creation
- **AND** the current host supports a bounded subagent mechanism
- **THEN** the agent SHOULD delegate that initial creation at the lowest reliable reasoning and cost tier supported by the host.

#### Scenario: Guidance offers current host/model examples
- **WHEN** guidance names a current host/model option for purpose curation
- **THEN** it MAY cite Codex `gpt-5.6-luna` with `low` reasoning or Claude Code `haiku` only as conditional examples after the durable capability-based rule.
- **AND** it SHALL NOT make either example a hard universal requirement.
- **AND** it SHALL preserve a generic lowest-reliable-tier fallback as names and availability change.

#### Scenario: Runtime returns an actionable purpose handoff
- **WHEN** init or an expanded or compact session brief returns an actionable `purpose_handoff`
- **THEN** `recommended_subagent_reasoning` SHALL be `lowest_reliable_host_supported`.
- **AND** the handoff instructions SHALL recommend the lowest reliable reasoning and cost tier supported by the host.
- **AND** a compact handoff SHALL retain that selection instruction without duplicating the remaining expanded instruction prose.
- **AND** the main-agent fallback SHALL remain available when the host cannot delegate the bounded batch.

#### Scenario: Agent notices a bad purpose during normal work
- **WHEN** any agent notices a wrong, stale, vague, or generic purpose during normal work
- **THEN** that agent MAY correct it along the way with ProjectAtlas MCP or CLI purpose APIs.

#### Scenario: Delegated subagent owns purpose work
- **WHEN** a subagent is assigned bounded purpose creation or correction
- **THEN** the subagent MAY apply purposes through ProjectAtlas MCP or CLI purpose APIs.
- **AND** the subagent SHALL report changed paths and commands or tools used.

#### Scenario: Purpose is written through ProjectAtlas APIs
- **WHEN** an agent or subagent writes a purpose through ProjectAtlas MCP or CLI purpose APIs
- **THEN** that purpose SHALL be treated as agent-approved without a second main-agent approval pass.

#### Scenario: Host lacks delegation support
- **WHEN** the host does not expose a subagent mechanism
- **THEN** the current agent SHALL perform purpose curation directly.

### Requirement: Purpose Delegation Boundaries
Delegated purpose subagents SHALL operate on bounded context and SHALL NOT mutate ProjectAtlas storage directly.

#### Scenario: Subagent receives purpose task
- **WHEN** the agent delegates purpose creation or correction
- **THEN** the task SHALL include bounded queue rows, summaries, outlines, or exact snippets needed to draft specific purposes.

#### Scenario: Subagent attempts direct storage mutation
- **WHEN** purpose curation is delegated
- **THEN** the subagent SHALL NOT edit SQLite directly.
