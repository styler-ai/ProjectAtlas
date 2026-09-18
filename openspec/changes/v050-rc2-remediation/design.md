## Context

RC1 passed ordinary short commands but failed six distinct installed-product boundaries. Each issue has accepted live task wording; the release owner needs to admit those slices together without allowing its planning PR to introduce unrelated task authority.

## Decisions

The release-owner IssueOps check permits only direct children declared in that release owner's local graph. Their task slices must still exactly mirror their live issues and pass the normal issue contract. All other new mappings remain rejected.

One shared change owns the six task slices because it is the RC2 planning contract. Each bug keeps its existing runtime owner and receives one implementation PR after the plan is accepted.

## Non-Goals

- Stable promotion, a new generic launcher or parser framework, hook auto-trust, or containment bypass.

## Migration Plan

1. Accept this release-graph and task-authority repair.
2. Deliver #604 through #609 from refreshed main, one owning issue and PR at a time.
3. Run exact-package RC2 acceptance and publish a non-draft prerelease only after the graph closes.
