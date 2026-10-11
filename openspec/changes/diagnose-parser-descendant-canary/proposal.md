## Why

The Windows clean optional-parser construction fails its descendant canary without exposing the causal child outcome, preventing RC3 acceptance. A previous successful run used the same scripts and hosted image; the current failure does not establish whether readiness timing, child launch, token admission, or marker access caused it.

A controlled fixture separately demonstrates that the former existence check can reject an incomplete marker before the live child finishes writing it. The correction waits for valid contents within the existing readiness bound and retains the child handle for live-descendant cleanup proof. The historical failure lacks marker state; this demonstrated defect must not be presented as its uniquely established cause.

## What Changes

- Report bounded child exit, exact marker state, and elapsed time at the existing canary failure boundary.
- Retain the child handle and establish a live admitted descendant before testing job-close cleanup.
- Repair only the demonstrated fixture or launch defect and verify it through the existing Windows clean construction and complete pack verification.
- Preserve outer deadlines, AppContainer admission, process ownership, immutable artifacts, and fail-closed publication.

## Capabilities

### New Capabilities

- `parser-construction-fixture-proof`: causal construction-fixture outcomes and proof of admitted live-descendant cleanup.

### Modified Capabilities

None. This tooling and fixture repair preserves the existing parser containment and construction requirements. The added fixture-proof contract owns the observable missing state; a demonstrated product containment-contract change requires revisiting this scope before implementation.

## Impact

Issue #664 owns the existing Windows AppContainer launcher canary and its causal proof. The disposable-principal broker and optional-parser workflow consume that result. Release owner #492 depends on acceptance of this fix on main before freezing a new clean RC3 candidate. The diagnostic-first contract is ready for bounded implementation after the synchronized issue/task mapping and native release graph pass their gates.

## Non-Goals

No containment bypass, removal of token or descendant-cleanup checks, retry-only acceptance, blanket timeout increase, new process framework, runtime/database migration, dependency update, or unrelated workflow change. Stable promotion remains separate #602 work.
