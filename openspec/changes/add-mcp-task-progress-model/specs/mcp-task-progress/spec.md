## ADDED Requirements

### Requirement: Task State Contract
ProjectAtlas SHALL define a typed MCP task-progress contract before long-running MCP operations are moved to asynchronous execution.

#### Scenario: Task state is inspectable
- **WHEN** a task-backed MCP operation exists
- **THEN** the server SHALL expose a typed task status with one of pending, running, complete, failed, or canceled.

### Requirement: CLI Behavior Preservation
Task-backed MCP progress SHALL NOT change existing CLI command behavior.

#### Scenario: CLI scan remains synchronous
- **WHEN** a user runs the CLI scan command
- **THEN** the command SHALL keep its existing synchronous behavior unless a separate CLI change is approved.

### Requirement: Bounded Initial Scope
Initial task-progress support SHALL be limited to explicitly selected long-running MCP operations.

#### Scenario: Direct source read stays direct
- **WHEN** an agent calls file summary or slice
- **THEN** ProjectAtlas SHALL return the direct result rather than requiring task polling.

#### Scenario: Long operation can report progress
- **WHEN** scan, watch refresh, or broad search is made task-backed
- **THEN** the server SHALL expose status, completion, failure, and cancellation semantics for that operation.
