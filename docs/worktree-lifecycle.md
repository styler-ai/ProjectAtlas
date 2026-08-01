# ProjectAtlas Worktree Lifecycle

This document owns the planned v0.5.0 architecture for [issue #430](https://github.com/styler-ai/ProjectAtlas/issues/430). It separates branch-derived atlas state from repository-owned reviewed purposes and lifetime token telemetry. The design is planning-only until its OpenSpec tasks are implemented and verified.

## System and Ownership

Each checkout remains an exact source/index boundary. One continuity database belongs to the logical repository and contains no source graph.

```mermaid
flowchart TB
    Host[CLI and MCP host]
    Root[Typed repository and worktree discovery]

    subgraph Common[Logical repository continuity]
        Continuity[(continuity.db)]
        Purposes[Reviewed purpose authority]
        Telemetry[Deduplicated lifetime telemetry]
        Registry[Worktree and import receipts]
    end

    subgraph Main[Checked-out main worktree]
        MainSource[Main branch source]
        MainAtlas[(worktree projectatlas.db)]
    end

    subgraph Issue[Checked-out issue worktree]
        IssueSource[Issue branch source]
        IssueAtlas[(worktree projectatlas.db)]
    end

    Host --> Root
    Root --> MainSource
    Root --> IssueSource
    MainSource --> MainAtlas
    IssueSource --> IssueAtlas
    Root --> Registry
    Purposes --> Continuity
    Telemetry --> Continuity
    Registry --> Continuity
    MainAtlas -. exact-root freshness .-> Purposes
    IssueAtlas -. exact-root freshness .-> Purposes
    MainAtlas -. usage event .-> Telemetry
    IssueAtlas -. usage event .-> Telemetry

    MainAtlas ~~~ IssueAtlas
```

Ownership rules:

- Worktree databases own files, summaries, symbols, relations, generations, freshness, watcher state, and purpose suggestions for exactly one checkout.
- The continuity database owns approved purpose revisions, logical worktree registrations, import receipts, telemetry event identity, exact retained aggregates, and repository-lifetime reporting.
- The continuity database never becomes a source atlas, including when it is stored by a bare/common Git manager.
- Git remains authoritative for branches and worktrees; ProjectAtlas reports and protects its own state without silently mutating Git.

## Worktree Lifecycle

Lifecycle transitions are typed and revalidated. A path is a locator, not the durable repository identity.

```mermaid
flowchart TB
    Start([Selected root])
    Discovered[Discovered]
    Init[Init required<br/>valid checkout without atlas]
    Registered[Registered<br/>identity and schemas valid]
    Active[Active]
    Refresh[Refresh required<br/>source or policy changed<br/>run watch once or scan]
    Relocate[Relocation required<br/>old root absent and candidate moved<br/>require explicit proof]
    Recovery[Recovery required<br/>crash, corruption, or incompatible schema<br/>restore or migrate last-valid state]
    Plan[Retirement planned<br/>dry run is complete<br/>changed evidence or cancel returns active]
    Blocked[Blocked<br/>dirty, unique, live, or uncertain<br/>resolve and refresh plan]
    Sealed[Sealed<br/>retirement manifest verified]
    Ready[Git removal ready<br/>continuity verified]
    Removed([User-authorized Git removal])

    Start --> Discovered
    Discovered --> Init --> Registered
    Discovered --> Registered
    Registered --> Active

    Active --> Refresh
    Refresh -.-> Active
    Active --> Relocate
    Relocate -.-> Active
    Active --> Recovery
    Recovery -.-> Active

    Active --> Plan
    Plan -.-> Active
    Plan --> Blocked
    Blocked -.-> Plan
    Plan -->|exact apply| Sealed
    Sealed --> Ready
    Ready --> Removed
```

When the Git executable is unavailable, structural root and database validation still allow local ProjectAtlas operations. Git-dependent branch, dirty, merge, and removal evidence is marked unavailable, and retirement cannot claim readiness.

## Purpose Continuity

Reviewed text is shared only after exact repository/path/content validation. Freshness always comes from the addressed worktree.

```mermaid
sequenceDiagram
    participant Agent
    participant WorktreeA as Worktree A atlas
    participant Service as Purpose service
    participant Shared as Repository continuity
    participant WorktreeB as Worktree B atlas

    Agent->>WorktreeA: Review purpose with state token
    WorktreeA->>Service: Exact path and current content identity
    Service->>Shared: Conditional approved revision write
    Shared-->>Agent: Approved revision

    Agent->>WorktreeB: Request purpose for same path
    WorktreeB->>Service: Current worktree path and content identity
    Service->>Shared: Read repository-approved revision
    alt Content identity matches
        Shared-->>WorktreeB: Approved purpose
    else Path exists with changed content
        Shared-->>WorktreeB: Stale reviewed purpose
    else Path absent or rename ambiguous
        Shared-->>WorktreeB: No current purpose or typed review guidance
    end
```

Folder purposes may be reused when the normalized folder exists. File purposes require the approved content identity. Deleted and branch-only paths remain historical repository knowledge but never appear as current sibling source.

## Repository Telemetry Publication

The repository ledger admits an event and updates its aggregates in one SQLite transaction. Retry identity makes an uncertain response safe.

```mermaid
sequenceDiagram
    participant Caller as CLI or MCP runtime
    participant Telemetry as Telemetry service
    participant DB as continuity.db

    Caller->>Telemetry: Event with repository, worktree, runtime, and event IDs
    Telemetry->>DB: BEGIN validated write
    DB->>DB: Insert unique event identity
    alt New valid event
        DB->>DB: Update exact bounded aggregates
        DB->>DB: COMMIT event and aggregates
        DB-->>Caller: Admitted once
    else Retry of committed event
        DB->>DB: ROLLBACK no-op write
        DB-->>Caller: Deterministic duplicate result
    else Busy, invalid, or failed write
        DB->>DB: ROLLBACK
        DB-->>Caller: Typed retry or terminal error
    end
```

Repository reports read indexed aggregate rows rather than all retained events. Per-worktree and per-session totals are dimensions of the same authority, not separately summed databases. Raw-detail eviction never changes exact retained component totals.

## Retirement and Recovery

ProjectAtlas retirement protects ProjectAtlas state and produces Git-authority guidance. It does not silently remove a branch or checkout.

```mermaid
flowchart TD
    Start[Request retirement dry run]
    Resolve[Resolve exact repository and worktree identity]
    Inspect[Inspect source, Git evidence, processes, schemas, WAL, purposes, and telemetry]
    Complete{Evidence complete and safe?}
    Plan[Return signed or state-token-bound plan]
    Apply[Revalidate every material field]
    Same{Evidence unchanged?}
    Seal[Seal target registration and contribution epoch]
    Manifest[Persist verified bounded retirement manifest]
    Verify{Continuity and manifest reconcile?}
    Ready[Return explicit Git removal guidance]
    Block[Return typed blockers without mutation]
    Recover[Preserve last-valid state and return recovery action]

    Start --> Resolve --> Inspect --> Complete
    Complete -- no --> Block
    Complete -- yes --> Plan --> Apply --> Same
    Same -- no --> Block
    Same -- yes --> Seal --> Manifest --> Verify
    Verify -- yes --> Ready
    Verify -- no --> Recover
```

The seal covers only the target worktree registration and contribution epoch; sibling continuity remains writable. The manifest contains reconciliation counts, import receipts, authority epochs, hashes, and recovery instructions. Rebuildable source graphs are not archived, and unreconciled unique state blocks retirement.

Migration holds source exclusion across a recoverable saga: read-only preflight and final snapshot; non-authoritative destination prepare; destination verification; database-enforced legacy-writer fence; then conditional registration switch. A pre-fence crash leaves the source authoritative and forces refresh of any prepared import after possible late writes. A post-fence crash completes the already durable prepared switch. The source is never fenced before destination durability, and no state exposes dual writers. Aggregate-only sources are combined only with provably disjoint provenance. Newer, corrupt, wrong-root, overlapping, unfenceable, or ambiguous databases remain preserved and return typed recovery guidance. Recovery after a continuity-authoritative write reconciles forward into a new epoch rather than reopening an older source.

## Architecture Invalidation Conditions

Revisit this split only if measurement or platform evidence proves one of these conditions:

- SQLite cannot meet the measured many-worktree write latency, contention, WAL growth, or durability limits with short prepared transactions and bounded checkpoint policy.
- Git common-directory storage cannot remain project-local, permission-safe, relocatable, and available to all supported worktree topologies.
- Content-identity purpose projection cannot express required rename or generated-source behavior without repeated incorrect stale/current classifications.
- Exact telemetry deduplication cannot migrate supported predecessor aggregates without either losing totals or accepting double counting.
- Extending the existing root/admin MCP surface makes lifecycle state less discoverable or breaks the stable tool contract; only then should a replacement-free top-level tool be considered.

Any redesign must preserve exact-root source isolation, non-destructive schema handling, offline local operation, and recoverable telemetry/purpose history.
