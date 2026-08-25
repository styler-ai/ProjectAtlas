# v0.5.0 release architecture

Each mapped v0.5.0 issue owns one focused view below. The release graph in `openspec/issue-map.json` owns hierarchy and implementation order; #492 owns acceptance only and closes after every child issue.

## Issue task authority and owner slices

```mermaid
flowchart LR
    LocalTasks[(Mapped local tasks.md)] --> OwnerSlice{Issue-map owner slice}
    OwnerSlice --> Implementation[Exactly one visible Implementation Tasks section]
    IssuePacket[Complete issue packet] --> Acceptance[Exactly one canonical Acceptance and Review Tasks section]
    Implementation --> Sync[Exact text, order, ownership, and state mirror]
    Acceptance --> Gates[Five ordered review gates]
    Sync --> Contract
    Gates --> Contract
    PR[PR candidate branch] --> Owner[One open owner against live state]
    PR --> Base[Unrelated open slices against accepted PR base]
    Owner --> Contract
    Base --> Contract
    Closed[Already CLOSED mapped issue] --> Inert[Native closed state only; no body migration or validation]
    Reopened[Reopened mapped issue] --> Implementation
    Hidden[Hidden, duplicate, or legacy open fields] --> Reject[Fail closed]
    Contract --> Ready[Truthful incremental or closure-ready state]
```

## Acceptance-state transition

```mermaid
stateDiagram-v2
    [*] --> ImplementationIncomplete
    ImplementationIncomplete --> ImplementationIncomplete: remains incomplete
    ImplementationIncomplete --> ReviewReady: all implementation checked
    ReviewReady --> ReviewInProgress: first acceptance checked
    ReviewInProgress --> Complete: all five acceptance complete
    ReviewInProgress --> ImplementationIncomplete: implementation reopens and resets acceptance
    note right of ReviewInProgress
        Acceptance checks advance as a checked prefix only
    end note
    Complete --> [*]: closure or release allowed
```

## PHP language-guidance evidence flow

```mermaid
flowchart LR
    registry[Language capability registry] --> fixtures[PHP fixtures and parser evidence]
    fixtures --> repos[Representative PHP repository tasks]
    repos --> decision{Claim established?}
    decision -->|yes| skill[Version-matched PHP guidance]
    decision -->|no| abstain[Explicit fallback or abstention]
    skill --> navigation[Overview to exact slice workflow]
    abstain --> navigation
```

## Reverse-caller query and decision boundary

```mermaid
flowchart LR
    summary[File summary symbols] --> aliases[Module and symbol aliases]
    aliases --> imports[(Bounded import-relation reads)]
    imports --> calls[(Exact call-target reads)]
    calls --> match[Ambiguity-safe caller matching]
    match --> output[Deterministic bounded called_by]
    baseline[Baseline measures and plans] --> decision{Material winner?}
    candidate[Smallest candidate] --> decision
    decision -->|yes| imports
    decision -->|no| retain[Retain current path]
```

## Graph-construction worker and publication ownership

```mermaid
flowchart LR
    budget[One process indexing budget] --> parse[Symbol parsing]
    budget --> summaries[Structural summaries]
    budget --> relations[Graph derivation]
    parse --> staged[Prepared generation]
    summaries --> staged
    relations --> staged
    staged --> tx[(Short SQLite publication transaction)]
    tx --> current[One current generation]
    cancel[Cancellation or failure] --> cleanup[Discard staging; retain last complete generation]
```

## Filtered custom-harness timeout ownership

```mermaid
sequenceDiagram
    participant Job as Release verify job
    participant Step as Filtered custom harness step
    participant Cargo as Existing cargo test command
    Job->>Step: Start with step timeout
    Step->>Cargo: Run unchanged command
    alt completes in bound
        Cargo-->>Step: Output and exit status
        Step-->>Job: Preserve result
    else exceeds bound
        Step-->>Job: GitHub Actions timeout failure
    end
```

## Entrypoint-profile reachability

