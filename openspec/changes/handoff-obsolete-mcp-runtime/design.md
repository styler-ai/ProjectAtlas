## Context

The installer already writes and verifies absolute project-local MCP configs and prefers a versioned runtime. On Windows, a running stable-mirror MCP child can keep `LocalAppData\ProjectAtlas\bin\projectatlas.exe` locked, preventing the compatibility mirror from being replaced even though the target versioned runtime is ready. The safety boundary is process termination: PID, path, or text-output matching alone is insufficient authority.

## Goals / Non-Goals

**Goals:**

- Converge a locked obsolete stable mirror without restarting or terminating the parent Codex host.
- Require observed target plugin and registry readiness even when mutations are intentionally skipped.
- Bind the one permitted termination to an unambiguous process handle and complete executable/command identity.
- Preserve typed partial states and one bounded mirror-copy retry.

**Non-Goals:**

- Broad process discovery or termination by executable name.
- Terminating current, unrelated, malformed, inaccessible, or ambiguous ProjectAtlas processes.
- Changing runtime, MCP, database, or non-Windows installer contracts.
- Treating local fixture coverage as proof of a real installed Codex host handoff.

## Decisions

1. **Separate mutation, repair, and readiness.** Skip environment variables guard only plugin/registry mutation calls. Readiness always observes the current Codex state through typed JSON and requires exactly one matching installed plugin and one exact registry entry. A marketplace already at the target ref still removes and re-adds the plugin when the current-ref skill is missing or its digest is stale. When Codex is present, missing, malformed, duplicate, or stale managed state keeps convergence partial.
2. **Use exact structured verifiers.** Plugin fields must have their contract types. Registry verification parses `codex mcp get projectatlas --json`, requires typed `name`, `enabled`, `stdio`, command, and a list containing only string arguments, then compares the exact command and complete ordered argument list. Only the command and values following `--db`/`--config` receive case-insensitive Windows path normalization; version, flags, ordering, and `mcp` remain exact.
3. **Authenticate the observed owner from one bounded snapshot.** One `Win32_Process` query has a five-second operation timeout and supplies both child and parent observations. Candidate selection requires one stable-path MCP child and an absolute `codex.exe` parent whose command begins with the same absolute path and whose creation time is no later than the child's. Signature inspection resolves the module-qualified `Microsoft.PowerShell.Security\Get-AuthenticodeSignature` cmdlet from the trusted `$PSHOME\Modules\Microsoft.PowerShell.Security` tree and rejects session command shadowing. It must report `Valid`, `SignatureType = Authenticode`, and signer simple name `OpenAI OpCo, LLC`; selection records the parent digest with both complete process identities. Non-Codex, unsigned, wrong-signer, inaccessible, incomplete, and ambiguous observations preserve all processes.
4. **Revalidate replacement readiness and retire through held handles.** The handoff reads each generated config once and binds semantic validation plus SHA-256 to that byte snapshot. It also captures the verified target runtime digest. Immediately before retirement the installer rechecks the target and all three config digests, the parent signature and digest, and plugin/registry readiness; drift returns `replacement_readiness_changed`. Before final reporting it re-probes the target runtime and rechecks all three config snapshots; uncertainty in that runtime/config bundle keeps convergence partial, sets `runtime_mcp_configs_ready=false`, and suppresses integration-verified claims. The native helper holds child and parent handles while it revalidates creation times, image paths, complete argv, parent relationship, and both image digests, then terminates only the child handle. The parent is never a termination target.
5. **Fail closed and retry once.** Bounded JSON probes buffer a valid payload until process and temporary-file cleanup succeeds; cleanup uncertainty becomes an unready result rather than an installer abort. Inspection, identity, access, readiness, timeout, or copy failures return typed partial states. Only an actual no-such-process result or observed child exit is `exited`; the installer attempts one stable-mirror sync after that exit or an exact child retirement and never escalates to parent or name-wide termination.

Alternatives rejected: parsing human-oriented Codex output permits substring false positives; WMI-only final identity leaves a PID-reuse gap; terminating all `projectatlas.exe` processes violates unrelated project and host ownership; requiring a full host restart defeats installer convergence.

## Risks / Trade-offs

- **Native command-line or Authenticode inspection can be unavailable on a restricted process** -> classify inspection/access failure and leave every process alive.
- **A process or replacement artifact can change after selection** -> bind both process images, the target runtime, and generated configs to captured digests; recheck replacement readiness immediately before retirement and process identity through held handles.
- **Retirement succeeds but mirror replacement still fails** -> report `retry_failed`; keep the verified versioned runtime and generated configs usable.
- **Fixtures cannot prove the real Codex parent/child lifecycle** -> retain exact-version real-host handoff as a hosted release gate.

## Migration Plan

Ship the installer and tests together. Existing installs require no state or schema migration; rerunning the installer observes and repairs eligible state. Rollback is the previous installer because versioned runtimes and project-local configs remain intact even when handoff stays partial.

## Open Questions

- The release gate still needs a real installed Codex host to confirm parent survival, child replacement, exact target version, and successful MCP initialization after handoff.
