## Context

ProjectAtlas is an agent tool. Its successful 0.3.26 contract is the atlas-first funnel:

```text
overview -> folders -> files -> summary/outline/symbols -> slice
```

Version 0.4 must improve what an agent learns from that funnel without adding mandatory orchestration. The selected local source tree on disk is the indexed truth, including uncommitted edits and non-Git projects; VCS state is optional context, never a substitute for current bytes. SQLite remains the project-local index source of truth, TOON remains the compact default agent format, `.gitignore` remains dynamically authoritative, and authored purposes/settings/telemetry remain outside disposable derived data.

The earlier implementation branch mixed useful graph/storage work with a per-task proof system. Recovery therefore happens commit by commit: reapply behavior that still fits this design, independently verify it on current `dev`, reject the proof machinery, and delete the old branches after their useful source has been taken over.

### Source recovery decisions

The retained #308 branches and dirty worktrees were reviewed against current `dev`. Current schema preflight, migration, full-publication, generation, WAL, concurrency, and local-read refusal behavior supersede the old schema-16, physical-slot, copied-staging-database, backup-lease, and immutable-read implementations; those mechanisms are rejected rather than replayed.

Useful responsibilities are owned once by the functional tasks that implement them:

- tasks 2.1 and 2.6 own a smaller typed graph identity/selector model and explicit project move/copy/detach behavior;
- tasks 2.4 and 2.5 own affected-path incremental publication plus normalized indexed graph persistence and queries on the current schema owner;
- tasks 4.1, 4.2, and 4.5 own one lean language/parser capability authority and generated settings/documentation;
- tasks 5.5 through 5.7 own reusable target selectors, bounded traversal, and generation-bound cursors through existing relation services;
- task 6.1 owns the accepted relation-family inventory.

The old hard-coded three-hop query, graph-scale/evidence runners, task receipts, SHA-bound fixtures, per-task test identities, binary schema-history chain, optional parser-pack lifecycle without a selected consumer, and evidence-specific workflows are intentionally not recovered. Seven responsibility-owned crates remain sufficient. The dirty repository root is preserved only because it also contains unfinished #314 Memory Atlas source; it is not an authority for #308 behavior.

## Goals

- Reach correct source context faster than 0.3.26 with the same normal command/tool vocabulary.
- Reduce wrong selections, redundant calls, broad file reads, backtracking, output bytes, and total context tokens.
- Preserve purpose-led folder and file selection as the architectural-responsibility layer and make graph context improve it rather than displace it.
- Keep graph and language truth typed, deterministic, bounded, and fresh after edits.
- Keep scan, incremental refresh, SQLite persistence/query, graph traversal, and MCP output responsive on huge local source trees through indexed access, bounded memory, efficient Rust data ownership, and host-aware parallelism.
- Preserve every existing valid CLI/MCP request and default unless an explicit delta requirement says otherwise.
- Keep optional breadth and semantic capabilities removable and absent from default-core cost when unused.
- Use ordinary behavior tests and locked Rust/workspace gates, not a second evidence system.

## Non-Goals

- No generic graph UI, mutable ADR store, custom Cypher language, or required semantic model.
- No runtime execution-trace ingestion; #308 trace views are bounded static paths over indexed source facts.
- No execution of repository code, build hooks, compilers, language servers, or downloaded parsers during normal indexing.
- No hidden global repository cache, implicit cross-root discovery, or cross-project write.
- No mechanical translation or mirrored source topology from another implementation.
- No per-task test identifiers, verification ledgers, SHA receipts, managed evidence comments, issue sealing, or between-slice mutation/coverage campaign. One combined final changed-surface line-coverage and mutation audit runs only after the complete issue behavior stabilizes.

## Dependency Order

1. Restore the lean workflow and dependency safety through issues #309 and #316.
2. Normalize this OpenSpec and recover only useful product source from the old #308 branches.
3. Complete database architecture/reconciliation task 2.0, reopened short-transaction staging task 2.3, real SQLite/MCP contract task 2.7, and bounded storage-lifecycle task 2.8 before task 3.2 or dependent graph navigation; finish dependency-aware convergence and the verified source-observation epoch in tasks 3.2, 3.3, and 3.5 before graph signals enter normal navigation; at each later task, land its smallest database delta and real round-trip coverage before service/adapter code.
4. Stabilize language and relation contracts before richer analysis or optional packs.
5. Complete issue #308 before bounded project memory in #314 consumes the database/session boundaries.
6. Run combined compatibility, packaging, platform, benchmark, and release readiness in #311 after #308 and #314 stabilize.

