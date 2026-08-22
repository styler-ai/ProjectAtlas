# ProjectAtlas v0.6 Agent Surface Architecture

These views own the intended post-v0.5 CLI/MCP decision and compatibility boundary for #310, the project-authored Memory Atlas boundary for #314, and the feature-free installed-product acceptance boundary for #493. The release graph in `openspec/issue-map.json` makes #310 and #314 direct children of #493, orders #314 after #310, and leaves #493 to close last.

## Evidence-led route decision

The installed stable v0.5 product is frozen before comparison. Results classify every public route; they do not begin from a desired tool count or transport doctrine.

```mermaid
flowchart LR
    baseline[Installed stable v0.5 baseline]
    cli[CLI-first task runs]
    mcp[MCP-first task runs]
    mixed[Mixed-route task runs]
    metrics[Success, freshness, context, calls, latency, recovery]
    classify[Classify every public route]
    surface[Smallest accepted v0.6 surface]
    nochange[Valid no-removal outcome]

    baseline --> cli --> metrics
    baseline --> mcp --> metrics
    baseline --> mixed --> metrics
    metrics --> classify --> surface
    classify -->|no material benefit| nochange
```

## Accepted CLI and MCP ownership

The concise CLI is the normal one-shot shell route. MCP remains first-class for session capabilities; both converge on explicit root routing and the same services.

```mermaid
flowchart LR
    agent[User or agent]
    cli[atlas CLI]
    mcp[MCP atlas tools]
    root[Explicit project-root routing]
    services[Shared repository services]
    tasks[Session-local task lifecycle]

    agent -->|ordinary completed shell operation| cli
    agent -->|no shell, typed session, persistent routing| mcp
    cli --> root
    mcp --> root
    mcp -->|only with a real producer| tasks
    root --> services
    tasks --> services
```

## Versioned compatibility migration

Runtime, CLI, MCP inventory, plugin skill, generated hosts, fixtures, and documentation move as one contract. Rollback restores a complete previous pair instead of mixing generations.

```mermaid
stateDiagram-v2
    [*] --> V05: Installed stable v0.5 contract
    V05 --> Frozen: Baseline and task outcomes captured
    Frozen --> Ratified: Every public route dispositioned
    Ratified --> Candidate: Runtime, skill, hosts, fixtures updated
    Candidate --> V06: Installed E2E and migration proof pass
    Candidate --> Rework: Drift or task regression
    Rework --> Ratified: Correct owning boundary
    V06 --> Rollback: Compatibility rollback requested
    Rollback --> V05: Restore complete compatible release
    V06 --> [*]
```

## Memory Atlas authored-state ownership

Memory Atlas is project-authored orientation in the existing project database. Structural source state remains derived, and structural and context revisions advance independently.

```mermaid
flowchart TB
    files[Project files and filesystem] --> scan[Scan and graph publication]
    scan --> derived[(Derived source, purpose, and graph state)]
    author[Explicit reviewed reflection batch] --> service[projectatlas-service policy, budgets, and recovery]
    service --> db[projectatlas-db transaction and query owner]
    db --> memory[(Authored Memory Atlas rows)]
    db --> context[Independent context revision]
    scan --> structural[Independent structural generation]
    core[projectatlas-core closed records, keys, references, and errors] --> service
    core --> db
    cli[CLI] --> service
    mcp[MCP atlas_memory and atlas_memory_update] --> service
    brief[Optional session-brief recovery] --> service
    settings[Content-free settings status] --> service
    derived -. portable selectors .-> memory
```

## Conditional reflection, transaction, and recovery state

```mermaid
stateDiagram-v2
    [*] --> Read: read bounded rows and context revision R
    Read --> Validate: submit complete upsert/remove batch for R
    Validate --> Reject: invalid, wrong root, stale, busy, or exhausted
    Validate --> Noop: exact state already present
    Validate --> Reconcile: validate keys, bytes, rows, and eligible cleanup
    Reconcile --> Pressure: protected state cannot fit
    Reconcile --> Commit: all writes and cleanup fit
    Commit --> Advanced: atomically publish revision R plus one
    Reject --> Unchanged: rows and revision unchanged
    Noop --> Unchanged
    Pressure --> Unchanged
    Advanced --> Recovery: bounded read-only recovery
    Unchanged --> Recovery
    Recovery --> Funnel: purpose then graph then summary then exact slice
    Recovery --> [*]
    Funnel --> [*]
```

Recovery never initializes, scans, refreshes, changes roots, bypasses freshness, or mutates authored state.

## Host trust and skill-resolution boundary

```mermaid
sequenceDiagram
    participant Event as Verified host lifecycle event
    participant Manual as Manual session-brief fallback
    participant Atlas as ProjectAtlas recovery
    participant Registry as Trusted host skill registry
    participant Skill as Current complete SKILL.md
    participant Agent as Agent instruction stack
    alt documented, trusted, enabled event
        Event->>Atlas: fixed bounded read-only recovery request
    else unavailable, untrusted, or undocumented
        Manual->>Atlas: explicit bounded recovery request
    end
    Atlas-->>Registry: logical required and recommended skill routes
    Registry->>Skill: resolve trusted current artifact
    Skill-->>Agent: load complete instructions before use
    Atlas-->>Agent: reviewed project data below system, developer, user, repository, and skill authority
    alt timeout, missing route, stale route, or failure
        Atlas-->>Agent: typed bounded fallback with manual next action
    end
```

The integration does not read or write host-private registries, caches, credentials, transcripts, personal memories, goals, or hidden state.

## v0.6.0 holistic release acceptance

```mermaid
stateDiagram-v2
    [*] --> Hierarchy: #310 and #314 are closed, reviewed children of #493
    Hierarchy --> Inventory: freeze installed CLI, nested command, and MCP inventory
    Inventory --> Regression: execute every supported route, including unchanged routes
    Regression --> HostProof: packaged host, process, root, freshness, format, error, cancellation, and source-evidence E2E
    HostProof --> Candidate: build exact release candidate
    Candidate --> Readback: independently verify tag, assets, checksums, runtime, plugin, MCP, and Latest policy
    Readback --> Defect: confirmed blocker
    Defect --> Owner: reopen or return defect to owning child issue
    Owner --> Hierarchy: land accepted fix and restart complete proof
    Readback --> Stable: accepted candidate with no blocker
    Stable --> FinalReadback: repeat installed and hosted stable proof
    FinalReadback --> Close: close #493 last with hierarchy and milestone complete
    Close --> [*]
```