```mermaid
flowchart LR
    request[Typed non-persistent profile] --> validate[Validate root, generation, anchors, families, bounds]
    validate --> normalized_graph[(Existing normalized graph)]
    normalized_graph --> traverse[Bounded node-simple reachability]
    traverse --> reachable[Reachable]
    traverse --> candidate[Evidence-backed unreachable candidate]
    traverse --> unknown[Inconclusive: dynamic, unsupported, incomplete, or truncated]
    reachable --> output[Typed bounded analysis result]
    candidate --> output
    unknown --> output
```

## npm runtime selection and integrity

```mermaid
flowchart TB
  npm[npm package] --> tuple[Resolve supported OS/arch tuple]
  tuple --> manifest[Exact release asset + version + SHA-256]
  manifest --> cache[Private cache staging]
  cache --> verify{Digest + archive + executable valid?}
  verify -->|no| reject[Typed failure; no install mutation]
  verify -->|yes| runtime[Verified private runtime]
  runtime --> wrapper[Thin npm wrapper]
  wrapper --> cli[Installed ProjectAtlas CLI]
  wrapper --> mcp[Installed ProjectAtlas MCP server]
```

## Real host configuration consumption

```mermaid
sequenceDiagram
  participant I as Installer
  participant C as Isolated host config root
  participant H as Real host CLI
  participant M as Generated ProjectAtlas MCP config
  participant R as Verified runtime
  I->>C: write host-specific config and plugin/skill state
  H->>C: parse/list configuration through native reader
  H->>M: consume generated MCP entry
  H->>R: start exact installed runtime
  R-->>H: initialize + tools/list + bounded tool call
  H-->>I: isolated success or typed reader/startup failure
```

## Released-main database baseline decision

```mermaid
flowchart LR
    normal[Measure normal init and scan] --> decision{Net product benefit?}
    seed[Measure exact-revision seed] --> decision
    decision -->|no| retain[Retain normal initialization]
    decision -->|yes| verify[Verify digest, revision, schema, runtime]
    verify --> copy[Create private writable project copy]
    copy --> refresh[Reconcile current and dirty source]
    verify -->|invalid| fallback[Typed full-init fallback]
    refresh --> local[(Independent project database)]
```

## Deterministic architecture-community analysis

```mermaid
flowchart LR
    normalized_graph[(Current normalized graph)] --> admit[Resolved local non-containment edges]
    admit --> bound[Node, edge, time, memory, iteration bounds]
    bound -->|complete and within bounds| labels[Stable-order weighted label propagation]
    bound -->|partial coverage or node/edge/intermediate-memory overflow| uncertain[Typed inconclusive or truncated result]
    labels --> ids[Stable parameter-and-member community IDs]
    ids --> result[Bounded returned output: members, evidence, coverage, convergence, truncation]
    uncertain --> result
```

## Bounded PDF and DOCX extraction

```mermaid
flowchart LR
    file[Repository file bytes] --> admit{PDF or DOCX magic and policy?}
    admit -->|no| unsupported[Typed unsupported coverage]
    admit -->|yes| limits[Compressed, expanded, time, memory, entry, recursion limits]
    limits --> parser[Approved in-process Rust parser]
    parser --> evidence[Text plus exact format locator and provenance]
    evidence --> publish[(Atomic indexed-text and graph publication)]
    publish --> navigate[Search, summary, graph, exact evidence]
    limits -->|exceeded or canceled| bounded[Typed bounded failure; no complete claim]
```

## Invalid graph identity admission

```mermaid
flowchart LR
  Parser["Parser facts and exact spans"] --> SourceAdmit{"Source identity admission"}
  SourceAdmit -->|valid| SourceRows["Valid source, symbol, and relation facts"]
  SourceAdmit -->|invalid| SourceReject["Typed source rejection detail<br/>file + span + parser + field + reason"]
  SourceRows --> Derive["Derive resolution keys"]
  Derive -->|valid| KeyRows["Valid resolution-key projection"]
  Derive -->|contract failure| KeyReject["Typed resolution-key rejection detail"]
  SourceRows --> Partial["Partial valid projection + bounded typed detail"]
  SourceReject --> Partial
  KeyRows --> Partial
  KeyReject --> Partial
  Partial --> Tx["One generation transaction"]
  Tx -->|all writes succeed| Current["Publish complete current generation"]
  Tx -->|fault or cancellation| Rollback["Rollback; previous generation remains current"]
```