## Decisions

### 1. Preserve the atlas-first public workflow

Existing scan, watch, overview, folder, file, search, summary, relation, outline, and slice calls remain the normal route. Graph extraction and freshness are automatic behind scan/watch. Default responses add only bounded relationship counts, high-value related identities, coverage, ranking reasons, and next-call hints. Full graph detail remains opt-in.

Reviewed `folder_purpose` and high-impact `file_purpose` remain first-class intent signals. Folder purposes are curated broadly; file purposes remain selective. Generated suggestions are not treated as reviewed truth, and meaningful responsibility changes make reviewed purposes stale rather than silently replacing them. Exact path/name and strong purpose matches remain ahead of weaker graph popularity signals.

Packaged agent guidance launches a low-reasoning purpose curator alongside the main task at startup and relevant task/source transitions when the host supports isolated bounded subagents. The curator obtains task-scoped missing/stale/suggested rows directly from the purpose queue, then uses bounded summary, graph-role, outline, or exact-slice context and writes only through purpose APIs. Project/generation/path ownership coalesces duplicate attempts.

Successful low-scope curation is silent in the main conversation and normal session/folder/file responses: no per-path progress, approval, or completion prose is rendered. Later navigation simply consumes the improved approved purposes. If the host requires a terminal result, the curator returns only a minimal machine-facing state. Task-relevant conflicts that would make ranking unsafe and repeated degraded/failure state remain available through compact blockers or explicit health/settings diagnostics.

`low` remains the default background scope: folders plus task-relevant/high-impact files. `medium` intentionally requires every source file to be reviewed. `strict` intentionally requires every indexed file and folder and is never started implicitly. A host without isolated subagent support keeps the task-scoped queue available and does not claim invisible background work; normal source answers remain available.

Architecture, impact, and trace begin as closed typed views of the existing bounded relation/summary/health services. At most one optional `atlas_analyze`-style surface may be added if real agent tasks prove that extending the relation request makes tool selection or schema size worse. Analysis is never a prerequisite for normal navigation, and complexity/bottleneck candidates do not receive a separate tool.

### 2. Use typed graph contracts with one owner

`projectatlas-core` owns closed enums and validated newtypes for entity identity, relation family, resolution, confidence, coverage, generation, optional selected-slot detail when justified, and limits. `projectatlas-service` owns use-case requests, reports, ranking, traversal, and cursors. `projectatlas-db` owns their SQLite representation.

Legacy relation values remain compatible. New graph-only relation families use an additive typed field rather than expanding the old exhaustive enum. Generic JSON property bags and duplicated schema strings are rejected.

One versioned accepted relation-family inventory is the compatibility authority for direct structural/type, package, test, route/protocol, configuration/environment, deployment/infrastructure, and bounded static read/write families plus separately gated inferred similarity/co-change families. Persistence, invalidation, settings, queries, fixtures, and generated documentation derive from it. Removing or weakening an accepted row requires an explicit compatibility decision and inventory-version change; an implementation cannot pass by silently ceasing to advertise the family. Counts are derived from the inventory rather than duplicated in Rust or tests.

### 3. Publish structural data atomically

A full scan builds derived data away from the currently queryable generation, validates it, rechecks source/configuration freshness, and publishes one complete new generation through one parent-owned transaction. The last valid generation stays queryable while work runs and remains current on failure. Physical slots or retained rollback generations are allowed only when a measured recovery or concurrency requirement justifies their storage and migration cost.

An incremental refresh computes one affected closure and applies its deletes/inserts/updates in one transaction, advancing the generation exactly once. A failed transaction exposes neither partial rows nor a new generation. Authored data is never owned by disposable derived publication state.

### 4. Keep database architecture explicit and workload-fit

