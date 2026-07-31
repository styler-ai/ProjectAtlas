## Context

`effective_git_config_bare_setting` starts `git config` with piped output but no explicit stdin. Under a long-lived stdio MCP server, Windows passes the still-open transport handle to Git, so Git waits for input and root selection blocks until ProjectAtlas kills it at the existing deadline.

## Goals / Non-Goals

**Goals:**

- Make the shared effective Git-config probe independent of caller stdin.
- Prove the real persistent MCP transport across the ten-call startup and agent-navigation path, then immediately reuse the same session.
- Preserve current root classification, timeout, output, and error behavior.

**Non-Goals:**

- General subprocess supervision or new cancellation/containment guarantees.
- New dependencies, public types, protocol fields, or database behavior.

## Decisions

- Set `stdin(Stdio::null())` on the existing `git config` command. This is the standard-library ownership boundary that fixes every caller without changing the established subprocess lifecycle.
- Exercise the three affected MCP startup tools and seven common navigation tools over one real stdio session whose stdin remains open. A direct unit test would not recreate the inherited transport handle that caused the bug. Keep the existing complete advertised-tool contract separate because it owns schemas, valid arguments, and SQLite effects for all tools; run both tests before release publication.
- Retain the existing implementation for deadlines, output collection, status parsing, and root semantics. A larger supervisor was considered and rejected for this patch because it would add unrelated process and platform contracts.

## Risks / Trade-offs

- A transport-only regression can be slower than a unit test. → Keep one focused E2E with a five-second per-call bound.
- Other subprocess policy improvements remain outside this repair. → Track them separately only when a concrete failing contract requires them.
