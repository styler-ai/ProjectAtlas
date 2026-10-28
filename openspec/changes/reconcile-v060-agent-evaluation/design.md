## Context

The September roadmap disposition explicitly scheduled #490 for v0.6.0. Its old v0.5 task slice remains unchecked, while native #493 and the versioned v0.6 graph currently omit it. The faulty planning assumption was that moving a milestone also moved task authority, release scope and native dependencies. All four representations must agree.

## Goals / Non-Goals

Keep the deliberate future target, give the evaluation one current owner and specification, and preserve honest evidence. The reconciliation does not run experiments, change runtime behavior, or delay RC1 for future feature work.

## Decisions

### Current task authority replaces historical delivery authority

Map #490 only to this change. Keep the original v0.5 task rows unchanged as explicitly deferred history, consistent with that proposal's disposition. Do not turn those unchecked rows into completed work or duplicate the open owner's mapping.

### Prerequisites reflect current inputs

#490 has sole parent #493 and directly blocks #493. Its direct blockers are #310 and #563 because the comparison must use the accepted agent interface and language-navigation contract. The already completed v0.5 issues are baseline history. #314 and #388 remain independent release siblings; this evaluation does not invent a serialized dependency on Memory Atlas or npm installation. A representative scenario may consume another sibling only after its accepted behavior is available and the preregistration records that input; scope expansion reconciles dependencies before execution.

### Freeze before observation and preserve every outcome

Before execution, freeze tasks and repositories, candidate and baseline artifacts, prompts and tool availability, success and stop rules, cold/warm conditions, rotated order, repeats, metrics, uncertainty, privacy and output limits. Select scenarios from accepted v0.6 contracts: ordinary source navigation, PHP and language breadth, classified documentation and root/worktree routing, and existing analysis where supported. Do not imply runtime reachability from static graph output.

Retain successes, failures, timeouts, invalid/setup traces, uncertainty and self-audit results. Publish exclusions with their preregistered reason rather than silently dropping runs. Report observed correctness, elapsed time, calls, wrong-file work and context separately from Atlas's modeled token estimates. A null or negative result is valid.

### Reuse the existing bounded harness

Audit existing preregistration, equal-arm harness and compact published artifacts first. Reuse them unchanged when adequate. Correct only a demonstrated gap, with causal and positive compatibility proof and applicable Rust/database/workspace checks if implementation changes. No new runtime or database architecture is introduced by planning. Architecture disposition: N/A: evaluation ownership and the existing equal-arm, offline, bounded artifact pipeline are unchanged.

## Risks / Trade-offs

- A new release label could hide stale task authority. Mitigation: reconcile one owner mapping, issue mirror, native parent/blockers and release graph before readiness.
- Tasks or metrics could be chosen after favorable results. Mitigation: freeze all inputs and decision rules before either arm runs.
- Outcomes could be discarded or private material published. Mitigation: retain all outcome classes in bounded local evidence, audit calculations and sanitization, and publish only safe compact artifacts.
- Future work could enter RC1. Mitigation: keep #490 in v0.6.0 and leave every execution/proof task unchecked.

## Migration Plan

1. Independently review this planning contract and exact issue/task mapping.
2. Update #490 and #493 scope, the versioned release graph and native relationships together; validate strict OpenSpec and IssueOps and read back live state.
3. Execute only after the current direct prerequisites are accepted; freeze the experiment before observing outcomes.
4. Publish the reviewed bounded result, including null/negative or no-product-code outcomes, before #493 can accept v0.6.0.

Rollback restores the earlier planning state without touching any product index, identity, approved purpose or release artifact. No database migration applies.

## Dependencies / Cross-Issue Impact

- #310 and #563 are direct implementation prerequisites; #314 and #388 remain independent siblings.
- #493 is the sole release parent and is directly blocked by #490 in addition to all its other accepted children.
- #715 owns v0.5.1-rc1 and has no dependency on #490. Confirmed RC1 defects continue under their existing RC1 owners.
- The historical `complete-v050-release-readiness` task rows remain unfinished historical context; this change is the only current #490 task authority.

## Open Questions

None.