The coarse `graph_coverage.reason` remains a stable compatibility category;
bounded identity detail is generation-owned in the existing publication
transaction and is read through structured coverage rows. Rejected identity
text is never persisted, and a fault or cancellation leaves both the detail
rows and valid graph rows at the previous complete generation.

## Built-in PHP parser and graph publication

```mermaid
flowchart LR
    php[.php bytes] --> registry[Language capability registry]
    registry --> grammar[Pinned built-in tree-sitter-php]
    grammar --> mapping[PHP node-to-symbol mapping]
    mapping --> exact[Exact symbols, parents, spans, provenance]
    mapping --> relations[Conservative namespace, import, include, call relations]
    mapping --> dynamic[Typed partial coverage for dynamic constructs]
    exact --> published_graph[(Existing graph publication)]
    relations --> published_graph
    dynamic --> published_graph
```

## Bounded document-reason publication

```mermaid
flowchart LR
  Total["Complete resolved + unresolved result"] --> Preflight["Global duplicate and target-compatibility preflight"]
  Preflight --> Chunks["Prepared chunks<br/>each <= GraphLimits::MAX_ROWS"]
  Chunks --> Tx["One savepoint / transaction<br/>one generation"]
  Tx -->|all chunks succeed| Commit["Commit and advertise generation current"]
  Tx -->|fault or cancellation| Rollback["Rollback every chunk<br/>previous generation remains current"]
```

## Canonical project-root identity

```mermaid
flowchart LR
  Input["Native path input"] --> Existing{"Existing addressed root?"}
  Existing -->|no| Missing["Typed missing-index/root result<br/>no mutation"]
  Existing -->|yes| Canon["Filesystem canonicalization"]
  Canon --> Identity["CanonicalProjectRoot<br/>native identity"]
  Identity --> Compare{"Bound database equivalent?"}
  Compare -->|unrelated| Wrong["Typed wrong-root result<br/>no mutation"]
  Compare -->|legacy equivalent| Repair["Atomic metadata reconciliation"]
  Compare -->|exact| Services["CLI / MCP / watcher / worktree / graph / telemetry"]
  Repair --> Services
  Identity --> Persist["Lossless versioned SQLite encoding"]
  Identity --> Display["Terminal UTF-8 display or typed unavailable"]
```

## Rust 1.98.0 toolchain upgrade and verification

Rust 1.93.1 is retained only as historical reproduction evidence. Each intended
stable upgrade is evaluated in its own issue and pull request; the repository
pins a numeric version only after the complete local and hosted gates pass.
Floating `stable` and workflow-local numeric pins are not release inputs.

```mermaid
flowchart LR
  History["Historical reproduction<br/>Rust 1.93.1"] -. evidence only .-> Decision["Issue/PR selects intended stable"]
  Official["Official stable release<br/>Rust 1.98.0"] --> Decision
  Decision --> Pin["rust-toolchain.toml<br/>exact 1.98.0<br/>sole numeric source"]
  Pin --> Preflight{"Expected = actual<br/>rustc / cargo / clippy / rustfmt?"}
  Preflight -->|no| Stop["Fail before expensive or mutating work"]
  Preflight -->|yes| Matrix["Linux + Windows + macOS x64 + macOS arm64<br/>features + parser pack + package + installer"]
  Matrix -->|pass| Artifacts["Reproducible v0.5 artifacts"]
  Matrix -->|fail| Keep["Do not accept a partial pin/release"]
```

## macOS optional-parser capability truth

```mermaid
flowchart LR
  Tuple["OS + architecture + features"] --> Authority{"One typed parser capability authority"}
  Authority -->|accepted Linux/Windows tuple| Contained["Verified pack + containment backend"]
  Contained --> Worker["Bounded worker<br/>limits + cancellation + cleanup"]
  Authority -->|macOS arm64 or unsupported| Unavailable["Typed optional-parser unavailable<br/>no pack mutation or worker start"]
  Unavailable --> BuiltIn["Built-in parser coverage remains usable"]
  Authority --> Report["Installer + runtime + CLI + MCP + tests<br/>same capability truth"]
```

