# Design

## Context

See proposal.md. The existing package has trusted read-only readiness scripts and a shared instruction asset. The Codex 0.162.1 documentation and version-matched source distinguish root `SessionStart` compaction context from child `SubagentStart` startup context; `PostCompact` does not expose an additional developer-context output. An isolated native Codex 0.160.0 parent startup previously reproduced a failed packaged `SessionStart` command with no reminder context. The Windows host selects a shell and `commandWindows` remains a command string, not an interpreter selector. A simple safe command string cannot both use PowerShell syntax and execute under CMD. The package therefore uses an explicit `SystemRoot` PowerShell path and documents CMD as a manual skill-read fallback; causal Windows tests cover PowerShell delivery and CMD fail-closed behavior, including current-directory executable shadowing. Actual corrected parent and two-subagent context delivery, skill reads, and recovery remain required proof; the earlier parent failure alone does not establish a subagent lifecycle failure.

## Goals / Non-Goals

Restore the skill/root instructions using supported lifecycle context. Preserve existing trust, bounded output, quiet failure, and read-only behavior. Do not build a lifecycle framework or infer undocumented context delivery.

## Decisions

- Reuse the existing reminder and package paths for supported SubagentStart delivery rather than duplicate handlers. On Windows, invoke the system Windows PowerShell executable through its explicit `SystemRoot` path and pass the readiness script through the host-supplied `PLUGIN_ROOT`. The command is PowerShell-only; CMD receives no automatic reminder and uses the documented manual skill-read fallback.
- Verify root SessionStart and subagent startup with the actual host. For compaction, use supported root SessionStart context and explicit persistent/manual subagent recovery instructions; adding an event without a supported additional-context output is insufficient.
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
