## Context

`.github/scripts/issue-checklists.py` validates each linked Mermaid block with the repository-locked Node parser. `mermaid_syntax_is_valid` currently returns `False` both for parser rejection and `subprocess.TimeoutExpired`, so the architecture-link caller reports a busy-host timeout as invalid source syntax.

The change stays inside this existing Python IssueOps boundary. It does not affect the Rust runtime, SQLite state, published CLI/MCP contracts, or the linked architecture itself.

## Goals / Non-Goals

**Goals:**

- Preserve valid, invalid, timed-out, and unavailable parser outcomes through the architecture-link diagnostic.
- Retry the same exact block once only after its first timeout, with the existing fixed per-attempt timeout.
- Preserve every existing architecture target and syntax rejection.
- Prove the behavior with focused deterministic self-tests.

**Non-Goals:**

- A process abstraction, pool, service, new dependency, or configurable retry policy.
- Retrying invalid syntax or unavailable execution.
- Relaxing link, repository, heading, fenced-block, or issue/milestone validation.

## Decisions

### Use one closed standard-library outcome at the existing parser function

Replace the lossy boolean parser result with one small `Enum` owned by `issue-checklists.py`. An uncached one-attempt runner remains the only subprocess owner; the bounded validator performs the timeout-only retry and caches only its final result. This is the minimum split that both launches a real second attempt and prevents timeout, invalid syntax, and unavailable execution from collapsing into one value.

Alternative rejected: exceptions or several booleans would spread classification across callers and make impossible combinations representable.

### Retry only the first timeout

The uncached runner returns one closed outcome. The cached validation boundary calls it a second time with the identical diagram only when the first call timed out. Invalid and unavailable outcomes are terminal. Both attempts retain the existing 30-second limit, so worst-case parser time remains bounded to two real subprocess attempts.

Alternative rejected: a generic retry helper or configurable policy adds machinery for one fixed recovery rule.

### Preserve target-level diagnostics without weakening `any valid block` behavior

The architecture-link validator still accepts a target when any non-empty fenced Mermaid block validates. When none validate, it reports the observed terminal class or classes together with the existing architecture URL. Missing or empty blocks keep their existing structural failure.

### Test the result boundary directly

Focused self-tests inject parser outcomes and count attempts. They cover timeout recovery, terminal timeout, invalid syntax without retry, unavailable execution without retry, exact attempt bounds, target-specific diagnostics, and the unchanged planned-issue and milestone checks. No wall-clock sleep or real timeout is needed for causal unit proof; the existing locked-parser tests retain integration coverage.

## Risks / Trade-offs

- Two real timeouts can extend one failing diagram check to at most 60 seconds. → Keep the existing 30-second per-attempt bound and exactly one retry.
- Cached terminal outcomes could hide a later recovery in the same process. → Cache only the final validation result after the bounded retry, preserving the existing per-diagram cache contract.
- Multiple failing blocks can produce ambiguous classification. → Report the bounded set of observed terminal classes for the target rather than inventing one source defect.

## Migration Plan

1. Publish this standalone #544-owned OpenSpec change and the release-graph membership through the non-closing planning pull request.
2. Rebase the issue worktree on accepted `main`, implement the minimal IssueOps change, and run focused and full IssueOps gates.
3. Complete independent implementation/intent review, hosted checks, task synchronization, merge, and issue closure.

Rollback is a normal revert of the IssueOps implementation; there is no data or public protocol migration.

## Dependencies / Cross-Issue Impact

#544 has no implementation predecessor. It is one direct `v0.5.0-00` child of release owner #492, directly blocks #492, and remains an independent first-wave lane in the shared release graph.

## Open Questions

None.
