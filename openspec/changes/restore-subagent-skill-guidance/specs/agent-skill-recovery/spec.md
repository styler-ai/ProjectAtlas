# Spec Delta

## Purpose

Keep ProjectAtlas agents oriented to the installed skill and intended checkout across supported startup and context recovery boundaries.

## ADDED Requirements

### Requirement: Supported subagent startup guidance
The package SHALL supply version-matched skill and root-selection guidance at supported subagent startup boundaries through the existing trusted read-only reminder.

#### Scenario: Trusted subagent startup
- **WHEN** a supported host starts a subagent with a reviewed package
- **THEN** the subagent receives the installed skill path and instructions to read it and select its intended checkout before Atlas navigation

#### Scenario: Disabled or untrusted hooks
- **WHEN** hooks are disabled or the package is untrusted
- **THEN** the integration preserves host trust behavior and documents the manual skill-reading fallback without claiming hook delivery

### Requirement: Truthful compaction recovery
The integration SHALL provide verified supported context delivery or explicit persistent/manual guidance for skill rereading and root reselection after subagent compaction. Documentation SHALL distinguish tested automatic recovery from manual fallback and reminder delivery from observed agent action.

#### Scenario: Supported automatic recovery
- **WHEN** the tested host supports subagent recovery context after compaction
- **THEN** real-host proof observes the renewed guidance, complete skill read, and correctly rooted Atlas navigation

#### Scenario: Host lacks automatic recovery context
- **WHEN** the host cannot deliver supported recovery context to a compacted subagent
- **THEN** documentation identifies that limitation and the persistent/manual recovery path, without promising a reminder after every compaction

### Requirement: Read-only checkout-safe guidance
Lifecycle guidance SHALL preserve existing databases and require explicit checkout selection. Exact-checkout guidance SHALL prefer the atlas CLI; registered aliases, compact briefs, and federation SHALL retain MCP support.

#### Scenario: Two subagents in separate checkouts
- **WHEN** two subagents use different worktrees before and after supported recovery
- **THEN** each reads the matching skill and navigates its intended root without inheriting the other checkout's selection

#### Scenario: Wrong root or missing index
- **WHEN** a root is ambiguous, wrong, or lacks an index
- **THEN** guidance requires explicit root selection and skill-directed recovery without hook-driven initialization, scanning, or database mutation