SQLite remains the selected engine because the product contract is exactly one authoritative project-local, offline, cross-platform database per resolved runtime/request source-state binding, with many bounded read snapshots and one atomic publication writer. Normal root, `project_path`, and nearest-project discovery select `<root>/.projectatlas/projectatlas.db`; explicit startup `--db` remains a compatible isolated binding for generated host configuration, tests, migration/verification, and protected runtime lanes, but it is never auto-discovered, attached, merged, substituted, used as a fallback, or combined with another database in one read, publication, authored mutation, telemetry write, or result. Purposes, future Memory Atlas records, graph/index projections, settings, health resolutions, and telemetry for that binding are not split into separate product databases. Bundled SQLite through workspace-owned `rusqlite` avoids a daemon, service account, network dependency, separate graph server, and platform-specific installation while providing transactions, foreign keys, B-tree adjacency indexes, query planning, backup support, WAL readers, FTS when justified, and direct Rust ownership. A graph-shaped workload does not by itself justify a graph database: ProjectAtlas asks bounded folder/file/neighbor/path/impact questions and rebuilds derived state from current local bytes rather than running arbitrary distributed graph computation. A large admitted extraction may spill to one bounded disposable working file, implemented as a plain file or SQLite-backed spool, but that implementation detail is not a ProjectAtlas database: it has no project identity, authored state, active generation, or query surface and is removed after publication, cancellation, failure, or validated restart cleanup.

The authority model is deliberate. Current saved source bytes remain outside SQLite and authoritative for what exists now. SQLite owns project identity/database metadata, reviewed purposes, health resolutions, and bounded usage telemetry as durable atlas state. Filesystem configuration, `.gitignore`, stricter atlas ignore policy, and optional VCS context remain external policy/freshness inputs rather than duplicated database authority. Nodes, text, summaries, symbols, logical graph relations, exact occurrences, coverage, and generations are derived projections that can be atomically rebuilt without deleting authored state. Purpose is joined/projected from its owning node at query time; graph rows never become a second mutable purpose authority. Future bounded Memory Atlas state may share the same database owner only through its own authored lifecycle, caps, replacement policy, and migration contract after #308 stabilizes.

The logical graph remains relational and normalized: stable project-scoped entities; one deduplicated logical source-kind-target relation; zero or more exact source occurrences; typed ambiguous/unresolved facts; coverage; and one complete publication generation. The physical model uses typed columns and integrity checks for hot predicates, fixed-width stable-key blobs with collision witnesses, source and target adjacency indexes, path/manifest/source-span/coverage indexes, cached prepared statements, batched caller-owned writes, and `LIMIT + 1` bounded reads. Full and affected-closure publication occurs in one parent transaction; read snapshots see only a complete generation. WAL, busy handling, checkpoints, statistics, database growth, and local-filesystem locking are part of the operating profile rather than universal tuning constants.

The project database also has a bounded lifecycle. Recent raw usage events are retained only within declared row/age/byte budgets; older measurements are compacted into bounded typed aggregates that preserve supported all-time totals and declared trend windows while reporting when session-level raw detail has expired. Obsolete derived rows, abandoned owned staging artifacts, WAL growth, free pages, and planner statistics have explicit cleanup or maintenance owners. Normal agent request paths never run an unbounded purge, blocking truncate checkpoint, or blind `VACUUM`. Settings expose only content-free retention and maintenance state, and a limit that cannot be serviced safely returns typed maintenance or rebuild guidance rather than silently deleting authored state or allowing monotonic growth.

The engine decision is reopened only when a required product contract needs shared remote multi-writer state, a live network filesystem unsupported by SQLite, unbounded/distributed graph computation, or still fails preregistered huge-source query/publication/resource thresholds after the owning schema, index, query, statistics, and transaction design has been corrected. Specialized search or ANN storage remains an optional derived accelerator, never a replacement for authoritative atlas state or structural correctness. An abstraction without a second proven implementation is rejected; `projectatlas-db` remains the concrete storage owner inside the seven-crate architecture.

### 5. Fix freshness before richer navigation