## Native non-UTF-8 worktree identity

```mermaid
flowchart LR
  Native["Path / OsString<br/>root + Git common + Git admin"] --> Identity["Shared native identity from #481"]
  Identity --> Codec["Lossless versioned SQLite codec<br/>native uniqueness"]
  Identity --> Route["Alias routing + capacity + watcher + retirement"]
  Identity --> Process["Native filesystem and Git Command arguments"]
  Identity --> UTF8{"UTF-8 representable?"}
  UTF8 -->|yes| Display["Public path display"]
  UTF8 -->|no| Typed["Stable alias + typed display unavailable"]
```

## Clean macOS Apple Silicon installed lifecycle

```mermaid
sequenceDiagram
  participant Gate as macOS arm64 release gate
  participant Install as Existing installer
  participant CLI as Packaged CLI/runtime
  participant DB as New project SQLite DB
  participant MCP as Generated MCP host config
  participant OS as Worktree/watcher/filesystem
  Gate->>Gate: Isolate HOME, config, cache, project, PATH
  Gate->>Install: Install exact candidate artifact
  Install-->>Gate: Verify path, version, digest
  Gate->>CLI: init, scan, overview, files, summary, slice
  CLI->>DB: Create schema and publish one current generation
  Gate->>MCP: start session and verify project identity
  Gate->>OS: /var alias, worktree, watch, telemetry, symlink docs
  Gate->>CLI: verify built-in parser and typed optional-parser unavailability
  alt wrong root, fault, or cancellation
    CLI-->>Gate: typed failure with no implicit/partial state
  else success
    DB-->>Gate: same identity and complete generation
  end
  Gate->>Gate: Assert cleanup and no ambient-state dependency
```

## macOS all-features reachability

```mermaid
flowchart LR
  Cargo["Cargo target + features"] --> Capability["#483 canonical parser capability"]
  Capability -->|supported optional pack| Backend["Compile contained worker backend"]
  Capability -->|unsupported / macOS| Fallback["Compile typed unavailability + built-in fallback"]
  Backend --> Check["Rust 1.98.0 check + pedantic Clippy<br/>warnings denied"]
  Fallback --> Check
  Check --> Matrix["macOS x64/arm64 + supported Linux/Windows combinations"]
```

## CLI E2E contract ownership split

```mermaid
flowchart TB
    support[Shared process, repo, JSON, platform, package support]
    lifecycle[Lifecycle and database contracts] --> support
    delivery[Installer and release contracts] --> support
    navigation[CLI, MCP, graph, document, language contracts] --> support
    worktrees[Worktree, watcher, freshness, federation contracts] --> support
    maintenance[Purpose, lint, telemetry, TUI contracts] --> support
    ci[CI and release exact selectors] --> lifecycle
    ci --> delivery
    ci --> navigation
    ci --> worktrees
    ci --> maintenance
```

## Codex MCP owner fixture readiness

```mermaid
flowchart TB
    suite[Parallel Windows E2E] --> owner[Spawn compiled Codex owner]
    owner --> child[Start obsolete MCP child]
    child --> publish[Atomically publish PID, start time, and path]
    owner --> poll{Publication and identity readiness within one named 30 s deadline}
    publish --> poll
    poll -->|exit or deadline| fail[Typed failure and owned cleanup]
    poll -->|not ready| pause[Wait 25 ms]
    pause --> poll
    poll -->|published before same deadline| validate{Exact identity valid before same deadline?}
    validate -->|no| fail
    validate -->|yes| installer[Run existing installer handoff assertions]
    installer --> cleanup[Owned parent and child cleanup]
```

## Production module ownership decision

```mermaid
flowchart LR
    callers[CLI, MCP, tests, services] --> map[Call, state, data, error, transaction map]
    map --> decision{Independent durable owner proven?}
    decision -->|no| retain[Retain current module with evidence]
    decision -->|yes| move[Move cohesive responsibility]
    move --> facade[Preserve owning public re-export]
    facade --> tests[Compatibility, SQLite, fault, concurrency, E2E proof]
    db[(One schema and transaction authority)] --> move
```

### Durable ownership map

