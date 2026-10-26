# Indexing and incremental refresh

This is the target behavior for 0.5.1 issue #708: faster initial indexing, full reindexing, symbol/graph rebuilding and refresh. The accepted implementation retains exact one-shot reconciliation; local measured gains are recorded in the indexing reference, with installed platform acceptance still required.

## Shared owners and accepted behavior

- Continuous watching receives a `WatchChangeSet` and has an incremental path.
- `SourceObservationRegistry` keeps process-local source observations for some MCP reads.
- `run_single_watch_refresh_controlled` and MCP watch-once reach `refresh_index_controlled`; one-shot refresh captures exact source evidence and can transfer it to the shared mutable owner without another detection inventory.
- An unchanged one-shot pass avoids parsing and a new generation, but still walks and hashes admitted source and compares ordered source-only records without full-node decoding.
- A changed pass can stage a full publication and perform two further exact validation scans.
- Full scans use `run_scan_pipeline_controlled` to stage complete index state, revalidate inputs and publish atomically. CLI/MCP symbol rebuilding shares `run_symbol_build_pipeline_controlled`.

The focused baseline and draft results are in [the indexing reference](benchmarks/v0.5.1-indexing.md). A read-oriented observer or empty callback queue does not safely authorize bypassing exact reconciliation.

## Target refresh flow

```mermaid
flowchart TD
    Request[CLI or MCP indexing request] --> Select[Validate exact root and operation]
    Select --> Budget[Choose bounded default budget or honor caller limit]
    Budget --> Mode{Full build required?}
    Mode -->|Yes| Discover[Discover required sources]
    Discover --> Build[Read parse and build required data]
    Build --> Full[Stage complete index or projection]
    Mode -->|Refresh| Evidence{Reliable change history?}
    Evidence -->|Yes| Changes[Known changes and affected data]
    Evidence -->|No or continuity lost| Exact[Exact discovery or reconciliation]
    Exact --> Changes
    Changes --> Empty{Unchanged?}
    Empty -->|Yes| Noop[Return current result without new generation]
    Empty -->|No| Update[Existing incremental refresh owner]
    Full --> Guard
    Update --> Guard{Source and writer valid?}
    Guard -->|Yes| Publish[Atomic publication of complete required state]
    Guard -->|No| Recover[Refuse unsafe publication]
    Recover --> Prior[Preserve prior state and allow safe retry]
```

Budget admission includes discovery; do not walk the whole tree just to estimate the budget. The evidence step selects the required source evidence, not an unbudgeted scan. In RC1 every explicit CLI/MCP one-shot refresh takes the exact branch. The known-change branch represents the existing continuous watcher batch and retains final source validation; it is not a zero-source-work admission witness. A cold process may need an exact baseline; an empty new event queue says nothing about past edits. Wider reconciliation follows uncertainty or relevant policy changes. Required initial/full/rebuild work preserves its existing command semantics and cannot be replaced by an unrelated cached no-op.

Validation, cancellation or failure at any phase refuses unsafe publication and preserves prior state, including an absent initial index. Only explicit initialization creates missing project state; other routes keep their typed recovery guidance.

## Ownership and safeguards

Keep the existing filesystem, source-observation, work-control and SQLite/publication owners. Capture root, native project identity, policy and generation consistently. Integrate observation with writer checks before reusing it for publication. Do not transfer another project's database or trust size/mtime alone.

Use one operation plan for trusted no-op, affected work or a required full build. Compare lightweight ordered source records with stored hashes in a linear merge; do not decode the whole stored project just to learn which source changed. Reuse captured source bytes, compatible parse facts and valid verification evidence within that plan. This architecture preserves the planned work reduction; focused measurements confirm it, with broader platform proof at release acceptance.

Read, hash, parse, decode and write counts matter alongside elapsed time. An unchanged one-shot refresh performs one exact source detection/comparison and avoids full-node decoding, parsing and a new generation. A changed refresh expands only to affected data. Initial/full indexing, explicit symbol rebuilding, cold discovery, lost events and wider invalidation have distinct measured targets. Full/rebuild benchmarks prove complete work actually ran. Preserve parser/document coverage, existing force/rebuild semantics, authored purposes and bounded workers/memory.

The user accepted shipping RC1 with exact reconciliation and measured gains. Zero-source-work admission is deferred to unscheduled [#725](https://github.com/styler-ai/ProjectAtlas/issues/725), which requires a reviewed causal history witness. A fresh CLI and long-lived MCP explicit refresh both retain exact comparison, including same-metadata and delayed-callback edits. Their own preregistered latency and work targets remain required.

Default budgets reflect estimated discovery, source bytes and affected work within documented bounds. Explicit caller limits win, cancellation remains responsive and retries do not reset the clock. Larger timeouts alone do not prove a performance improvement.

Keep the existing 1,800-second maximum and larger-request clamp. A new cold workload without a reliable estimate keeps the 1,800-second automatic budget; estimating it must not add an unbudgeted tree walk. Only omitted-limit admission becomes adaptive. Explicit limits up to the ceiling, including 1,800 seconds, keep their existing meaning.
