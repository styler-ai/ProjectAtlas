## Why

#490 was deliberately moved from v0.5.0 to v0.6.0, but its issue still names the old release owner and evaluation cycle. The v0.6 milestone therefore contains an issue without the matching native hierarchy or declared release graph. Reconcile that planning state without adding evaluation work to v0.5.1-rc1 or claiming that an experiment has run.

## What Changes

- Give #490 a current v0.6 evaluation specification and its own task authority; retain the unfinished v0.5 task rows as historical planning, outside accepted delivery.
- Admit #490 as a direct child and blocker of #493. Its direct prerequisites are #310 for the settled agent-facing interface and #563 for the accepted language-navigation scenarios. Preregistration design may proceed in parallel; candidate execution consumes those accepted inputs.
- Preregister equivalent baseline and ProjectAtlas arms before observing results, retain unfavorable outcomes, and publish bounded sanitized observed measurements separately from modeled savings.
- Reuse the existing harness when adequate; a reproducible no-product-code outcome is valid. This change is planning-only and ready for later implementation after its prerequisites land.

## Capabilities

### New Capabilities

- `agent-navigation-evaluation`: preregistered, reproducible real-task evaluation for the v0.6 evidence cycle.

### Modified Capabilities

None. This work measures accepted product behavior and does not redefine it.

## Non-Goals

- No v0.5.1 RC1 implementation, experiment execution, favorable-result promise, or completion claim.
- No replacement benchmark framework, product feature, parser, schema, host hook, or automatic collection of private source or prompts.
- No repeating closed v0.5 work or retaining its old release dependencies as current blockers.

## Impact

Only OpenSpec ownership, issue text, native issue relationships and the release graph change during this reconciliation. Later execution reuses the repository's evaluation harness and compact artifact conventions. Any demonstrated product or harness defect requires its owning specification, causal proof and review; scanner or measurement output alone does not authorize broad product restrictions.