This map follows the current callers, state, and publication boundaries. It is a responsibility decision, not a line-count partition, and it preserves the seven-crate workspace. Dependency direction remains CLI adapter -> runtime/service -> database/core; database internals stay private while the database root retains the compatibility re-exports used by callers.

| Owner | Callers, state, and data | SQLite, concurrency, cancellation, and errors | Tests and hot path | Decision and rejected splits |
| --- | --- | --- | --- | --- |
| `crates/projectatlas-cli/src/mcp.rs` | `main.rs::run` starts `run_mcp_server`; RMCP invokes `ProjectAtlasMcpServer` through its tool router. The module owns MCP parameter/response schemas, route dispatch, selected-project state, usage lifecycle, source observations, and server lifecycle. | It opens the root-bound `AtlasStore` and delegates SQL/publication to the database crate. `Arc<RwLock<_>>` protects selected project/task state, `Arc<Mutex<_>>` protects bounded telemetry, and the background envelope bounds aggregate work. The request cancellation bridge propagates cancellation and joins its monitor on drop. Runtime, service, and database errors are converted at the RMCP boundary without changing their typed meaning. | `mcp.rs::tests` and CLI/MCP route smoke exercise the adapter. The hot path is request decode/validation, bounded runtime/service/database read, and response serialization; task status/cancel uses the same session state. | Retain the protocol adapter and all route state here. The only accepted move is private `mcp/task_registry.rs`: its bounded session-local records have one lifecycle, no database or wire ownership, and no independent external caller. Splitting DTOs, routes, telemetry, root selection, or cancellation would sever shared request state and add a facade without a durable owner. |
| `crates/projectatlas-cli/src/runtime.rs` | `main.rs` and `mcp.rs` call the shared runtime. It owns `ScanRuntimePlan`, init/scan/watch orchestration, freshness/read status, symbol-build options/reports, settings/lint/telemetry orchestration, and the stage transitions shared by CLI and MCP. | It does not own SQL or commit publication: it passes `IndexWorkControl` through bounded filesystem/parser/service stages and asks `AtlasStore` to begin/complete caller-owned transactions. `CliError` preserves filesystem, database, service, cancellation, and resource failures. | Runtime tests plus submodule tests cover the shared path. Scan/watch freshness, staging, and symbol projection are the hot paths. | Retain the orchestration module and its existing private submodules. `graph_projection.rs` remains coupled to runtime stages and graph publication; `module_resolution.rs` remains a bounded compiler-config helper consumed by that projection; feature-gated `optional_parser_runtime.rs` remains coupled to parser work types and resource admission; `source_observation.rs` remains the runtime freshness registry and its `pub(crate)` re-export. Splitting by phase, report, or feature would duplicate cancellation, bounds, and publication state. |
| `crates/projectatlas-db/src/lib.rs` | CLI runtime/MCP, service, and snapshot callers use the `AtlasStore` façade. It owns the SQLite connection, read-snapshot state, database path/location, validated native project binding, direct-library telemetry instances, and compatibility re-exports. | `schema.rs` owns schema/migrations and `lib.rs` owns the store/guard lifetimes: immediate publication and purpose transactions, read snapshots, binding validation, commit/rollback, and ancillary telemetry connections. SQLite read/write serialization and short lock scopes remain here; `DbError` propagates storage, binding, corruption, and rollback failures. | `projectatlas-db` unit/integration tests cover these guards and public methods; every indexed read/publication is a hot path. | Retain `AtlasStore`, `IndexPublicationGuard`, `PurposeMutationTransaction`, and the public re-export façade. No submodule split is accepted: `content_classification.rs` (classification rows), `derived_snapshot.rs` (portable snapshots), `diagnostics.rs` (bounded reports), `hydration.rs` (backup hydration), `project_identity.rs` (root binding), `repository_graph.rs` (graph SQL), `schema.rs` (schema authority), `sqlite_profile.rs` (connection profile), `telemetry.rs` (usage persistence), and `worktree_registry.rs` (registry persistence) each remain behind the existing façade because their APIs borrow its connection/guards or share its binding and transaction invariants. |
| `crates/projectatlas-db/src/repository_graph.rs` | `AtlasStore` methods are called by runtime, service, snapshot, and navigation paths. The module owns normalized graph entities, relations, occurrences, coverage, resolution-key rows, graph staging, bounded hydration/read pages, and graph-specific row reconstruction. Its public graph types remain re-exported by `projectatlas-db/src/lib.rs`. | It owns graph SQL, prepared statements, graph read budgets, and staging helpers, while `AtlasStore` owns the live connection and outer commit/rollback guard. `IndexWorkControl` is checked during bounded reads and staging; failures return typed `DbError`/graph-contract errors and never advertise partial rows. | Repository-graph tests and database/service integration tests cover publication/navigation; graph staging, relation hydration, and bounded navigation are hot paths. | Retain one graph owner. Moving navigation, staging, row decoding, or query families would split shared keys/limits/schema and `AtlasStore` lifetimes, duplicate SQL authority, and obscure transaction/cancellation/error behavior. The existing `repository_graph.rs` boundary is the smallest durable owner. |

