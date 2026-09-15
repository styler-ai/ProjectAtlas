## Context

The release workflow calls the same all-closed milestone checker used for closeout. The existing issue-map schema declares release roots and dependencies, but the current checker reads only task ownership. The release root must remain open through publication and readback; closing it early is not a remedy.

## Goals / Non-Goals

Goals: admit publication with one declared open release owner and closed children, reject declaration or membership drift, preserve strict closeout and exact-release repair.

Non-goals: runtime changes, automatic publication policy changes, stable authorization, issue mutation automation, or a second acceptance ledger.

## Decisions

- Add `--publication-version` as a distinct checker mode. Reuse `release_version.py` to classify its canonical tag and derive the milestone. Keep `--milestone` all-closed and reject mixing the two modes.
- Read the existing `release_graphs` declaration only for publication. Validate positive mapped identities, root membership, known unique non-self dependencies, acyclicity, and root blockers equal to all children. Require exact native milestone membership. Historical milestones without a graph retain all-closed validation.
- An open root permits the release operation to execute its remaining tasks; it does not complete those tasks. A closed root is accepted only for a non-draft existing release with matching tag/classification and a tag commit equal to the checked-out candidate. Reuse authenticated GitHub reads, Git commit resolution, and the canonical version classifier. Errors refuse admission.
- Keep normal OpenSpec/acceptance validation and existing exact-main and pre-mutation release checks. Native parent/blocker relationships remain an independently reconciled release prerequisite; do not restore the removed general mutation framework.
- Correct stale declined membership in the release declaration when reconciling the active native graph. Do not turn declined scope into accepted implementation.
- Architecture evidence is N/A: this corrects a predicate within the existing IssueOps-to-release workflow; it adds no component, data owner, storage, protocol, or dependency boundary. Update workflow prose to explain publication versus closeout.

## Risks / Trade-offs

- A broad owner exception could admit unfinished children or false closure: derive it from the validated graph and test open-child, drift, and premature-closure refusals.
- Repair could target a different version or commit: require the exact existing release identity for a closed owner and preserve the workflow's final identity checks.
- Graph declarations can retain declined scope: reconcile native membership and blockers before publication; test malformed and missing declaration behavior.

## Migration Plan

Land the checker and workflow invocation together after focused self-tests and required repository/hosted proof. Existing closeout callers retain their behavior. Reverting the change restores the publication blocker without altering releases or product data.

## Dependencies / Cross-Issue Impact

Issue #492 owns final release acceptance and is blocked by this defect fix. Issue #448 owns the historical generic release classifier being reused. No open implementation prerequisite remains; this fix must merge before candidate freeze restarts.

## Open Questions

None.
