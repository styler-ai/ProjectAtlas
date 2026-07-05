## 1. Review

- [ ] 1.1 Review the task state names and status payload fields.
- [ ] 1.2 Decide which MCP operations, if any, should become task-backed first.
- [ ] 1.3 Confirm whether cancellation is required in the first implementation.

## 2. Implementation

- [ ] 2.1 Add typed task status structs/enums after contract approval.
- [ ] 2.2 Add minimal MCP status and cancellation tools if approved.
- [ ] 2.3 Move one long-running MCP operation behind the task model as a pilot.
- [ ] 2.4 Add serialization and in-process MCP tests for status, failure, completion, and cancellation.
