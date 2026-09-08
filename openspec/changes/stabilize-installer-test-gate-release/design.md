## Context

Three explicitly requested test hooks pause the Windows installer after writing a
ready marker. The Rust fixture removes the gate to release the child. A concurrent
provider lookup has reported access denied and aborted the installer under its
normal stop-on-error policy.

## Goals / Non-Goals

Preserve the existing exact-file hold/release handshake and all real installer
lifecycle assertions. Do not change production locks, budgets, or error handling.

## Decisions

Use `System.IO.File.Exists` in the three test-only gate loops. The native exact-file
predicate avoids PowerShell provider exceptions during file removal. Retain the
ready markers and existing 50 ms polling interval. A new retry owner or helper is
unnecessary for three one-line predicates.

## Risks / Trade-offs

`File.Exists` treats inaccessible files as absent. These gates are fixture-owned,
explicitly opted-in test files, so this is release semantics, not a production
ownership or authorization decision. Production file and lock validation remains
unchanged. Existing opposite-migration E2E proves held owners, release, recovery,
repair ordering, shared deadlines, and unrelated-state preservation.

## Migration Plan

Merge this shared test repair before rebasing #465 and retrying its normal push.
There is no data migration. Reverting the predicate restores the intermittent
provider failure and does not alter installed project state.

## Architecture

N/A: Existing installer, fixture, process, and lifecycle-lock owners are unchanged.

## Dependencies / Cross-Issue Impact

No implementation prerequisite. #572 is a direct child and blocker of #492 and a
shared validation prerequisite of #465. Document source and parser artifacts stay
in #465; this change repairs only the installer fixture baseline.

## Open Questions

None.