The first normal index-backed read after process start performs exact freshness verification against persisted local file and source-state fingerprints. A long-lived MCP/watch runtime activates root/policy observation before or during that verification, reconciles buffered events, and establishes a process-scoped verified source-observation epoch. Later unchanged reads reuse that epoch and do not repeat a whole-tree walk or full node-table load while observation remains continuous and healthy. A bounded safe delta is reconciled before answering; otherwise the call returns a compact typed `refresh_required` state instead of silently serving known-stale facts. This applies after process restart, in dirty worktrees, and in non-Git source directories. The watcher remains a latency and continuity mechanism, not a substitute for the initial exact verification.

Every indexed result binds its database generation to the verified source epoch. A relevant event, observer overflow/gap/disconnect, policy/root uncertainty, cancellation, or source change during a query invalidates the epoch and requires reconciliation, exact re-verification, retry, or `refresh_required`. A separate one-shot CLI process performs its own first exact verification and never trusts an in-memory epoch owned by another process.

Watch and one-shot refresh use the same current-content, ignore/configuration, parser/capability, and optional VCS-context contract. Local bytes and paths remain authoritative. True deletes are distinguished from transient access or root uncertainty. Exported identity changes invalidate inbound dependents. An unchanged dirty state coalesces without repeated publication.

For the same final repository state, incremental refresh must converge to the same canonical structural graph, coverage, and lexical results as a clean full scan. Source changes observed during full staging prevent publication or make the new generation observably stale for bounded follow-up.

### 6. Generate honest language capability truth

One versioned registry owns detection order, aliases, exact filenames, compound extensions, content/dialect rules, parser ownership, optional pack ownership, fixtures, and support tiers. Generated Rust tables, settings, tests, and documentation derive from it.

The registry also owns an explicit accepted capability-set manifest. Every accepted row declares its required membership, tier, natural fixtures, provenance/license inputs, and required platforms. Removing or weakening an accepted row requires an explicit compatibility decision and capability-set version change; generated validation refuses silent shrinkage and derives all counts instead of hardcoding them in product code or tests.

Current built-in parsers remain closed compile-time choices. New language breadth must pass non-vacuous fixtures and platform checks before it is advertised. Language-specific semantic providers stay small and independently gated; ambiguity and external targets are explicit rather than guessed. Optional parser packs bind a pinned provenance, digest, license, and ABI to their capability rows and run offline during normal use behind a supervised out-of-process or capability-denied WASM/native boundary. They cannot execute repository code, and hard time, memory, output, and cancellation limits prevent pack failure from harming the MCP process or active generation.

Where translation-unit languages require compiler metadata for correct resolution, ProjectAtlas accepts only typed bounded data such as working directory, include roots, dialect/target, and opaque define identity. It never executes compilers, shells, response-file commands, builds, or repository code, and it never persists or emits secret-bearing raw values.

### 7. Keep lexical correctness authoritative

Deterministic literal, regex, fuzzy, case, context, ordering, pagination, punctuation, short-string, and Unicode behavior remains available without FTS or a model. FTS may narrow only query shapes for which it can produce a complete candidate superset, and exact matching verifies candidates. Unsafe or unsupported query shapes use a correctness-authoritative persisted-text scan bounded by selected paths, inspected bytes, time, cancellation, and output, with searched-file/searched-byte/truncation state. The design does not claim repository-size-independent indexed complexity for arbitrary regex or fuzzy semantics.

Optional semantic retrieval has an explicit install/enable/build/ready/stale/update/rollback/disable/remove lifecycle. Explicit `semantic` or `hybrid` search returns a typed capability error unless a compatible ready generation exists; omitted mode remains lexical. Hybrid ranking preserves lexical completeness, exposes bounded score reasons, and is enabled only after labeled retrieval-quality and resource checks. The pack never downloads implicitly, never blocks structural publication, and is not linked or initialized by default core when absent.

### 8. Bound every analysis and cursor

Ranking keeps exact path/name and strong purpose matches ahead of weaker graph signals and exposes compact deterministic reasons. Coverage discovery, relations, architecture, language-valid complexity/bottleneck candidates, VCS-aware impact, and static trace have hard row, depth, visited-node, edge, time, memory, output, and cancellation budgets. Complexity and dead-code results remain labeled candidates, never proof. VCS input uses a maintained Git crate or shell-free argument-vector process boundary and never mutates, replaces local bytes, or implicitly scans the source tree. Static trace paths are node-simple and use indexed source relationships; runtime trace ingestion is not part of #308.

