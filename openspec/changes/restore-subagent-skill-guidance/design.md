# Design

## Context

See proposal.md. The existing package has trusted read-only readiness scripts and a shared instruction asset. Official host documentation distinguishes root SessionStart compaction from SubagentStart; PostCompact plaintext is ignored. An isolated native Codex 0.160.0 parent startup reproduced a failed packaged SessionStart command with no reminder context. The host selects its active shell and can fall back to COMSPEC; commandWindows selects a command string, not its interpreter. Causal Windows tests therefore cover both PowerShell and CMD command execution. Actual corrected parent and two-subagent context delivery, skill reads, and recovery remain required proof; the parent failure alone does not establish a subagent lifecycle failure.

## Goals / Non-Goals

Restore the skill/root instructions using supported lifecycle context. Preserve existing trust, bounded output, quiet failure, and read-only behavior. Do not build a lifecycle framework or infer undocumented context delivery.

## Decisions

- Reuse the existing reminder and package paths for supported SubagentStart delivery rather than duplicate handlers. On Windows, explicitly launch the system Windows PowerShell executable and construct the readiness-script path from the host-supplied PLUGIN_ROOT inside that process, so either supported outer shell reaches the same existing handler.
- Verify automatic and manual subagent compaction with the actual host. Use a verified context mechanism when available; otherwise provide explicit persistent/manual recovery instructions and document the limitation. Adding an event whose output the host ignores is insufficient.
- Retain CLI-first exact-checkout guidance and explicit root selection. MCP remains available for registered aliases, briefs, and federation.
- Validate reminder delivery separately from observed skill reads and subsequent correctly rooted Atlas navigation. Use isolated host configuration and two checkout fixtures; never rebind shared MCP or modify project databases through a hook.

## Risks / Trade-offs

- Host lifecycle differs by version -> pin the tested host and describe unsupported boundaries explicitly.
- A subagent inherits its parent's root -> exercise separate checkout selection and wrong-root refusal.
- Hook changes invalidate trust receipts -> preserve existing validation and prove reviewed versus untrusted package behavior.
- Broad lifecycle testing becomes a new subsystem -> use existing harnesses and the smallest causal checks.

## Migration Plan

Integrate after #620, update package-owned assets and existing source contracts together, and run affected native/hosted proof. Rollback restores previous package assets; no database migration or repair is involved.

## Dependencies / Cross-Issue Impact

#645 is a direct child and blocker of #492 and depends on #620. Host investigation is independent; source integration waits for the accepted readiness baseline. No Dependabot ownership changes.

## Open Questions

None.
