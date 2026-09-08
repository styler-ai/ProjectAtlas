## Context

The fixture seeds a sibling database but explicit `atlas_init(project_path=...)`
selects the nested project database. Its later implicit calls select the sibling.
The hosted payloads prove this mismatch and a purpose freshness refusal, but do
not identify the event that invalidated the purpose mutation's source witness.

## Goals / Non-Goals

Use one index for the stated lifecycle and preserve the real protocol assertions.
Do not change product selection, freshness, storage, deadlines, or recovery policy.

## Decisions

Use the canonical nested database already selected by explicit project routing.
Assert the initialized and selected database paths against that expected path,
using existing TOON decoding and JSON assertion helpers. This is smaller and more
truthful than inserting a refresh of the unrelated external index or changing
product routing to accommodate a fixture.

## Risks / Trade-offs

The identity correction may expose a separate freshness defect. Retain any such
failure and determine its actual source delta or observer cause; a passing rerun
alone is not a causal explanation. Existing successful purpose and graph payload
assertions remain mandatory alongside the new identity check.

## Migration Plan

Land the independent fixture repair on main before refreshing #572 and #465.
There is no product migration. A revert restores the known fixture mismatch.

## Architecture

N/A: Existing product, process, and test-harness ownership remains unchanged.

## Dependencies / Cross-Issue Impact

No implementation prerequisite. #574 is a direct child and blocker of #492 and
a shared validation prerequisite of #572. #465 continues after #572 closes.

## Open Questions

None.
