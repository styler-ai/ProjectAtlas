# Proposal

## Why

The packaged reminder registers only SessionStart. Its root-session compaction coverage does not establish that subagents receive version-matched ProjectAtlas guidance at startup or recover it after compaction. RC3 needs a truthful, tested recovery contract for agents working in separate checkouts.

## What Changes

- Reuse the trusted, read-only package reminder for supported subagent startup events.
- Verify host behavior after subagent compaction and use its supported context mechanism or explicit persistent/manual recovery guidance.
- Prove skill rereading and correct checkout selection in an isolated parent/subagent workflow, preserving RC3 CLI-first guidance.

## Capabilities

### New Capabilities

- `agent-skill-recovery`: version-matched skill guidance and checkout selection at supported agent lifecycle boundaries.

### Modified Capabilities

None.

## Impact

Plugin hook registration, existing reminder scripts/instructions, agent integration documentation, and their owning regression/host proof. No database, schema, new dependency, or new orchestration service.

## Release Scope

Accepted for v0.5.0-rc3 under release owner #492. Implementation integrates after #620 establishes the accepted readiness baseline; host investigation may proceed independently.

## Non-Goals

No memory system, dashboard, per-tool enforcement, automatic initialization or scanning, or guarantee of undocumented host events. A reminder alone is not proof that an agent read the skill.