No schema, index, query, transaction, migration, or database-authority change is part of this amendment. The existing `schema.rs` and `AtlasStore` boundaries remain authoritative; the map documents why the private task registry is the only accepted move and why all other proposed splits are no-change decisions.

## Benchmark artifact retention boundary

```mermaid
flowchart LR
    run[Benchmark run] --> classify{Compact durable evidence?}
    classify -->|yes| source[Sanitized summary or bounded result in source]
    classify -->|large raw trace| local[Ignored local output]
    classify -->|release evidence| release[Release or external artifact]
    gate[Deterministic tracked-file policy] --> source
    gate --> reject[Reject accidental oversized raw source artifact]
```

## Repeatable real-task agent evaluation

```mermaid
flowchart LR
    prereg[Versioned preregistration] --> baseline[Baseline arm]
    prereg --> atlas[ProjectAtlas arm]
    baseline --> retain[Retain success, failure, timeout, uncertainty]
    atlas --> retain
    retain --> sanitize[Redact private paths/content and bound artifacts]
    sanitize --> metrics[Success, time, wrong-file reads, context, tool bytes]
    metrics --> report[Compact observed comparison; modeled claims remain separate]
```

## atlas shim lifecycle and command compatibility

```mermaid
flowchart TB
  installer[Installer] --> collision{Existing atlas command?}
  collision -->|unmanaged| reject[Typed collision; no overwrite]
  collision -->|managed| shim[Atomic managed shim install]
  shim --> discover[PATH discovery]
  discover --> aliases[atlas top-level command aliases]
  aliases --> canonical[Canonical projectatlas command handlers]
  aliases --> resolve[atlas health resolve]
  aliases --> legacy[atlas health-check remains compatible]
  shim --> uninstall[Managed uninstall/repair]
  uninstall --> clean[Remove only managed artifact]
```

## v0.5.0 candidate, readback, remediation, and stable promotion

```mermaid
stateDiagram-v2
  [*] --> PublishedIssueReadback: read exact main OpenSpec and architecture targets
  PublishedIssueReadback --> PublicationRepair: mapped task, document, heading, or Mermaid is missing or stale
  PublicationRepair --> PublishedIssueReadback: planning PR publishes corrected evidence
  PublishedIssueReadback --> ExactRevision: published milestone gate and every required review pass
  ExactRevision --> SurfaceInventory: freeze complete CLI and MCP inventory
  SurfaceInventory --> InstalledProof: safely execute every supported route
  InstalledProof --> CandidateBuild: package exact main revision
  CandidateBuild --> UpdateProof: update exercised v0.4.5 installation and database
  UpdateProof --> RC1: state, migration, failure, retry, and rollback hard gate passes
  UpdateProof --> Remediation: update or migration blocker
  RC1 --> HostedReadback: independently verify tag, assets, runtime, and Latest
  HostedReadback --> Remediation: confirmed blocker
  Remediation --> PublishedIssueReadback: return defect to owning child issue and restart proof
  HostedReadback --> StableBuild: accepted candidate and no blocker
  StableBuild --> StableReadback: repeat installs and hosted identity
  StableReadback --> FinalState: v0.5.0 is Latest with hierarchy, issues, milestone, and workflows verified
  FinalState --> [*]
```
