## ADDED Requirements

### Requirement: Session brief returns an optional Memory Atlas recovery view
The existing `atlas_session_brief` SHALL accept an additive recovery request that returns bounded Memory Atlas orientation in the same read-only startup call. Requests that omit recovery SHALL preserve existing fields, defaults, behavior, and discovery budget.

#### Scenario: Existing client requests a normal brief
- **WHEN** a pre-change client calls session brief without Memory Atlas recovery
- **THEN** ProjectAtlas preserves the compatible response and requires no new mandatory call

#### Scenario: Agent resumes after compaction
- **WHEN** an agent calls session brief with recovery and its current task query
- **THEN** the response includes the highest-value project goal, scope, architecture, patterns, checkpoint, decisions, workflows, skills/plugins, blockers, and next calls within one bounded result

### Requirement: Recovery is a deterministic bird's-eye projection
Recovery SHALL return one coherent read snapshot ordered as project identity/freshness/pressure, overarching project goal and scope, architecture/pattern digest, current checkpoint/blockers/next action, applicable decisions/workflows, task-ranked skill/plugin routes, and exact reusable selectors/next calls. It SHALL NOT return unbounded history, full skill bodies, transcripts, host memory, or arbitrary documents.

#### Scenario: Long-running issue resumes
- **WHEN** the selected project contains a durable project goal and an active issue checkpoint
- **THEN** recovery shows both the overarching outcome and the issue-level next action so the issue does not replace the project goal in the agent's orientation

#### Scenario: Memory Atlas is empty
- **WHEN** a compatible project has no reviewed Memory Atlas rows
- **THEN** recovery reports an empty capability and continues with the normal purpose-led overview/folder/file/summary/slice path without inventing context

#### Scenario: Repeated recovery reads use unchanged state
- **WHEN** request parameters, context revision, structural generation, and the effective lifecycle evaluation instant are unchanged
- **THEN** normalized output is byte-identical and performs no write

### Requirement: Recovery references current skills, plugins, and source selectors
Skill and plugin routes SHALL store logical identifiers, bounded applicability terms, host-resolvable selectors, capability summaries, and optional fingerprints rather than copied bodies, executable commands, installation directives, or machine-local paths. Recovery SHALL return stale/unavailable state and SHALL direct the harness to resolve and read the complete current owning skill or plugin instructions through its trusted registry and policy before implementation. Source references SHALL use exact reusable project-relative folder, file, optional symbol/span, issue, OpenSpec change/task, or public documentation selectors.

#### Scenario: Rust architecture task resumes
- **WHEN** the task query matches Rust architecture and OpenSpec routes
- **THEN** recovery ranks those routes, returns their current selectors, and omits unrelated routes within the default budget

#### Scenario: Skill source changed or disappeared
- **WHEN** a stored route fingerprint or selector no longer resolves
- **THEN** recovery marks it stale or unavailable and does not return cached skill instructions as current truth

#### Scenario: Memory record points into source
- **WHEN** an architecture or checkpoint record references a file and symbol
- **THEN** the response includes an exact selector and next summary/slice call rather than only a display label

#### Scenario: Durable decision points to changed local source
- **WHEN** a referenced file or symbol is stale, ambiguous, or missing in the current structural generation
- **THEN** recovery preserves the reviewed decision, labels the selector unresolved, and directs the agent through current purpose/graph navigation instead of deleting or rewriting the decision

### Requirement: Recovery is bounded and independently versioned
Recovery rows and serialized bytes SHALL have configured limits, stable tie-breakers, explicit returned/omitted counts, and context-revision-bound pagination. When a page also includes structural selectors, it SHALL bind structural generation independently. A memory-only change SHALL invalidate the context cursor; a source-only publication SHALL invalidate only the structural portion it affects.

#### Scenario: Relevant context exceeds the default budget
- **WHEN** more applicable records exist than fit
- **THEN** ProjectAtlas returns the highest-priority rows, exact truncation counts, and a bounded next read through `atlas_memory`

#### Scenario: Memory changes between pages
- **WHEN** a context update commits after page one
- **THEN** the old cursor fails with typed stale-context state instead of mixing revisions

#### Scenario: Structural generation changes independently
- **WHEN** source publication advances without a Memory Atlas update
- **THEN** the context revision remains current while stale structural selectors are reported independently

### Requirement: Recovery rejoins the purpose-led source funnel
Memory Atlas recovery SHALL provide orientation and candidate-reducing references, then direct the agent through folder purpose plus graph role, file purpose plus relevant connections, summary plus trust/coverage, and exact slice. It SHALL NOT replace current-source freshness, purpose navigation, graph retrieval, summaries, or slices.

#### Scenario: Agent needs implementation details
- **WHEN** recovery identifies the owning architecture area and checkpoint
- **THEN** the next call narrows folders/files through purpose and graph evidence before source summary or slice

### Requirement: Recovery performs no implicit mutation
Memory Atlas recovery SHALL NOT initialize, scan, refresh, migrate, compact, update timestamps, write telemetry, advance revisions, change active project state, create SQLite sidecars, read host-private memory/transcripts, or access the network.

#### Scenario: Read-only recovery against stable state
- **WHEN** the database and selected root are captured before and after recovery
- **THEN** database bytes, sidecars, timestamps, revisions, telemetry, root selection, and private host state are unchanged

#### Scenario: Recovery detects pressure or conflict
- **WHEN** the Memory Atlas is pressured, a checkpoint is stale, or a route is unresolved
- **THEN** the response returns a typed warning/blocker and explicit next action rather than silently mutating or guessing
