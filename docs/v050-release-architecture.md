# v0.5.0 release architecture

Each mapped v0.5.0 issue owns one focused view below. The release graph in `openspec/issue-map.json` owns hierarchy and implementation order; #492 owns acceptance only and closes after every child issue.

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
    bound -->|partial coverage or node/edge overflow| uncertain[Typed inconclusive or truncated result]
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
  Source["Parsed file and exact spans"] --> Admit{"Source identity admission"}
  Admit -->|valid| Strict["GraphIdentityText remains strict"]
  Admit -->|invalid| Coverage["Typed coverage/unresolved row<br/>file + span + parser + field + reason"]
  Strict --> Rows["Valid symbol/relation rows"]
  Coverage --> Tx["One generation transaction"]
  Rows --> Tx
  Tx -->|all writes succeed| Current["Publish complete current generation"]
  Tx -->|fault or cancellation| Rollback["Rollback; previous generation remains current"]
```

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
