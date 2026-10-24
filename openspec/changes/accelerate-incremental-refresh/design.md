# Design

## Context

See [proposal.md](proposal.md) for the user problem and scope. This is the implementation plan for #708, a v0.6.0 must-have. It changes no RC4 input.

The current CLI enters `run_single_watch_refresh_controlled`, then `refresh_index_controlled`. MCP watch-once calls the same refresh owner without using its server's `SourceObservationRegistry`. An unchanged call walks admitted paths, hashes their contents, loads stored nodes and compares the two sets. It avoids parsing and a new generation when nothing changed. A changed one-shot call can stage a full publication and perform two further exact validation scans. Continuous watching already supplies `WatchChangeSet` to an incremental path.

The initial cost is therefore tied to all entries and source bytes, with sorting/map construction as well. These are source findings, not current latency measurements. Historical #358 timings are useful fixture context but are not an RC4 baseline.

## Goals / Non-Goals

**Goals:** reuse reliable changes, minimize affected work, make repeated refreshes fast, preserve exact results and choose a bounded default budget from the planned work.

**Non-Goals:** a generic change-tracking framework, a new crate/database/daemon by default, Git-only discovery, metadata-only freshness, or speed claims based on unoptimized binaries or disabled normal telemetry.

## Decisions

### Keep one refresh and publication owner

CLI, MCP and continuous watching will use the existing shared refresh and atomic-publication owners. Known changes describe additions, modifications, deletions and renames of any size. The work plan includes affected text, symbols, summaries, relations and configuration-dependent data. It must not assume that one changed file means only one graph row changes.

Reuse `WatchChangeSet`, the existing source observer, source verification and store transaction boundaries. Do not add parallel adapter implementations or a second source-of-truth index. Keep native project/worktree identity captured throughout the operation.

### Reuse trustworthy observations; handle missing history honestly

For a long-lived process, use the existing observer only while its root, policy, publication identity and event continuity remain valid. Integrating that observation with writes must fence concurrent writers and source changes; a read stamp alone is not permission to publish.

A fresh CLI process cannot infer edits made before it started from an empty event queue. Without trustworthy retained evidence it performs exact initial discovery. Start observation before this baseline where the existing backend supports it, so later source changes can be checked during the same refresh. Lost events, unavailable observation or invalid identities require exact reconciliation. No unchanged result may be inferred solely from equal size/mtime or the absence of newly observed events.

This chooses the existing observer plus exact fallback, rather than introducing an unproved persistent service or silently relaxing freshness. If measurements show that cross-invocation evidence needs an additional component, publish and review that concrete design before adding it; the issue cannot close with only a fast MCP path and an unexplained slow CLI path.

### Prove the standalone CLI approach before production changes

Task 2.1 preregisters a separate cold-CLI latency target and checks whether exact initial discovery plus reduced comparison/update work can meet it. Task 2.3 must prove that selected CLI approach as well as the warm MCP path. A fast MCP result cannot substitute for a slow CLI result.

If exact cold discovery cannot meet that target, publish and review a concrete trustworthy cross-invocation evidence design before implementing that alternative. Cover how history is obtained and retained, root/identity validation, gaps, restart, fallback, ownership and any required process lifetime or deployment behavior. Do not silently introduce a service. The zero-traversal bound applies to a continuously observed MCP route or a CLI route that actually implements such reviewed retained evidence; it is not promised for an ordinary fresh CLI with no history.

### Apply the delta and avoid repeated whole-project work

Convert discovered or observed changes into the existing incremental path. Use source/dependency invalidation rules to expand the affected set only when necessary. Relevant compiler, ignore, parser or publication-policy changes may legitimately invalidate a wider scope.

Use source-only stored fields for comparison where feasible, and compare ordered source records in a linear pass instead of building unnecessary maps on an unchanged result. These are concrete candidates to measure, not permission to add a speculative index or cache.

Reuse a valid observed epoch and per-source verification across staging and publication where the existing safety contract permits it. Remove repeated exact scans only after proving their race/failure protection is provided by the retained observation and writer checks. When that proof is unavailable, keep the exact fallback. Optimize initial/full discovery separately where measurement identifies avoidable work.

### Select the adaptive budget once at admission

Automatic budget selection uses the best reliable work estimate available before work begins: discovery needed, admitted file counts/bytes, known changes, affected graph work and parser costs. A cold operation includes the expected discovery cost; a verified warm no-op does not receive a full-rebuild estimate just because the repository is large.

Use one bounded, documented policy in the existing work-control owner. Choose its numerical coefficients and minimum/maximum from the baseline experiment before implementation. An explicit caller timeout takes precedence, including immediate zero-budget refusal and fail-closed unrepresentable deadlines. Do not reset the operation clock or extend caller limits during retries. Preserve cooperative checkpoints, cancellation and all existing memory/file/output/worker bounds; do not claim a hard wall-clock guarantee.

### Measure before choosing further optimizations

Use the actual optimized installed CLI and MCP with normal telemetry. Freeze fixture shapes and preregister numeric latency/resource targets before changing the implementation. Include large admitted trees, large source-byte totals, small and large deltas, warm/cold starts, no Git, ignore/config changes, and required full reconciliation on Windows, Linux and both macOS architectures.

Record distributions rather than one timing. Record visited entries, hashed bytes, parse candidates, decoded nodes, store statements/writes, generation changes, CPU and RSS where supported. Reuse `SourceVerificationWork` and existing harnesses; keep benchmark-only detail out of normal product output unless a real user contract needs it.

The hard work-count target for a verified unchanged observation is no project-tree traversal/content hashing/full-node decode, no parse work and no new publication. Delta processing must avoid unrelated parsing/publication. Cold discovery and justified recovery have separate baselines. A larger budget alone is never a performance result. Full/indexing and unaffected routes must not regress beyond preregistered noise/resource limits.

## Risks / Trade-offs

- Missing or coalesced events could hide an edit. Detect continuity loss and test overflow, offline edits, restart, races, same-size/same-mtime changes and exact fallback.
- A read-oriented observer could incorrectly authorize a write. Require writer-safe root/policy/generation validation and concurrent-reader/writer failure tests.
- A small delta could still rebuild the graph or decode every stored summary. Assert affected work and row/byte counts as well as elapsed time.
- Adaptive budgets could hide slow algorithms or violate caller intent. Test the estimator separately from speed and preserve explicit-limit precedence.
- Cold exact discovery still has a cost proportional to the evidence it must inspect. Report this honestly, measure it separately and do not claim constant-time discovery without a trusted change source.

## Migration Plan

No schema change, public route removal or new dependency is required by this plan. Introduce each proven improvement in the owning shared boundary with causal tests, retaining an exact fallback. If storage or a long-lived component becomes necessary, amend this design with compatibility, migration and rollback proof before implementation. Reuse the existing installed-upgrade and four-platform release gates before accepting v0.6.0.

## Dependencies / Cross-Issue Impact

#708 is a required v0.6.0 child and release blocker of #493. It has no implementation prerequisite: profiling and the shared refresh work can proceed independently. Preserve the CLI/MCP route decisions owned by #310 and interoperability with #314 rather than inventing a dependency on either feature. #388's packaged installation remains an acceptance consumer, not an implementation blocker.

#699 supplies the existing cooperative-budget and caller-limit contract. This change must update affected v0.6.0 default-budget guidance and tests together without weakening caller control. #358 supplies historical measurement context only; task 2.1 establishes the new baseline and numerical targets. No source or release change is made to RC4.

## Open Questions

None.