Opaque cursors bind project/root identity, active generation, query, filters, ordering version, capability digest, and membership-affecting budgets. Any binding change returns a typed stale/mismatch error instead of mixing generations.

Output shape follows the agent task rather than forcing one renderer everywhere. Uniform purpose, candidate, coverage, and relation rows remain compact TOON by default; exact slices remain verbatim source; graph overview/path results use bounded typed aggregates plus node/edge/path records that preserve topology. JSON compatibility remains available where already supported. Every paged section reports returned, truncated, continuation, and a typed `total_state` of exact, at-least, or unknown. An exact total is returned only when already known, proved by the bounded page, or computable within the same statement, row, time, and cancellation budget; a high-degree graph page does not scan its full adjacency merely to decorate a bounded result.

### 9. Federate only explicit read-only call roots

A federated analysis call supplies its complete ordered root set. Every root must already have a valid index and is opened read-only/query-only under hard root-count, simultaneously-open-snapshot, aggregate row/edge/input/intermediate/output byte, time, and cancellation-to-close budgets. One invalid, stale, corrupt, or mismatched participant fails the whole call and closes all handles. Cross-root relationships are computed in bounded call memory and discarded afterward; no roots, edges, cache, telemetry, settings, or active-project mutation is retained.

### 10. Prefer the smallest Rust-native mechanism

Closed variants use enums and exhaustive matching. Stable identities use validated newtypes. Concrete modules own one implementation. Traits or dynamic dispatch require a real runtime extension boundary or multiple implementations. Bounded owned messages are used only where worker isolation requires them. SQLite transactions own publication atomicity; no actor, overlay chain, unconditional multi-slot framework, raw page writer, or generic command framework is added.

Canonical maintained crates are preferred for parsing, Git data, storage, serialization, and platform containment. New dependencies remain workspace-centralized and must justify their default features, security, maintenance, compile/package, and runtime cost.

### 11. Treat large-source performance as an architecture contract

Performance is reviewed at every non-trivial Rust slice rather than deferred to one final benchmark. Full indexing remains linear in included entries and bytes plus emitted facts. Normal bounded reads and graph queries use prepared indexed access and must not materialize or scan the whole repository graph. Incremental refresh work is driven by the changed paths plus a bounded affected dependency closure; it escalates explicitly instead of silently truncating or degenerating into repeated full rebuilds. SQLite writes are batched under the existing atomic publication owner, with query-plan, transaction-duration, WAL/write-amplification, and persistent-size risks reviewed at the storage boundary.

The SQLite physical model serves the actual repository-graph workload: project-scoped stable entities, deduplicated logical relationships, separate exact source occurrences, generation/coverage rows, and indexed outbound plus inbound adjacency. Index order and covering choices follow bounded folder/file/relation/impact and invalidation queries, including skewed high-degree nodes. Reviewed purpose remains authored once in its purpose owner and is projected at query time rather than copied into entity or edge rows. A bounded query never reconstructs or decodes the whole graph, and an incremental identity change uses old/new keys plus indexed inbound dependents to publish one complete affected closure.

Rust ownership and concurrency choices are part of the same contract. Hot paths avoid unnecessary owned conversions, clones, per-edge JSON, and unbounded intermediate collections. Filesystem and database I/O are batched where correctness permits. CPU parallelism derives from one effective host/process envelope, prevents cross-task oversubscription, keeps locks short, and preserves cancellation, backpressure, and service responsiveness. Representative scale runs separately measure full scan, incremental refresh, bounded query latency, startup, process-tree memory, database/WAL/staging writes, persistent bytes, worker utilization, and output size. Code review and static lints establish structure; query-plan assertions, benchmarks, and profiles support performance claims.

### 12. Make agent improvement observable

The issue cannot close on feature count alone. Repeated representative tasks compare 0.3.26 with the current candidate for:

- answer/source-selection correctness;
- calls and elapsed time to first useful context;
- wrong or redundant selections and backtracking;
- full-file reads and broad-read escapes;
- total tool calls, emitted bytes, and conservative context tokens;
- usefulness and correctness of next-call guidance;
- compatibility of the normal overview-to-slice workflow.

Each normal workflow must use the same or fewer mandatory calls and must not regress file reads or total context. Extra bounded context is acceptable only when it avoids at least as much later context and improves the task result. Comparative public claims remain deferred to validated release results in #311.

The final main-agent acceptance check uses the candidate as the first navigation tool for real local-source tasks and verifies this combined contract:

```text
live local source state tells me what exists now
reviewed folder/file purpose tells me where and why
crisp graph context tells me how each candidate participates
content summary verifies what is currently in the selected file
parser and coverage state tell me how much to trust
slice gives me the exact source
```

The candidate fails acceptance if the graph is stale, noisy, unbounded, purpose-displacing, repository-commit-centric, or reachable only through extra expert calls. It passes only when the normal streamlined MCP funnel selects correct source with fewer or equal calls, full-file reads, backtracking, emitted bytes, and total context than 0.3.26 and meets the stronger predefined agent-navigation targets.

The durable agent-facing contract is documented in `docs/agent-navigation.md`. It is a target contract until the combined behavior passes task 7.3 and must be reconciled against the implemented candidate before issue closure.

## Verification Model

A coherent behavior slice gets the smallest meaningful owning test plus any integration, real CLI/MCP smoke, fault, concurrency, affected-platform E2E, query-plan assertion, benchmark, or profile required by its actual risk. Deterministic logic is proved at the owning unit or crate boundary; database/publication and multi-crate behavior at integration boundaries; public adapter contracts through real smoke paths; watcher, installer, recovery, concurrency, cross-platform, and packaged agent workflows through affected E2E paths. One shared test may cover several tasks, and duplicate coverage at every layer is not required when the behavior and adapter boundary are already proved. Documentation, generated data, and policy changes use their natural validators.

Significant compiling slices pass:

```text
cargo fmt --all --check
cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
cargo test --doc --all-features --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --locked
```

Repository source lints, dependency policy, OpenSpec validation, and IssueOps synchronization remain ordinary gates. Exact release packaging/platform/benchmark gates belong to #311 after the combined surface stabilizes.

Only after every #308 functional task and the combined agent workflow stabilize, task 7.4 runs one final line-coverage audit toward near-100% coverage of the changed functional Rust surface plus mutation testing of that final surface. Every uncovered or surviving behavior-relevant branch is fixed with a meaningful test or explicitly justified as unreachable/platform-only/generated. These final audits are not run between slice commits and do not create per-task receipts or evidence ledgers.

## Migration And Rollback

- Read-only schema/root/integrity preflight occurs before any migration write.
- Supported migrations run transactionally and preserve project identity plus authored data.
- Unknown future schemas, ledger mismatches, failed checks, or insufficient resources fail without destructive rebuild.
- Full publication failure leaves the active generation unchanged; incremental failure rolls back the delta and generation.
- Snapshot export, if enabled, uses a private consistent backup only as input to a freshly constructed allowlisted derived artifact; authored/private rows and free-page remnants never enter the distributable archive. Import validates that temporary artifact and publishes derived data through the normal atomic generation path rather than replacing the live authored database.

## Acceptance

- Existing CLI/MCP inventory and normal workflow remain compatible.
- Freshness after edits is reliable and incremental output equals clean-scan output.
- Current saved local source state, including dirty worktrees and non-Git directories, remains authoritative over optional VCS context.
- Purpose-led navigation remains first-class and gains graph-aware ranking without making purpose curation exhaustive or mandatory for every file.
- Graph identities, relations, coverage, publication, and queries are typed, deterministic, indexed, and bounded.
- Advertised language and relation capabilities are derived from validated registries and fixtures.
- Default-core startup, package, and runtime behavior do not depend on optional packs.
- Representative agent tasks are better than 0.3.26 in practical navigation and do not regress calls, reads, or context.
- Old #308 branches are deleted after useful product source has been independently reapplied and verified.
