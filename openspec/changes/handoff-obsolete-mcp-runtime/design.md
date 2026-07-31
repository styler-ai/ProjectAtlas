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

1. **Separate mutation from readiness.** Skip environment variables guard only plugin/registry mutation calls. Readiness always observes the current Codex state; when Codex is present, missing or stale managed state keeps convergence partial. This avoids turning administrative policy into termination authority.
2. **Use one structured registry verifier.** Both pre-update currentness and post-update readiness parse `codex mcp get projectatlas --json`, require `stdio`, compare the exact command, and compare the complete ordered argument list. Only the command and values following `--db`/`--config` receive case-insensitive Windows path normalization; version, flags, ordering, and `mcp` remain exact.
3. **Use a handle-bound retirement guard.** Candidate selection requires one stable-path process whose parsed final argument is `mcp`, captures creation time and complete argv, and associates the probed obsolete version with a SHA-256 image identity. Immediately before retirement the version is reprobed. The retirement helper holds the process handle while it revalidates creation time, image path, command line read from that handle, full argv, and image digest, then terminates only that handle.
4. **Fail closed and retry once.** Any inspection, ambiguity, readiness, identity, access, timeout, or post-retirement copy failure returns a typed partial handoff state. The installer attempts the stable-mirror sync once after handoff and never escalates to parent or name-wide termination.

Alternatives rejected: parsing human-oriented Codex output permits substring false positives; WMI-only final identity leaves a PID-reuse gap; terminating all `projectatlas.exe` processes violates unrelated project and host ownership; requiring a full host restart defeats installer convergence.

## Risks / Trade-offs

- **Native command-line inspection can be unavailable on a restricted process** -> classify inspection/access failure and leave the process alive.
- **The obsolete executable can change between version probe and retirement** -> bind the observed version to a before/after digest and recheck the digest while holding the process handle.
- **Retirement succeeds but mirror replacement still fails** -> report `retry_failed`; keep the verified versioned runtime and generated configs usable.
- **Fixtures cannot prove the real Codex parent/child lifecycle** -> retain exact-version real-host handoff as a hosted release gate.

## Migration Plan

Ship the installer and tests together. Existing installs require no state or schema migration; rerunning the installer observes and repairs eligible state. Rollback is the previous installer because versioned runtimes and project-local configs remain intact even when handoff stays partial.

## Open Questions

- The release gate still needs a real installed Codex host to confirm parent survival, child replacement, exact target version, and successful MCP initialization after handoff.
