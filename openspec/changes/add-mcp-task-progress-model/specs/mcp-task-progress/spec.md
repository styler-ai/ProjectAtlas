## ADDED Requirements

### Requirement: Task State Contract
ProjectAtlas SHALL define a typed MCP task-progress contract for future long-running MCP operations.

#### Scenario: Task state is inspectable
- **WHEN** a task-backed MCP operation exists
- **THEN** the server SHALL expose a typed task status with one of `pending`, `running`, `complete`, `failed`, or `canceled`.

#### Scenario: Unknown task is typed
- **WHEN** an agent requests status for an unknown task id
- **THEN** the server SHALL return a typed `not_found` result rather than panicking or requiring prose parsing.

### Requirement: Bounded MCP Task Surface
ProjectAtlas SHALL expose minimal MCP task status and cancellation tools backed by bounded MCP-session-local state.

#### Scenario: Task status lookup
- **WHEN** an agent calls `atlas_task_status` with a task id
- **THEN** the response SHALL include the task id, lookup status, operation kind when known, state when known, progress when known, and concise error/result fields when present.

#### Scenario: Task cancellation lookup
- **WHEN** an agent calls `atlas_task_cancel` with a task id
- **THEN** the response SHALL report `canceled`, `not_found`, `already_finished`, or `not_cancelable` as typed values.

#### Scenario: Registry remains bounded
- **WHEN** task records exceed the configured in-memory capacity
- **THEN** ProjectAtlas SHALL evict old completed records rather than retaining unbounded task history.

### Requirement: CLI Behavior Preservation
Task-backed MCP progress SHALL NOT change existing CLI command behavior.

#### Scenario: CLI scan remains synchronous
- **WHEN** a user runs the CLI scan command
- **THEN** the command SHALL keep its existing synchronous behavior unless a separate CLI change is approved.

### Requirement: Bounded Initial Scope
Initial task-progress support SHALL NOT move existing ProjectAtlas read or scan operations behind polling.

#### Scenario: Direct source read stays direct
- **WHEN** an agent calls file summary or slice
- **THEN** ProjectAtlas SHALL return the direct result rather than requiring task polling.

#### Scenario: Long operations remain synchronous for this release
- **WHEN** an agent calls MCP scan, watch refresh, or search in this release
- **THEN** ProjectAtlas SHALL keep the existing synchronous response shape and leave async migration to a later operation-specific change.
