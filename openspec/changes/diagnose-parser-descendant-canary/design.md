## Context

See proposal.md for the observed failure. The launcher self-test treats every nonzero `LaunchContained(tree)` result as `descendant-parent-canary`. Its parent fixture polls for an AppContainer marker for three seconds; the child checks its token, writes the marker, and sleeps five seconds. The outer launch has a ten-second deadline. The failed job does not reveal which condition failed. The prior successful job used the same launcher, workflow, named-object admission results, and hosted image.

## Goals / Non-Goals

**Goals:** Establish the exact failed fixture state, fix its demonstrated cause, and prove admitted live-descendant cleanup through the existing clean Windows construction.

**Non-Goals:** No new process owner or diagnostic framework; no product containment-policy change, sensitive environment/path output, arbitrary sleeps, retry-only acceptance, or workflow deadline increase.

## Decisions

- Keep the existing launcher, child handle, marker, outer deadline, and job lifetime. Add only fixed outcome names, numeric exit/status, marker state, and bounded elapsed time at the current failure owner. Report early exit separately from marker deadline and invalid marker. Do not infer an AppContainer token failure from a marker timeout.
- Establish that the admitted child is still alive when the cleanup assertion begins. A marker from an already exited child cannot prove descendant cleanup. Retain the handle until the existing cleanup owner retires the process tree; do not add an independently managed process lifecycle.
- Obtain causal proof before changing timing or access. If the readiness window is the demonstrated cause, keep its finite bound below the existing outer launch deadline and keep the child alive through that observation window. A token, creation, or marker-access failure requires fixing that actual owner. Blanket timeout inflation and repeated successful retries do not establish the missing state.
- Exercise successful readiness, early child failure, missing/invalid marker, deadline, and descendant cleanup with the smallest existing injected/self-test boundary. Use only owned fixtures. Run affected hosted Windows clean construction, then the complete parser-pack verification and runtime lifecycle gates. Linux construction remains a compatibility gate; an isolated Windows diagnostic cannot replace the all-target acceptance run.
- The fixture-proof specification records the missing causal observation and cleanup state while preserving product containment requirements. If causal evidence requires changing a product containment contract, revise the proposal/specification and scope before that implementation.

## Risks / Trade-offs

- Generic failure persists -> preserve each child outcome at the exact failed stage and check negative cases.
- Child exits before cleanup -> require a live admitted descendant and prove its retirement rather than relying on marker existence.
- More diagnostics expose secrets -> fixed outcomes and numbers only, with bounded output and no paths/environment dump.
- Local startup differs from hosted execution -> require exact hosted clean construction and final pack verification before acceptance.

## Migration Plan

No runtime/database migration is required. Land the reviewed correction under #664, refresh main, and freeze a new exact RC3 candidate. Rebuild and verify clean parser artifacts; never reuse the failed run as an accepted handoff. Preserve the previous candidate, logs, authored databases, and unrelated worktrees.

## Dependencies / Cross-Issue Impact

#664 has no open implementation prerequisite and is a direct child/blocker of #492 in v0.5.0-00. It owns the Windows construction failure; #492 owns complete installed release acceptance and publication and closes last. Other accepted RC3 work remains complete. Stable promotion #602 stays separate.

## Open Questions

None.
