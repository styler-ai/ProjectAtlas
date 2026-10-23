## Purpose

Let agents choose indexing work budgets from truthful public schema and help while preserving cooperative cancellation and atomic publication.

## ADDED Requirements

### Requirement: Public indexing deadlines describe shared work

MCP scan, symbol-build and one-shot-watch descriptions and applicable CLI/help guidance SHALL describe a cooperative whole-indexing-work deadline covering traversal, source hashing, parsing and publication checkpoints. They SHALL state the 30-minute default/ceiling and higher-value clamping without promising a hard startup-to-response deadline.

#### Scenario: Budget expires before parsing

- **WHEN** a short indexing budget expires during source hashing
- **THEN** the operation refuses at that shared checkpoint, preserves the last valid index, identity and approved purposes, and a subsequent adequate-budget refresh recovers
- **AND** schema and help predict this behavior without increasing the existing timeout
