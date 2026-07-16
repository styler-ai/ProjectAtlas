## Context

ProjectAtlas 3 is already a Rust-native, atlas-first repository intelligence system. Its durable source of truth is project-local SQLite; its normal agent funnel is overview -> folders -> files -> summary/outline/symbols -> slice; its agent output is TOON-first; its MCP calls are project-root scoped; and purpose metadata plus token telemetry are ProjectAtlas-owned data rather than disposable parser output. This program expands the intelligence behind those surfaces instead of replacing them with a generic graph product.

The current implementation has six useful ownership boundaries:

| Boundary | Current responsibility | Important current limitation |
|---|---|---|
| `projectatlas-core` | Domain types, language detection, symbol and relation enums, summaries, health, telemetry | Symbols have a small relation vocabulary and no durable cross-file identity/resolution state. |
| `projectatlas-fs` | Parallel repository discovery, dynamic `.gitignore`, stricter atlas excludes, BLAKE3 file identity | Change discovery exists, but graph invalidation is not yet dependency aware. |
| `projectatlas-symbols` | Tree-sitter, manifest, structural, and fallback extraction plus language augmenters | The parser list is hand-wired and most relations are file-local. |
| `projectatlas-db` | SQLite schema, files/folders, symbols, text, purposes, health, telemetry | Source candidate search uses `instr(...)`; symbol IDs are transient database integers. |
| `projectatlas-service` | Ranking, search, summaries, slices, relations | It cannot yet express stable multi-hop impact/architecture analysis. |
| `projectatlas-cli` | CLI, MCP, scan/watch/runtime composition, task progress, installer surfaces | Several modules and E2E files are now large enough to split by ownership before adding this scope. |

The current watcher already uses filesystem notifications, debounce, polling fallback, content hashes, and per-path refresh. The current ignore behavior dynamically inherits `.gitignore`. Both remain authoritative foundations for this program.

### Capability Baseline

Phase 0 SHALL maintain a neutral, version-controlled inventory of repository-intelligence behaviors, failure modes, parser capabilities, supply-chain evidence, and benchmark questions. The inventory informs the accepted capability set but is not itself a feature-count target. Every accepted capability needs a ProjectAtlas owner, tier, producer, consumer, evidence contract, test corpus, resource budget, and release decision; deliberate exclusions remain visible with rationale.

Generated grammar artifacts are treated as dependencies with pinned provenance, digest, ABI, license, fixtures, and SBOM entries rather than as ProjectAtlas-owned source. Third-party correctness or performance claims receive no ProjectAtlas credit until reproduced by the preregistered ProjectAtlas harness on pinned inputs.

### Repository-Intelligence Pattern Inventory

```text
process/bootstrap
  -> MCP, config, watcher, optional UI
  -> supervised same-binary indexing child
  -> discovery + ignore evaluation
  -> parallel tree-sitter extraction into worker-local buffers
  -> project registry + module-to-definition candidate index
  -> per-file and cross-file semantic resolution
  -> package, route, config, test, history, similarity, service, and IaC passes
  -> whole in-memory generic graph
  -> SQLite + FTS5 + vector persistence
  -> MCP search, query, trace, schema, architecture, project, ADR, and trace tools

Side paths:
  Git-state polling watcher
  compressed shared graph artifact
  cache-discovered cross-repository linker
  optional localhost 3D graph UI
  multi-agent installer/config updater
```

### Architecture Disposition Ledger

| Pattern | Behavior | ProjectAtlas disposition |
|---|---|---|
| Bootstrap/allocator/watchdog | Initializes allocator, UTF-8 process state, logging, memory limits, MCP/watcher/UI | Retain Rust process/runtime ownership; adopt only measurable startup/resource diagnostics. |
| Index supervisor/recovery | Runs indexing in a child, detects no progress, retries with stable suspect-file quarantine | **Adopt behavior, redesign protocol** with a typed supervised Rust worker that cannot activate the database. |
| Discovery/ignore | Supports nested/global Git ignores and a custom ignore engine plus hardcoded exclusions | **Retain ProjectAtlas** dynamic `.gitignore` and stricter atlas-only ignores; do not import hardcoded product invariants. |
| Language detection | Filename, compound extension, extension, content/dialect, semantic YAML modes, override | **Adopt layered contract** through a generated typed registry. |
| Grammar registry | Static AST node-kind/factory tables and embedded-language metadata | **Adopt declarative idea**, generate Rust adapters/capabilities/tests/SBOM from one lock manifest. |
| Unified extraction | Iterative tree walk extracts definitions/imports/calls/usages/types/env/data-flow-like facts | **Adopt normalized pass shape**, but use typed Rust facts and non-vacuous fixtures. |
| Coverage/quarantine | Distinguishes complete, partial parse, failed, ignored, oversized, and quarantined inputs | **Adopt and improve** with per-pass coverage, exact reasons, reconciliation, and no false-ready state. |
| Project registries | Builds global definitions plus module-to-definition inverted candidates | **Adopt** as scoped candidate registries before resolution. |
| Handwritten pseudo-LSP | Roughly 40K lines of language-specific static/type resolution | **Do not port**. Use small typed semantic providers, canonical Rust metadata/parser crates, explicit tiers, and measured scope. |
| Generic graph buffer | String labels/types, JSON bags, per-object allocation, multiple RAM indexes | **Replace** with typed enums/records, compact vectors/arenas, stable keys, edge-owned identity, and evidence tables. |
| SQLite store | Projects, hashes, nodes, edges, coverage, summaries, indexes, FTS5, vectors | **Adopt proven SQLite features**, extend current project-local store and migrations. |
| Raw SQLite page writer | Manually writes b-tree pages and duplicates schema knowledge | **Reject**. Benchmark `rusqlite` prepared batches, transactions, pragmas, and staging. |
| Full pipeline | Sequential/parallel extraction plus multiple global enrichment passes | **Adopt explicit stages**, deterministic ordering, cancellation, and pass-specific coverage. |
| Incremental pipeline | Loads old graph, reparses changes, restores some edges, rewrites the DB/FTS; skips deeper resolution | **Replace** with changed-row persistence, dependency invalidation, atomic deltas, and full/incremental equivalence. |
| FTS/BM25 | Contentless FTS5 with identifier normalization and capped reranking | **Adopt and improve** with exact candidate verification and honest totals. |
| Semantic/vector | Bundled static token vectors, heuristic score blend, row-wise cosine scan | **Defer/replace** with optional evaluated model packs and ANN; lexical/graph search remains complete without it. |
| Similarity/clones | MinHash/LSH candidate edges over AST leaf trigrams | **Defer until labeled**; retain deterministic candidates/caps, report estimated vs exact similarity honestly. |
| Routes/services | HTTP/RPC/GraphQL/tRPC/broker/channel extraction and rendezvous | **Adopt product capability**, implement typed protocol identities and adversarial negative fixtures. |
| Package/manifests | Extracts common language/build manifests with custom parsers | **Adopt format scope**, use canonical structured Rust parsers and typed package identities. |
| Config/env/IaC/K8s | Links configuration, environment, containers, Terraform/Helm, K8s/Kustomize | **Phase after core graph**, one independently gated enrichment at a time. |
| Tests/history | Adds test-production and Git co-change edges | **Adopt as inferred evidence**, keep confidence/provenance and independent toggles. |
| Cross-repository linker | Scans cached DBs and exact/normalized protocol strings, writes cross edges | **Replace** with explicit-root read federation and typed canonical rendezvous identities. |
| Graph query/Cypher | Custom read-only subset and in-memory bounded-hop execution | **Reject initially**; typed relations/architecture/impact/trace cover agent workflows with smaller surface. |
| MCP tool inventory | Fourteen tools; some overlap ProjectAtlas, `ingest_traces` is a no-op | **Preserve ProjectAtlas names**; enrich existing calls and add at most architecture, impact, trace. |
| Runtime traces | Accepts trace payload shape but does not create runtime edges | **No accepted-capability credit** until a separately specified real trace capability exists. |
| ADR storage | Stores mutable ADR state beside derived graph data | **Do not adopt in this program**; ProjectAtlas/OpenSpec/docs retain decision ownership. |
| Shared artifact | Consistent SQLite snapshot, zstd, integrity/sidecar checks, atomic replace | **Adopt concept**, add digest, optional trust/signature, root/revision policy, and preserve authored state. |
| Watcher | Polls Git HEAD/dirty state, adaptively 5-60s, conservative root pruning | **Retain ProjectAtlas notify design**; add Git/content signature coalescing and conservative lifecycle only. |
| Local UI | Custom HTTP server plus guarded loopback 3D graph | **Out of scope**. Use maintained Rust web crates if separately justified later. |
| Installer/client integration | Detects and mutates many agent/editor configs with dry-run/plan | **Retain ProjectAtlas installer/drift model**; adopt only useful host smoke/plan cases. |
| Tests/defects | Strong reproduce-first malformed/scale/platform corpus; defect evidence exposes gaps | **Adopt negative cases** as independently authored fixtures and release gates. |
| Release/SBOM/security | Multi-platform builds, attestations, SBOM, checksums, but grammar/checksum drift exists | **Improve** with registry-driven per-component SBOM and self-testing fail-closed integrity gates. |

### Architecture Comparison Map

| Concern | Current ProjectAtlas | Architecture pattern baseline | Target ProjectAtlas |
|---|---|---|---|
| Agent workflow | Guided atlas-first funnel | Broad generic graph/query tool inventory | Keep funnel; graph enrichment is automatic behind existing calls. |
| Language truth | Broad detection, approximately 15 specialized parsers/adapters, explicit fallback metadata | Large declarative registry, but capability depth, fixtures, and documentation can drift | Generated manifest for the accepted capability set, separate detect/parse/symbol/semantic/benchmarked tiers, no phantom mode. |
| Semantic depth | Per-file typed symbols and small relation set | Deep but costly handwritten resolvers for about ten families | Typed project registries and focused Rust providers for the accepted semantic set; ambiguity/external outcomes first class. |
| Graph schema | Typed symbols, four typed relations, transient IDs | Generic strings plus JSON property bags | Typed extensible graph, stable keys, typed evidence, candidates, confidence, slot/epoch provenance, and coverage. |
| Full scan | Parallel filesystem/symbol indexing into project-local SQLite | Whole RAM graph then bulk/custom DB dump | Bounded worker extraction into a separate staging database; parent imports the inactive slot and atomically publishes its epoch. |
| Incremental | Notify/per-path refresh foundations | Whole graph reload/rewrite and incomplete deeper resolution | Changed-row transactional deltas plus dependency-driven invalidation; canonical equivalence to full. |
| Text search | Correct literal/regex/fuzzy behavior; candidate content scans | FTS5 BM25 plus capped reranking | FTS5 candidate acceleration with exact verification and correct fallback/totals. |
| Semantic search | None required | Bundled heuristic vectors and full scan cosine | Optional model pack plus measured ANN only; no implicit network/model. |
| Architecture/impact | Basic relations/health/ranking | Rich architecture facets and generic traversal | Three bounded ProjectAtlas tools, typed services, evidence, coverage and token limits. |
| Cross-repository | Strong explicit per-call project isolation; no federation | Cache-discovered read/write cross-links | Explicit selected-root read federation; typed canonical endpoints; no hidden DB/global writes. |
| Watcher | Notify + debounce + poll fallback | Git-only polling | Keep notify; add state-signature coalescing and dependency invalidation. |
| Persistence safety | `rusqlite`, project-local authored metadata | WAL plus custom raw page writer and shared artifacts | `rusqlite` staging/transactions, loud errors, safe backup/import, digest/trust and authored-data preservation. |
| Concurrency/failure | Bounded MCP task progress and cancellation surface | Child isolation, but single-threaded MCP dispatch and global pipeline lock | Supervised per-project workers, independent reads/projects, bounded CPU pools, no process-global serialization. |
| Metrics | Token/read telemetry | Marketing/performance claims with incomplete reproducibility | Public paired-baseline correctness/performance/resource/agent-efficiency artifacts linked to claims. |

## Goals / Non-Goals

**Goals:**

- Reach accepted capability-set parity and exceed the accepted baseline in correctness, determinism, incremental efficiency, resource use, query latency, agent simplicity, and evidence quality. Raw language, parser, node, edge, or tool counts do not define parity.
- Make the atlas-first workflow pleasant and confidence-building for agents: reach useful context quickly, avoid wrong or redundant calls and backtracking, return clear actionable evidence, and remain the preferred first repository tool in blinded workflow review.
- Preserve every existing ProjectAtlas CLI command and MCP tool name, accepted request shape, default behavior, response format, exit/error contract, and normal workflow while making graph enrichment automatic behind scan/watch and query services.
- Support the Phase 0 accepted language/file capability set without equating detection or no-crash syntax support with symbol, relation, semantic, or benchmarked support.
- Build independently structured Rust product logic with ProjectAtlas-native modules, types, method names, schemas, tests, and tool names.
- Make all graph facts auditable through stable identity, evidence, resolution state, confidence, resolver version, slot/epoch provenance, and coverage.
- Guarantee full/incremental canonical equivalence and fail loudly rather than serving corrupt, partial, stale, or misleading success results.
- Prove superiority through pinned reproducible gates rather than assuming Rust is faster.
- Implement and stabilize repository-intelligence architecture and features before the separate repository-wide test-saturation and release-evidence campaign, while adding focused tests at the same time as every stable behavior.

**Non-Goals:**

- No source-to-source translation, mirrored source directory/pass/function naming, copied comments/constants/tests, or unapproved alternate public API.
- No rename or removal of existing ProjectAtlas MCP calls.
- No custom Cypher parser, 3D UI, ADR subsystem, generic runtime-trace claim, or always-on model in this program.
- No execution of repository code, language servers, package managers, build scripts, or repository-supplied parser binaries during normal indexing.
- No universal semantic-depth claim for every syntax grammar.
- No dead-code safety proof from inbound-edge absence.
- No hidden cross-repository discovery, global mutable graph DB, or implicit cross-root write.
- No hand-written SQLite pages or duplicated schema implementations.
- No postponement of focused tests for new algorithms, branches, boundaries, migrations, or public workflows merely because repository-wide coverage closure runs later.

## Decisions

### Decision 1: Preserve Public ProjectAtlas Vocabulary, Independently Name New Internals

Existing ProjectAtlas tool/command/type terminology remains stable. New internal modules and methods are named from their ProjectAtlas ownership and domain role; copied or mirror-shaped source topology is prohibited. Industry-standard names such as SQLite, FTS5, BM25, MinHash, ANN, tree-sitter, BLAKE3, TOON, HTTP, gRPC, GraphQL, and Kubernetes remain unchanged because renaming public algorithms/protocols would reduce clarity.

Implementation review SHALL compare new source topology and identifiers against the Architecture Disposition Ledger and current ProjectAtlas crate ownership. A behavior may be reimplemented, but its code structure, comments, constants, fixtures, and naming must arise from this design and ProjectAtlas conventions. This is a technical independence and maintainability rule, not a cosmetic mass rename after copying.

Every active product owner SHALL have a durable name that identifies its concrete responsibility. New or changed files, modules, crates, types and traits, methods/functions, constants/statics, durable variables, commands, serialized contracts and schemas, fixtures, and tests SHALL NOT use a phase, codename, migration order, temporary state, predecessor identity, or vague catch-all as their enduring owner name; examples that fail this rule include `common`, `admin`, `manager`, `helper`, `utils`, `phaseN`, and `scaffold`. Documented exceptions are limited to exact external protocol or algorithm terminology, frozen compatibility contracts, versioned release or evidence history, and genuine domain operations or lifecycle states. This is a responsibility review rather than a mechanical substring ban or a rule for short local bindings.

Phase 0 SHALL NOT exit while an inaccurate initiative or provisional identity remains for behavior that has landed, including a scaffold or placeholder name and `bootstrap` or `partial` used only to mean unfinished work. Each such artifact must be removed or replaced by the final responsibility-named implementation and its real behavioral test or validator; cosmetically renaming a placeholder without replacing its provisional assertions does not satisfy the exit gate. Genuine first-run/bootstrap operations and typed partial-result or lifecycle states remain valid final domain names.

**Alternative considered:** translate implementation files and rename symbols afterward. Rejected because it imports unrelated ownership decisions and does not produce an independent or better architecture.

### Decision 2: Keep Graph Complexity Behind Existing Calls

`scan`, `watch_once`, and the watcher maintain the active graph automatically. Existing file/folder ranking consumes available graph signals without new required arguments. File summaries add only a bounded relationship digest, coverage, and typed next-step recommendations. Full evidence stays in `atlas_symbol_relations` or the new analysis tools. Graph schema version, accepted relation families, capability tiers, active search backend, slot/epoch state, and optional-pack lifecycle are surfaced through additive fields on the existing `atlas_settings`; this program adds no separate schema or capability tool.

Only `atlas_architecture`, `atlas_impact`, and `atlas_trace` may be added by this program. They are optional expert/informational surfaces and are never prerequisites for graph freshness or normal startup, navigation, search, file selection, summary, outline, relation, or slice work. `atlas_symbol_relations` gains optional direction/kind/depth/evidence filters while preserving old defaults. Exact search modes keep their semantics and deterministic lexical fallback remains available even when FTS acceleration is unavailable. A generated inventory and golden corpus cover every existing CLI and MCP surface, including names, arguments, defaults, output formats, exit/error behavior, root isolation, and recorded startup/navigation sequences; old clients do not need more calls.

**Alternative considered:** clone fourteen generic tools and a Cypher surface. Rejected because it expands agent choice and token cost while duplicating workflows already represented by ProjectAtlas services.

### Decision 3: Generate One Honest Language Capability Registry

Create a version-controlled registry/lock input that owns identifier, aliases, filename/extension/content rules, parser artifact, ABI, embedded-language adapters, feature pack, extraction queries, semantic provider, fixtures, source/version/digest/license metadata, and capability tiers. Code generation emits:

- Rust detection and adapter tables;
- capability/settings output;
- parser/fixture conformance tests;
- `language-capabilities.json` for docs/releases;
- per-component SBOM/provenance inputs;
- drift checks for README and packaged assets.

Phase 0 freezes an accepted capability set from product value, user workflows, parser quality, supply-chain status, platform support, and resource cost. The initial manifest contains an explicit 212-mode union, retains all 63 current public modes, and maps those modes to 207 normalized parser capabilities after 11 standard-name aliases and justified parser reuse are disclosed; CFML script and tag dialects remain separate modes. These rows are accepted pending delivery targets, not achieved claims. Capability-set parity is achieved only when every accepted ProjectAtlas capability ID passes its declared detect/parse/symbol/relation/semantic/benchmark tier; deliberately excluded entries remain listed with reasons and may be accepted in later revisions. Aliases, semantic modes, and parser reuse are disclosed rather than counted as unique parser implementations. A fixture with zero expected declarations earns parse/no-crash tier only, and the breadth floor never substitutes for capability evidence.

**Alternative considered:** copy a large grammar inventory into Cargo dependencies and hand-maintain match expressions. Rejected for binary size, compilation time, drift, and inaccurate claims.

### Decision 4: Use Trusted Core Parsers Plus An Explicit Broad Grammar Pack

Current trusted grammar crates remain available for priority languages. Phase 0 freezes the closed native-grammar and versioned-WASM candidate inventory, evaluation schema, containment/ABI/portability requirements, and fail-closed selection rules without selecting or advertising a broad-pack host. Representative candidates land in Section 5, and Section 11 runs the claim-eligible correctness, throughput, memory, startup, package-size, and hosted-platform campaign before any host is selected. The preferred candidate form remains versioned tree-sitter WASM parser artifacts behind a separate Rust pack-host process with strict time/memory/input limits because that design avoids dynamically loading arbitrary native libraries and isolates broad syntax support from the core executable. If WASM fails the registered gates, a vetted generated-parser pack MAY be selected, but ProjectAtlas-owned parsing orchestration, extraction, resolution, storage, and query logic remains Rust and every parser remains pinned/inventoried.

The broad pack is installed explicitly and can be mirrored for offline use. Normal scan/query never downloads or compiles a parser. The default feature graph and core executable SHALL NOT link, load, or initialize a broad grammar/WASM runtime, ANN backend, tokenizer/model runtime, or model asset. Pack manifests may be inspected as data; execution occurs only by starting the separately installed, supervised pack process after an explicit enablement or pack-targeted task. Core operation remains useful without a pack and reports precise tiers.

The following are hard per-platform default-core release ceilings, measured without optional packs on the declared release runners and permanent benchmark corpora. Phase 0 MAY tighten them; raising one requires a reviewed design change rather than rebasing the measurement:

| Default-core property | Hard ceiling |
|---|---|
| Stripped release executable | 48 MiB |
| Installed core payload, excluding symbol/debug files and optional packs | 64 MiB |
| Cold `runtime-info` process completion, p95 | 250 ms |
| Cold MCP process start through successful initialize and tool readiness, p95 | 500 ms |
| Idle MCP RSS 30 seconds after initialize with no scan or pack process | 96 MiB |
| Peak RSS for the declared no-pack large-corpus full scan | 512 MiB |

Each optional pack has separate executable/install/RSS/startup budgets in its manifest and cannot spend the default-core allowance. A default-core budget failure blocks release even if an aggregate comparison is favorable.

**Alternatives considered:** runtime compilation of repository grammars; dynamic loading of unverified shared libraries; an always-bundled monolith. Rejected for supply-chain, startup, crash-isolation, and package-size reasons.

### Decision 5: Use Typed Facts, Stable Keys, And Explicit Resolution

Extraction emits typed syntax facts, not resolved edges. Project-wide provider stages build package/module/scope registries and candidate indexes, then produce typed resolution outcomes. The graph contract separates:

| Concept | Required fields |
|---|---|
| Entity | stable key/version, project/repository, kind, qualified identity, normalized path/external identity, source span, language/adapter, slot/last-changed epoch |
| Relation | stable key/version, source/target stable keys or unresolved identity, typed kind/payload, resolution state, confidence, resolver/version, slot/last-changed epoch |
| Evidence | origin path/span or manifest/protocol/Git evidence, direct/inferred class, bounded explanation |
| Candidate set | ambiguous/unresolved source, typed candidates, rejection reasons, total/returned/truncated |
| Coverage | file/stage/relation class, complete/partial/failed/ignored/quarantined/stale, counts, limits/reason, slot/last-changed epoch |

Confidence is a small typed class with documented meaning, not arbitrary precision theater. `resolved`, `ambiguous`, `unresolved`, `external`, `inferred`, and `truncated` are not collapsed. Stable keys use a versioned BLAKE3 encoding of owning project/repository identity, entity kind, normalized path or external package identity, qualified name, and documented overload/signature disambiguator.

Root binding is a typed identity boundary. Normal mismatched opens stop at `RebindRequired`; an explicit verified move on the existing root-set surface transitions through `Rebinding` to `Bound` in one database transaction and preserves `ProjectInstanceId`. A copy/clone instead initializes independently or explicitly detaches to a new instance ID. This program does not add identity adoption, and snapshot import never replaces the destination identity.

Every accepted relation family has one generated traceability row naming its stable serialized kind, owning module, producers, consumers, required evidence, resolution and confidence rules, capability tier, feature/pack boundary, schema migration, query surfaces, fixtures, precision/recall gate, and OpenSpec task/test evidence. A relation family cannot enter the shared graph merely because an extractor can emit a string; missing ownership, consumer, evidence, or benchmark data keeps it adapter-private or deferred. `atlas_settings` renders this accepted relation-family manifest without creating a new schema tool.

**Alternative considered:** generic string labels and JSON property bags. Rejected because identity, migrations, queries, validation, and Rust compiler coverage all become weaker.

### Decision 6: Implement Small Semantic Providers, Not A Pseudo-LSP Monolith

The initial accepted semantic-provider set covers Go, C/C++/CUDA, PHP, Perl, Python, JavaScript/JSX/TypeScript/TSX, C#, Java/Kotlin, and Rust. Each provider consumes normalized facts and package metadata through a common typed interface but owns only the language-specific resolution rules that cannot be expressed generically. Canonical Rust crates and structured metadata are preferred for manifests, package/module rules, and compiler formats. External language-server processes are not required for normal ProjectAtlas correctness.

Every provider ships positive, negative, ambiguity, external dependency, malformed, and cross-file tests. ProjectAtlas abstains when static evidence is insufficient. Objective-C, Zig, Vue, and PowerShell retain current extraction and can advance independently without blocking the accepted semantic-provider set.

**Alternative considered:** port the entire handwritten type system/resolver collection. Rejected due to maintenance cost, correctness risk, and conflict with ProjectAtlas's focused product boundaries.

### Decision 7: Publish Through Two Derived-Data Slots And Epochs

The live database owns exactly two physical core structural/lexical derived-data slots plus `(active_slot, active_epoch)` metadata. A supervised full scan writes only to a parent-created separate staging database. The worker cannot open the live database for writes or choose a slot. After read-only staging validation, the parent re-discovers and fingerprints the complete source/config/compiler-metadata input manifest; any drift aborts, retries, or replans within the task budget. Only then does the parent start one live-database transaction, clear/replace the inactive slot from staging, assign the next epoch, reconcile schema/root/coverage/counts/UTF-8/endpoints/FTS state, and atomically flip `active_slot` and `active_epoch`. Normal graph, coverage, and FTS queries filter by `active_slot` only. Per-row `last_changed_epoch` is freshness/provenance metadata and is never a visibility predicate.

Optional semantic/vector storage is pack-owned and separately published. `StructuralGenerationInputs` contains only the source, repository, ignore/configuration, compiler-metadata, parser/registry/provider, feature, seed, and hard-budget identities that determine core structural/lexical output; it never contains model, tokenizer, model-input, or preprocessing identity. `SemanticGenerationInputs` contains one captured structural `(active_slot, active_epoch)` plus model, tokenizer, preprocessing, normalized model-input, feature, seed, and semantic-budget identity. Its active generation binds to that complete semantic tuple. A structural flip or incremental epoch advance makes an older semantic generation stale immediately, while a model/tokenizer/preprocessing-only change invalidates and rebuilds only the semantic generation. Semantic/hybrid queries require a separately ready compatible semantic generation; lexical/structural publication never waits for or rolls back because vector work fails.

The previously active slot becomes the retained rollback slot after a full publication. It remains intact until the next successful full publication needs that inactive slot or an explicit bounded cleanup/backup policy removes rollback eligibility. The design does not create append-only slot chains or a table copy per epoch.

Incremental work computes a delta against the active slot, including affected inbound references and provider/global-enrichment dependency keys. It validates the delta, then updates/deletes/inserts only affected rows in that slot and advances `active_epoch` in one transaction. Unchanged rows keep an older `last_changed_epoch` but remain visible because they belong to the active slot. Readers observe the pre-commit or post-commit epoch, never a mixture; SQLite rollback preserves the prior epoch on failure. Authored purpose/telemetry/settings tables are outside both disposable slots.

**Incremental publication pattern fit (`ARRI-4.14`, `ARRI-7.7`, `ARRI-7.18`, `ARRI-7.19`):** the runtime computes one concrete `IncrementalStructuralDelta` after bounded scan, lexical preparation, and parser work, then `AtlasStore` applies that value under one `TransactionBehavior::Immediate` transaction. The transaction rechecks the captured slot/epoch and persisted versioned content/Git signature, coalesces an already-published target signature before any mutation, invalidates only the affected active-slot graph/evidence/coverage/lexical closure, applies compatibility rows, stores the new signature, and advances the epoch exactly once. The retained inactive slot is not read, copied, or mutated; an epoch advance makes any older semantic tuple stale without introducing semantic-pack storage early. Reusing the existing changed-path methods was considered simpler, but those methods own separate commits or direct live writes and therefore cannot prevent mixed visibility or late-failure leakage. A third slot, incremental staging database, overlay chain, actor pipeline, generic transaction-command framework, and whole-graph copy/rewrite were rejected because they add state or machinery without strengthening the SQLite-owned atomicity contract. Focused proof is owned by `task_arri_ut_arri_4_14`, `task_arri_ut_arri_7_7`, `task_arri_ut_arri_7_18`, and `task_arri_ut_arri_7_19`, including injected late rollback, two-connection coalescing, zero-write no-change WAL evidence, and affected-row write scaling.

The store rejects malformed paths and every prepared mutation outside the declared affected closure before taking the write lock. Full-scan staging clears the copied live signature and requires the captured target signature before publication, while built-in or generated purpose work cannot replace stale agent-authored intent. Continuous notify mode schedules the same complete content/Git signature check even without a relevant source event; concurrent connection and CLI-process proofs require one publication with no busy leak or second epoch. The proportional-write gate applies the same one-file closure to small and materially larger unrelated graph/lexical fixtures and permits only a fixed additive WAL-frame allowance for B-tree depth or page placement. Full publication retains complete `quick_check`, foreign-key, graph, coverage, lexical, and FTS reconciliation. Incremental publication instead revalidates the exact schema/root/publication contract, requires live foreign-key enforcement, and reconciles only the declared active-slot path closure through slot/path indexes plus trigram-indexed FTS path lookup; `ARRI-11.5` and `ARRI-11.6` retain the measured release campaign so integrity and proportional cost are both release evidence rather than assumptions.

**Migration backup and retained-slot cleanup pattern fit (`ARRI-4.20`):** a supported file-backed migration uses the canonical `rusqlite` online-backup API after read-only preflight and before the target database is opened for writes. A non-blocking project-local advisory lease serializes backup retention through the migration write so concurrent old-runtime opens cannot delete another process's still-needed image. The runtime verifies `quick_check`, source schema identity, and a streaming BLAKE3 digest, retains the image beside the project-local database under a digest-bound name, and removes an older image only after the replacement is verified. Lease contention or obsolete-backup cleanup failure is reported as recoverable and prevents migration writes while leaving the verified image in place. Full publication reuses the retained inactive slot only inside the already validated immediate transaction; every table delete is bounded by that slot, a cleanup error reports that rollback will restore it, and the active slot is never a cleanup target. A focused proof captures committed WAL frames, rejects a corrupt backup without changing the destination, restores the pre-migration image, exercises lease contention and recoverable backup cleanup, injects a late retained-slot cleanup failure after earlier table deletes, proves transaction rollback restores both slots and authored metadata, and then proves the next validated publication replaces only the retained slot. Raw main-file copy, pre-verification deletion of the prior backup, a third slot, an append-only generation chain, and a background cleanup framework were rejected because they weaken WAL correctness or add unbounded lifecycle state without strengthening SQLite-owned atomicity.

Parser, registry, provider, compiler-metadata, or configuration version changes participate in core invalidation through `StructuralGenerationInputs`. Similarity, architecture communities, call-only federation results, coverage, aggregates, and FTS are recomputed or marked stale when their declared input dependency changes. Model, tokenizer, preprocessing, normalized model-input, or captured structural-tuple changes participate only in `SemanticGenerationInputs` and invalidate the separate semantic generation; optional vector rebuilding and activation occur only through the semantic lifecycle. Stale results are never silently restored.

**Alternatives considered:** whole-graph RAM reconstruction for one-file changes; append-only overlay chains or a physical table set for every epoch. Rejected respectively for write amplification and query/migration complexity.

### Decision 8: Supervise Risky Work Without Serializing Projects

A hidden ProjectAtlas index-worker mode uses a length-delimited typed protocol. The parent selects the project, configuration, registry, budgets, and staging destination. The child receives no authority to activate or replace the live database. Heartbeats identify current stage and bounded file identity; no-progress and resource thresholds terminate the child. Repeated deterministic suspect evidence may quarantine a file while coverage remains partial and visible.

On Windows, the parent assigns the child to a kill-on-close Job Object before accepting work and configures supported process-memory/CPU limits. On Unix, the child starts in its own process group with supported `rlimit` ceilings, while the parent independently enforces wall-time, no-progress, cancellation, and sampled resource watchdogs and terminates the whole group. Platform reports distinguish kernel-enforced limits from sampled/watchdog limits. A broad WASM pack additionally receives fuel or epoch-interruption deadlines and a hard linear-memory cap with no inherited filesystem/network capability unless explicitly required by a separately reviewed pack contract. Native parser process isolation limits crash/resource blast radius; it is not described as a hostile-code sandbox or complete filesystem-security boundary.

The effective resource envelope uses typed precedence: per-call override, project configuration, then a derived default, followed by unavoidable hard clamps. Derived defaults use the minimum applicable host, Linux cgroup v2/v1 CPU-memory-cpuset/quota, Windows Job Object, and ProjectAtlas safe limits. Reports preserve requested, configured, derived, effective, source, and kernel-enforced versus watchdog/advisory values; invalid zero, negative, overflowed, contradictory, or unsupported limits fail explicitly. Platform discovery stays behind narrow `cfg` adapters and uses maintained safe wrapper crates where they meet the contract.

CPU parsing uses a bounded Rayon pool; orchestration and process I/O use existing synchronous/runtime patterns or Tokio only where it removes real complexity. Separate project-local databases can index concurrently. Read-only queries remain available against the active slot/epoch while a full scan builds its separate staging database. There is no global pipeline lock.

**Alternative considered:** keep all parser/model work in the long-lived MCP process. Rejected because generated parsers and optional native model runtimes can abort outside Rust unwinding and would spend the default-core idle/startup budget.

### Decision 9: Keep Lexical Retrieval Complete And Treat FTS/Semantics As Accelerators

Deterministic lexical retrieval is an always-available default-core capability. The current exact matcher and bounded deterministic fallback remain authoritative when FTS5 is absent, fails runtime preflight, does not support the selected tokenizer, or cannot safely narrow a regex/fuzzy/short/punctuation query. A migration ID assigned from the repository migration ledger MAY add an external-content/trigram candidate FTS table for file text after benchmarks prove support, plus an identifier-oriented FTS table using Unicode tokenization and deterministic camel/snake/qualified-name expansion. FTS is conditional acceleration: it narrows candidates, the exact matcher verifies final lines, differential tests require result equivalence, and `atlas_settings` reports the active backend and fallback reason.

Optional semantic retrieval is a separately installed and supervised feature pack. Phase 0 freezes the typed readiness gate and labeled evaluation contract for maintained Rust-compatible ANN backends and local model runtimes without selecting a backend or model. Section 10 materializes representative candidates, and Section 11 runs the claim-eligible labeled retrieval, package-size, licensing, hosted-platform, tolerance-based determinism, update-cost, RSS, and latency campaign before a shipping choice is accepted. No ANN/model runtime is linked or initialized in default core, and no model is bundled or downloaded implicitly. No backend/model choice is committed until the quality gate passes.

The per-project semantic lifecycle uses the single typed state contract from the semantic specification: `Absent`, `InstalledDisabled`, `EnabledIndexMissing`, `Building`, `Ready`, `Stale`, `Updating`, `RollbackReady`, `Incompatible`, `Failed`, and `Removing`. Its transition table defines allowed operations, active/rollback generations, and readiness. Install verifies platform, manifest, digest, license, runtime, and separate resource budgets before `InstalledDisabled`; enable/build publishes a derived semantic index against the complete `SemanticGenerationInputs` identity only after reconciliation; a structural epoch change makes the selected generation `Stale`; disablement enters `InstalledDisabled` immediately; update retains last-known-good rollback data; and removal deletes only pack-owned derived data/assets before `Absent`. A failed semantic task never flips or invalidates the structural slot.

Normal `scan`, `watch_once`, and watcher completion means the required structural/lexical pipeline published successfully. If semantic maintenance was requested or auto-scheduled, its task/state is reported separately and does not turn structural success into failure. Omitted or explicit lexical search remains deterministic and independent of pack state. Explicit semantic or hybrid search succeeds only in `Ready` or `RollbackReady`; every other lifecycle state returns a typed capability error and never silently executes another mode. Program/phase completion cannot claim semantic capability until install, enable, build, stale-update, update, rollback, failure, disable, removal, offline, and cross-platform lifecycle tests pass.

**Alternative considered:** embed a static token-vector blob and scan every vector. Rejected because retrieval quality is ungrounded and normal query complexity scales linearly with vector count/dimensions.

### Decision 10: Expose Typed Architecture, Impact, And Trace Services

`projectatlas-core` owns only reusable graph identities, relation/evidence types, selector/filter newtypes, and hard-budget primitives. `projectatlas-service` owns use-case request objects, reports, algorithms, and cursor semantics for relations, architecture, impact, trace, and federation, preserving the acyclic `service -> db -> core` direction. Architecture facets are computed from direct typed facts first; optional deterministic community/layer inference is clearly separated. Impact returns evidence paths and affected identities, not an invented risk score. Trace returns node-simple paths: one stable entity identity may appear at most once in a path, so cycles cannot be represented by taking a different edge back to a visited node. Dead-code output is candidate-only and gated by entry-point/export/framework/dynamic/unresolved coverage.

All query budgets include depth, rows, visited nodes, expanded edges, wall time, cancellation, and memory. Result ordering, totals where knowable, and truncation are deterministic. Every opaque cursor binds a cursor-format version, canonical project root identity or explicit ordered federation roots, `active_slot`, `active_epoch`, graph schema/capability version, query kind, normalized selectors/filters/relation families/order, and page-budget identity. A checksum detects accidental corruption; any binding mismatch, epoch change, root change, or unsupported cursor version returns a typed stale/invalid-cursor error rather than resuming against different data. CLI/MCP adapters only parse typed parameters and serialize service output.

**Alternative considered:** general graph query language first. Rejected until real agent-task evidence demonstrates recurring needs that typed services cannot cover.

### Decision 11: Federate Only Explicit Call Roots And Fail Closed

Federation is query-time and call-only. Each approved relation/architecture/impact/trace request supplies its complete ordered root set explicitly; roots are not inherited from a prior call, remembered in server state, discovered from caches, or expanded from the active single-project default. Each root must already own a valid project-local database and is opened with SQLite read-only/query-only flags. Entities remain project/repository qualified. Protocol-specific canonical identities match packages, HTTP, gRPC, GraphQL, tRPC, channels/topics, configuration, and data/schema references with original evidence and ambiguity.

Before query execution, ProjectAtlas validates every requested root's canonical binding, schema/capability compatibility, integrity/readability, active slot/epoch, revision/dirty-state metadata, and path authorization. Any missing, writable-only, mismatched, corrupt, unsupported, changed-during-bind, or otherwise invalid root fails the entire call before returning graph rows; silently dropping a root or returning a plausible partial federation result is prohibited. Federated calls never write participating databases, persist cross-root edges, schedule background work, or mutate single-project state.

A persistent federation cache is outside this change. If future scale evidence requires one, it needs a separate specification for derivation, trust, invalidation, and deletion and still cannot become an authored global source of truth.

**Alternative considered:** scan cache directories, retain an implicit root set, partially answer around invalid roots, or mutate matching databases. Rejected for correctness, security, concurrency, and explicit-root isolation.

### Decision 12: Make Snapshot Sharing Optional And Integrity-First

Snapshots use SQLite online backup or `VACUUM INTO`, zstd, a cryptographic digest over metadata plus payload, schema/runtime/registry/project/revision metadata, decompression/size limits, temporary-path `quick_check`, required-table/count reconciliation, stale destination sidecar removal where replacement is unavoidable, and atomic activation. Trust/signature policy is optional but explicit. Dirty overlays are never represented as a clean commit artifact without a content/state digest.

Authored purpose, review, settings, and telemetry state is never replaced by an imported derived graph. Import writes only the inactive derived-data slot or preserves/reconciles authored tables through a typed migration before an atomic slot/epoch flip.

**Alternative considered:** raw-copy the main SQLite file in WAL mode. Rejected because committed WAL frames and concurrent checkpointing make the main file alone an unsafe snapshot.

### Decision 13: Measure Superiority With A Pinned Public Harness

The harness records exact commits, repositories, ignore profiles, hardware/OS, toolchains, commands, timeouts, warmups/repetitions, failures, raw results, and machine-readable summaries. Provenance claims stop at the exact bound components: the Phase 0 calibration contract pins Cargo and rustc executable identities and requires the generated executable digest to be observed when the runner executes, but it does not claim pre-bound linker, SDK, or complete build-chain identity until those inputs are independently pinned. It measures:

- per-language/per-relation precision, recall, F1, abstention, span and coverage;
- full/incremental and 1/N-worker canonical equivalence;
- Unicode and platform correctness through real processes;
- cold/warm indexing, one-file/fan-out/no-change updates, writes/WAL, RSS, DB/package bytes;
- simple/bounded graph query p50/p95 and cancellation;
- lexical/semantic IR quality and cost;
- blinded agent answer quality, unsupported claims, tokens, reads, calls, and time.

Before a benchmark run, the harness freezes accepted capabilities, corpora, primary endpoints, scoring rubrics, equivalence/tolerance rules, minimum sample sizes, random seed, and exclusion policy. Timeout, crash, missing output, invalid serialization, or unsupported accepted capability counts as failure rather than disappearing from the denominator. Primary endpoint confidence intervals use a deterministic paired bootstrap with at least 10,000 resamples; proportion-only capability gates additionally publish Wilson intervals, and Holm correction protects families of primary comparisons.

The decision functions are explicit:

- **Accepted capability-set parity:** every accepted capability independently meets its declared correctness/coverage threshold and compatibility contract. Aggregate success cannot hide a failed language, relation family, platform, or query class.
- **Performance superiority:** all hard absolute and per-resource budgets pass, the upper corrected adverse confidence bound for paired geometric-mean ProjectAtlas/baseline cold-index time and peak-RSS ratios is at most `0.80`, and the upper bound of each required corpus's runtime ratio is at most `1.10`. Writes, DB bytes, package/install bytes, and every other named resource receive independent non-regression and claim decisions; one composite score cannot trade memory or storage failure for speed.
- **Agent-quality superiority:** tasks are paired, blinded, and order-randomized under a frozen rubric. The point estimate for normalized quality improvement is at least five percentage points and the corrected 95% lower confidence bound is above zero. A high-baseline 95% absolute-quality result may be reported separately but does not replace the paired superiority rule and is not called superiority unless that rule also passes.
- **Retrieval/semantic acceptance:** lexical baselines remain complete; MRR, nDCG@10, Recall@10, latency, and cost use paired queries and declared non-inferiority/superiority margins. Vector preprocessing is exact and seeded, while floating vector values compare with backend-declared absolute/relative tolerances; cross-platform bit identity and ANN internal topology are not required. Top-k overlap and labeled recall must still meet frozen thresholds.

Query latency has two non-interchangeable tracks. Warm SQLite/service benchmarks use an already-open database after declared warmups and exclude process start and JSON-RPC serialization; their reference-host goals are 1 ms for simple indexed name/filter/one-hop queries and 50 ms for the declared bounded three-hop query. Warm end-to-end MCP benchmarks use an already-started server but include JSON-RPC framing, project routing, service execution, TOON/JSON serialization, and response delivery; their reference-host goals are 50 ms and 150 ms respectively. Phase 0 pins an eligible runner class and a pre-result tolerance factor no greater than 1.25; raw measured latency is never divided by a calibration score, and uncalibrated hosted-runner results are informational. Cold process/MCP startup is measured only against Decision 4's fixed-runner startup budgets. Paired same-host comparisons against each baseline remain mandatory.

ProjectAtlas must be correct first. A superiority claim requires every applicable decision function and compatibility/resource gate to pass. Failures remain published and block the claim; changing a margin after seeing results creates a new benchmark version and requires rerunning every baseline.

**Alternative considered:** repeat marketing wall-clock and manual PASS claims. Rejected because they cannot support engineering or release decisions.

### Decision 14: Split Large Modules Only At First Touch

There is no standalone repository-wide module-splitting phase. When an implementation task first touches a large file and the accepted change would otherwise mix ownership or make focused testing materially harder, extract only the smallest existing ownership boundary needed by that task. Preserve behavior with focused tests and compatibility re-exports where an internal path is already consumed. Untouched large files are not split merely to match this target map, which is need-driven guidance rather than a mandatory end-state checklist:

```text
projectatlas-core/src/
  graph/{entity,relation,evidence,identity,resolution,coverage,selector,limits}.rs
  language/{registry,capability}.rs

projectatlas-db/src/
  schema/{mod,migrations,slots}.rs
  graph/{entities,relations,evidence,coverage}.rs
  search/{file_fts,symbol_fts}.rs
  existing purposes/health/telemetry modules

projectatlas-symbols/src/
  registry/{generated,adapter,embedded}.rs
  extraction/{tree_sitter,structural,manifest,fallback}.rs
  resolution/{registry,provider,candidates}.rs
  languages/<projectatlas-native provider names>.rs

projectatlas-service/src/
  ranking.rs, search.rs, summary.rs, slice.rs, relations.rs
  query/{cursor,architecture,impact,trace,federation}.rs

projectatlas-cli/src/
  index/{full,incremental,worker,publication}.rs
  watch/{events,state_signature,reconcile}.rs
  mcp/{navigation,graph,index,project_routing,configuration,runtime,task_control,purpose,telemetry}.rs
```

`projectatlas-index` becomes a crate only if another real binary must consume scan/index orchestration without `projectatlas-cli`; a hypothetical consumer is insufficient. Semantic indexing, ANN, tokenizer, and model-runtime code remains owned by the optional semantic pack outside the default core crate graph and process. Speculative crates and traits are prohibited.

### Decision 15: Win Performance Through Data Layout And Less Work

Rust is an enabler, not the benchmark result. The graph hot path uses compact typed vectors/arenas, numeric internal IDs, string/path interning, relation-kind-owned identity fields, bounded worker-local batches, deterministic indexed completed-batch collection, and sequential persistence. It avoids one heap allocation per fact and avoids JSON serialization/deserialization between extraction, persistence, adjacency lookup, and traversal. Persistence uses measured prepared batches and staging publication; queries use source/target/kind/package/stable-key/slot indexes; optional pack-owned vectors use ANN rather than full scans.

ARRI 4.21 selects one concrete compatibility-graph layout at the existing parser/DB boundary: each worker converts its expanded per-file result into contiguous symbol and relation rows whose string and path fields are 32-bit IDs into one owned intern pool, while symbol, relation, and parser kinds remain typed enums. Full-scan parsing derives reporting and completed-result batch size from the constructed Rayon pool's actual worker count before sequential persistence; incremental publication retains the compact rows until its required atomic transaction commits. Persistence borrows interned text directly, including search-summary inputs, rather than rebuilding per-row owned strings. The focused representative repeated-fact measurement requires the compact retained bytes to remain below half of the equivalent expanded graph, freezes four-byte interned and optional IDs plus bounded row sizes, proves exact round-trip compatibility and direct compact-row persistence, and does not claim the adjacency/query work assigned to ARRI 4.22, the million-node complete-process allocation/RSS/throughput evidence assigned to ARRI 4.23, or the repeated one-worker/N-worker determinism campaign assigned to ARRI 7.11. The nearest simpler alternative—keeping the existing expanded graph but chunking Rayon results—bounds the result count but retains repeated path and text allocations. A generic graph framework, unsafe arena, shared concurrent interner, and new interning dependency are rejected because the closed worker-local layout needs none of their lifecycle, synchronization, or public-surface cost. The preserved borrowed-graph DB compatibility shim may clone before compaction, but production parser and incremental-publication paths enter compact storage directly; a second borrowed interner is rejected unless ARRI 4.23 attributes material measured cost to that non-hot shim.

ARRI 4.22 normalizes that compact parser output into slot-bound typed entities, logical relations, occurrence evidence, and non-traversable resolution occurrences without a JSON intermediate. One safe `GraphKeyArena` retains each canonical entity, relation, evidence, and resolution key once and the compact persistence plan keeps fixed offset/digest handles; one shared text arena owns qualified names. Schema 16 is installed by the append-only `install-graph-query-indexes` migration and adds four ordered, slot-leading indexes: source/relation-kind/stable-key adjacency; target/relation-kind/stable-key adjacency; relation-kind/stable-key filtering; and entity-kind/qualified-name/stable-key lookup. Stable-key lookup deliberately reuses the existing `(structural_slot, stable_key_digest)` primary key rather than adding a redundant index; verification checks the indexed `SEARCH` plan and the primary-key column order through SQLite schema metadata instead of treating a generated `sqlite_autoindex_*` spelling as a product contract. Agent-facing reads capture the active publication tuple in a SQLite read transaction, use cached prepared statements, decode typed columns directly, and enforce caller row limits. Individual reads are snapshot-safe; the focused two-hop composition proof holds only while publication is unchanged. Request-wide traversal snapshots and cursor bindings remain owned by ARRI 8.12–8.15.

Entity derivation excludes compatibility-only dependency, import, and unknown pseudo-symbols. One contiguous name index sorts compact entity rows by compatibility name, stable entity digest, and original row order, deduplicates equivalent logical targets, then uses bounded partition points to select deterministic candidate ranges without a heap map per name. A unique range resolves; a duplicate range persists one typed `Ambiguous` occurrence with bounded stable-key-ordered candidates; and a missing call/import persists one typed `Unresolved` occurrence. Both abstention states remain visible alongside the compatibility projection and stop before exact adjacency. Declaration signatures participate in stable identity and are returned with optional producer discriminators so distinct overloads remain distinguishable even when equivalent parser occurrences collapse to one candidate. Resolved same-file calls become internal edges with `High` confidence, while direct containment remains `Exact`. A missing-source dependency becomes a file-owned exact external package edge only for the complete Cargo producer contract: `DependsOn`, synthetic source `cargo`, manifest parser provenance on both graph and relation, language `cargo-manifest`, and basename `Cargo.toml`; every other dependency source becomes a typed unresolved occurrence. Cross-file inbound reconciliation remains owned by ARRI 7.4.

Logical edge identity excludes mutable source coordinates. Repeated parser occurrences therefore upsert one logical relation while each occurrence persists separately with repository-path origin, resolver identity/version, content fingerprint, same-fingerprint discriminator, direct evidence, relation-owned confidence, complete coverage, slot, epoch, and canonical source context when the bounded identity-text contract accepts it. Compatibility data does not provide trustworthy byte offsets, so typed evidence leaves the all-or-none source span empty instead of inventing coordinates. Resolution occurrence identity is target-free; bounded candidates belong to that occurrence through deterministic ordinals and same-slot foreign keys. Entity, relation, evidence, and resolution writes first probe an existing digest for a different encoding version or canonical byte string before same-path invalidation, then retain the same equality guard on the upsert itself; any mismatch returns a typed collision error. The surrounding transaction rolls back typed entities, logical relations, evidence, resolution occurrences/candidates, compatibility symbols/relations/metadata, and publication state on collision or injected evidence failure. Incremental writes receive the publication owner's selected active slot and next epoch, while full scans write parser output into the parent-owned staging database for the later validated slot flip.

The nearest simpler alternative—querying the compatibility tables or decoding a per-file JSON graph—cannot provide typed target identity, occurrence evidence, or bounded indexed inbound/outbound adjacency. A more complex generic graph-query layer, redundant stable-key index, per-edge occurrence payload, or request-wide snapshot service is rejected here because the accepted closed query set and existing SQLite publication owner already satisfy this task; scale, allocation, RSS, persistence throughput, index bytes, and warm-latency claims remain gated by ARRI 4.23. Clean-room review against `DeusData/codebase-memory-mcp` at pinned commit `2469ecc3a7a2f80debe296e1f17a1efcfdb9450c` influenced only behavioral failure analysis: ProjectAtlas independently retains every evidence occurrence instead of deduplicating away source facts, rejects digest/canonical aliasing, and keeps unresolved references out of exact adjacency. Upstream names, topology, module boundaries, constants, comments, tests, and expressive implementation details are not copied.

ARRI 4.23 uses one dedicated release-mode evidence executable with an instrumented Rust global allocator and same-executable supervised workload child. The evaluator-local concrete process boundary owns process-group lifecycle, manifest-declared 100 ms RSS sampling, manifest-bounded child output, parent-death request, hard timeout, explicit drain and teardown, and the RAII hard-kill fallback without adding another supervisor framework. The 100 ms contract is the configured interval; raw strictly increasing sample timestamps remain retained for audit, but scheduler overhead and delay are not mislabeled as a portable maximum-gap guarantee. Production and evaluator graph batches both use indexed Rayon collection followed by sequential persistence inside one database-owned transaction per completed worker-bounded batch; the source-compatible single-graph staging operation delegates to that same transaction owner. This removes repeated per-graph commit and automatic-checkpoint work while bounding memory, ordering, and failure rollback to the existing completed batch; a whole-scan transaction guard, background writer, actor, or channel would widen ownership and rollback without a demonstrated need. Focused 1/N-worker production evidence proves returned graph paths preserve producer order, while the scale artifact retains and independently validates the completed graph count plus a SHA-256 over the encountered producer-path sequence without an evaluator-only corrective sort. Live and staging SQLite families occupy separate evaluator-owned directories so each snapshot can distinguish its main/WAL/SHM bytes from unattributed SQLite temporary files without counting the other database. The evaluator records sealed staging as transient publication cost, explicitly removes the staging main/WAL/SHM family after successful publication, checkpoints and closes the final live database, retains persistent live bytes separately from the maximum observed live-plus-staging coexistence across pre-seal, sealed, and post-publication checkpoints, and attributes logical index-page bytes through SQLite `dbstat` without adding those subset bytes to the physical file total. Staging persistence and inactive-slot publication retain distinct recomputed logical-facts-per-second values; the manifest-owned portable throughput floor applies to staging, while publication throughput remains an explicit measured value rather than relabeling its duration. Allocator requests mean allocations plus reallocations while the component counts and explicit SQLite/native exclusion remain visible.

The supervisor rejects dirty full-profile source before spawn and rechecks HEAD plus exact filter-free worktree state after child exit before accepting the child result. Every Git read uses the shared repository-bound policy: one canonical source root and Git executable, explicit metadata/worktree arguments, cleared Git/config environment, built-in conversion through a private sanitized index for worktree state, a 30-second process-tree deadline, and an 8 MiB retained-output ceiling. The evaluator compares every material compiled evaluator/plan/process/Git-policy/workspace-and-target-manifest/service/database/publication/example/evaluation-manifest/lock input byte-for-byte with its exact tested-commit blob, binds the canonical Git executable and digest, rejects lossy retained path or argument conversion, and binds the exact supervisor and workload command tuples plus the evidence executable digest. Artifact validation independently recomputes count, publication, allocator, deterministic completed-batch ordering, lifecycle throughput, persistent/transient storage, logical index bytes, warm-query result counts/digests/percentiles, integrity, process sampling/output/teardown, resource-gate, profile/plan, and command/source invariants rather than trusting retained pass summaries. ARRI 4.23 obtains actual query-plan rows from DB-owned plan operations that reuse the same private production SQL constants as `AtlasStore::load_graph_entity` and `AtlasStore::load_graph_adjacency`, then times those production DB and `bounded_three_hop` service operations without copying SQL into the evaluator or adding a DB-pass-through service wrapper. The retained graph digests are named for their actual entity identity, relation topology, and evidence occurrence-identity coverage instead of implying a digest over every persisted semantic column. The ordinary reduced storage/query test exercises the same production boundaries, but reduced output is explicitly exploratory and cannot serialize `passed` or satisfy the task. The authoritative task-verification command uses one all-target Cargo filter with `--include-ignored`: it executes the registered production 1/N-worker parser-order assertion and the ignored release-mode driver that retains the clean-commit exact million-entity/three-million-relation artifact plus a create-new external SHA-256 receipt.

ARRI 4.23 retains two instrumentation dependencies at the outer CLI evidence boundary after the following dependency review. Neither dependency is reachable through a normal or build dependency edge, so normal ProjectAtlas builds, installation, startup, scans, queries, and MCP operation do not compile, link, load, or execute them. Their costs apply to the lock/source cache, all-target CI, the evidence-executable build, and instrumented evidence runs only.

| Review area | Recorded evidence and disposition |
| --- | --- |
| Ownership and features | `stats_alloc 0.1.10` and `sysinfo 0.38.4` are `projectatlas-cli` dev-dependencies used only by `graph-scale-evidence-runner`. `stats_alloc` has an empty default feature set and its `nightly` feature is disabled. `sysinfo` uses `default-features = false` with only `system`; default component, disk, network, user, multithread, and serde features stay disabled. Its selected target closure still includes Windows bindings and `objc2-io-kit`, which is an all-target compiler/source-cache cost rather than a shipped-runtime edge. |
| Maintenance and instrumentation risk | `stats_alloc` is MIT-licensed, not yanked, has no declared MSRV, and was published 2022-03-30; its dormant maintenance and sequentially consistent atomic instrumentation overhead are accepted only for release evidence, not product execution. `sysinfo` is MIT-licensed, not yanked, was published 2026-03-09, declares Rust 1.88, and was actively maintained; selected `0.38.4` trails the observed `0.39.6`, so upgrades remain ordinary reviewed dependency work rather than evidence-scope churn. |
| Integrity and supply chain | Locked checksums are `5c0e04424e733e69714ca1bbb9204c1a57f09f5493439520f9f68c132ad25eec` for `stats_alloc` and `92ab6a2f8bfe508deb3c6406578252e491d299cbbf3bc0529ecc3313aee4a52f` for `sysinfo`. Both licenses are MIT. `cargo audit --file Cargo.lock --deny warnings` passed for 305 locked packages; `cargo deny check` passed advisories, sources, licenses, and bans, with only configured duplicate warnings. These gates must be rerun after lock changes. |
| Registry and all-target cache cost | Direct registry archives/unpacked sources/files are 4,476 B/15,853 B/12 for `stats_alloc` and 234,134 B/1,086,452 B/150 for `sysinfo`. The ten newly locked packages total 10,220,657 B compressed, 120,596,793 B unpacked, and 1,069 source files; generated `windows 0.62.2` accounts for 9,360,572 B compressed and 114,212,926 B unpacked. These figures are registry/compiler-cache costs, not ProjectAtlas product-package sizes. |
| Clean evidence build | An offline, locked, release build of `graph-scale-evidence-runner` in an empty external target directory took 101.685 s wall time with 16 jobs, 0 fresh and 366 compiled units. The executable was 11,491,840 B; the target directory was 1,146,665,662 B across 2,740 files. Cargo recorded 0.57 unit-seconds for `stats_alloc`, 1.56 for `sysinfo`, and 46.78 overlapping unit-seconds across the new Windows package units. Concurrent unit durations are not wall time, and no dependency-free counterfactual was built, so these measurements do not claim exact incremental time or executable-byte deltas. |
| Reduced runtime observation | The real reduced path completed in 4.714 s as `exploratory_completed` with `claim_eligible = false`. Across 50 logical facts it observed 51,544 allocations, 51,391 deallocations, 4,016 reallocations, 55,560 allocator requests, 4,806,360 requested bytes, 4,756,126 deallocated bytes, and a 1,081,223 B reallocation delta; SQLite/native allocation remains explicitly unmeasured by the Rust global allocator. At the configured 100 ms interval, two complete Windows Job Object membership samples at 22,077,200 ns and 142,276,500 ns observed 21,278,720 B peak aggregate RSS with no membership or active-member discovery failures. This proves both instrumentation paths execute, but does not certify the million-entity ARRI 4.23 contract. |
| Rejected alternatives | A ProjectAtlas-owned `GlobalAlloc` wrapper would introduce forbidden ProjectAtlas-owned unsafe code because the standard library exposes no safe allocation-request counter. Existing `processkit::stats` reports materially different peak/committed or per-member-peak semantics and lacks portable concurrent sampled aggregate RSS for the complete process group. `sysinfo` therefore supplies current per-process RSS/start identity while `processkit::ProcessGroup::members()` retains containment and lifecycle ownership; neither dependency becomes a product abstraction. |

The primary speed advantage for normal development comes from doing less work: content/state coalescing, dependency-driven invalidation, changed-row transactions, and concurrent queries against the active slot/epoch. Full scans retain parser reuse, bounded Rayon parallelism, project-wide candidate indexes, and batched writes, but memory remains bounded rather than requiring an unconstrained whole-repository graph in RAM.

Every optimization must preserve stable identities, coverage reconciliation, deterministic 1/N-worker output, and per-language graph correctness. Phase 0 freezes the profile schema and validates its instrumentation with local feasibility labs; Section 11 records the eligible allocation, complete-process-tree memory, write, and query profiles after the relevant implementation stabilizes, then compares each optimization against the last correct baseline and the pinned comparison baseline.

**Alternatives considered:** rely on Rust alone; hold an unconstrained generic graph in RAM; copy a handwritten SQLite page writer; trade correctness/coverage for throughput. Rejected because none provides a maintainable or truthful superiority result.

### Decision 16: Give Plugin Installation One Managed Lifecycle Owner

Phase 0 evaluates the documented official plugin-store and host lifecycle surfaces and records any missing runtime-provisioning capability as a typed release blocker. Real clean-host proof belongs to the packaged platform gates after the managed lifecycle exists; absence of early packaged evidence is never converted into a passing claim or a hidden manual installer step. The runtime-side ProjectAtlas host-lifecycle service owns a typed plan, managed-artifact journal, apply/verify sequence, compensating rollback, last-known-good recovery, and preservation policy across runtime, skill, plugin, and MCP registration. Shell and PowerShell remain thin platform bootstrap/download/launch adapters; they do not independently own version selection, host reconciliation, repair, or rollback policy.

Filesystem, plugin-manager, and MCP-registry updates are not one atomic transaction. The implementation therefore records every intended and completed managed action, verifies the resulting runtime and real `atlas_init`/overview path, and either restores the last-known-good managed state or fails closed with a precise recovery command. Project-local databases and authored purpose/settings/telemetry are never managed installation artifacts and survive update, rollback, repair, removal, and reinstall by default.

**Alternative considered:** continue expanding independent PowerShell and shell policy or call the cross-system sequence transactional. Rejected because duplicated lifecycle rules drift and the participating host/filesystem registries do not provide one atomic transaction.

### Decision 17: Stabilize Product Behavior Before Repository-Wide Quality Closure

Sections that implement repository-intelligence architecture and functionality run before the separately mapped Rust test-quality program performs whole-repository saturation. Each stable implementation slice still includes the smallest focused owning-logic unit test and every integration, CLI/MCP E2E, packaged, platform, property, fuzz, or benchmark layer required by its actual risk. Existing format, check, strict Clippy, test, doctest, rustdoc, source-lint, and compatibility gates remain active throughout implementation.

At an implementation-phase boundary, scoped local gates run first. The phase is not complete until the same commit has a successful applicable GitHub Actions checkpoint covering hosted unit, integration, real E2E, packaged-smoke, and platform jobs, with the commit SHA and run URL visible in issue #308. This avoids publishing obviously broken checkpoints while still detecting hosted-platform and clean-environment drift before the next phase. Repository-wide coverage saturation and the complete source-mutation campaign remain final post-stabilization work.

Coverage evidence therefore carries one closed enforcement mode. `implementation_checkpoint` always validates the pinned LLVM export and tool identity, selected root and source confinement, per-file/aggregate count reconciliation, nonempty applicable counts, exact exception policy, and any established platform floor, but it does not claim the final v0.4 percentages. `release_quality` adds every agreed raw and adjusted target. Omitted CLI enforcement remains strict `release_quality`; the retained coverage result and validator summary bind the selected mode; release aggregation rejects `implementation_checkpoint` evidence; and the reusable release caller must request `release-quality` explicitly. Ordinary push, pull-request, and manual implementation checkpoints cannot silently become release evidence.

After the architecture, migrations, public contracts, and feature behavior have stabilized and no planned broad refactor would invalidate a saturation pass, the mapped quality change audits legacy and refactored code, closes the agreed adjusted-coverage targets, runs the complete source-mutation campaign, and records final commit-bound evidence. That change is a hard v0.4 release prerequisite, not a prerequisite to begin or continue feature implementation. Tasks remain unchecked and no implementation PR is proposed for review until their required evidence exists.

**Alternative considered:** require the complete repository-wide coverage and mutation program before feature implementation. Rejected because planned ownership splits and schema/service refactoring would invalidate much of that work without improving the correctness of the feature slices; focused tests at the point of behavior change provide the immediate regression protection.

## Target Runtime Architecture

```text
existing CLI / MCP names
          |
          v
thin typed adapters
          |
          v
projectatlas-service
  navigation/ranking/search/summary/slice
  relations/architecture/impact/trace/federation
          |
          +-------------------------------+
          |                               |
          v                               v
projectatlas-db                      index coordinator
  active slot / epoch                  |
  lexical / conditional FTS            v
  authored purposes/telemetry       supervised worker
          ^                         discovery + extraction
          |                         project registries
          |                         semantic providers
          |                         gated enrichments
          |                               |
          +---- validate + atomic publish-+

optional broad grammar pack -> supervised pack-process boundary
optional semantic pack      -> supervised ANN/model process boundary
explicit call roots         -> read-only/query-only federation boundary

official plugin-store action -> thin platform bootstrap -> typed host-lifecycle plan/journal -> verified runtime + skill + MCP registration
```

### Pipeline Stages

1. **Select and verify project:** canonical root, DB/config binding, ignore policy, registry/model capabilities, compiler-metadata identity where consumed, and the typed effective resource envelope after host/container/job clamps.
2. **Discover and fingerprint:** dynamic `.gitignore`, atlas ignores, BLAKE3 identity, language detection, Git/content state signature.
3. **Plan:** full staging-database rebuild or active-slot incremental delta; parser/provider/config invalidation; dependent files/global enrichments.
4. **Extract structure:** repository/folder/file/package facts and exact coverage rows.
5. **Extract syntax:** bounded parallel parsers emit normalized facts and parse/error spans.
6. **Build registries:** packages/modules/scopes/definitions plus inverted candidate indexes.
7. **Resolve:** provider-owned import/export/call/reference/type outcomes with evidence and ambiguity.
8. **Enrich:** independently toggled structural tests, routes/services, config/env, IaC/K8s, history, and similarity candidates.
9. **Index retrieval:** always build deterministic lexical state; populate conditional file/symbol FTS for the same staging snapshot only when preflight allows it. Optional semantic maintenance is a separate pack task bound after structural publication.
10. **Reconcile:** counts, coverage, stable-key uniqueness, edge endpoints, UTF-8, schema, integrity, deterministic order.
11. **Publish:** parent imports the validated full snapshot into the inactive slot and atomically flips `(active_slot, active_epoch)`, or transactionally updates the active slot and advances its epoch for an incremental delta.
12. **Maintain:** preserve the prior slot as bounded rollback state, report semantic-pack freshness/tasks separately, and emit telemetry/task results. Call-only federation has no persistent cache or maintenance job.

### All-Surface Compatibility Contract

Phase 0 SHALL generate a complete compatibility manifest from the registered MCP tool inventory and CLI command tree, then replay a golden request/response/error corpus against every entry. The groups below organize that manifest; they do not exempt an omitted alias or command. For each existing MCP tool and every CLI equivalent, the contract freezes names and aliases, arguments/request schemas, defaults, root selection and `project_path` isolation, output formats, boundedness/pagination, stdout/stderr placement, exit status or typed MCP error behavior, task semantics, and current startup/navigation call flows. Additive fields must remain deserializable by recorded clients, and graph enrichment cannot require an extra call.

The release replay uses the actually installed/packaged v0.4 executable, real CLI child processes, real stdio MCP transport, and project-local SQLite databases created by indexing real repositories. The frozen inventory currently contains 41 CLI paths and 40 MCP tools; the gate derives those identities from the manifest, covers every later-added surface, and exercises normal workflows, failures, limits, cancellation, project routing/isolation, freshness, automatic graph behavior, and TOON/JSON compatibility. Unit-only, mock-transport, in-memory, or prebuilt-database evidence cannot satisfy this contract.

| Existing surface family | Automatic enhancement | Compatibility rule |
|---|---|---|
| `atlas_init`, `atlas_config`, `atlas_root`, `atlas_root_set`, `atlas_set_project_path` | Graph defaults and root/schema capability preflight where applicable | Existing initialization, config precedence, project-local DB ownership, and root-selection behavior remain unchanged. |
| `atlas_ignore_list`, `atlas_ignore_init_gitignore`, `atlas_ignore_add`, `atlas_ignore_remove`, `atlas_lint`, `atlas_map` | Capability-aware diagnostics and graph health where additive | Dynamic `.gitignore` inheritance, stricter atlas-only ignores, purpose levels, report modes, and existing exit/error contracts remain authoritative. |
| `atlas_runtime_info`, `atlas_mcp_config`, `atlas_settings` | Default-core budgets; graph schema, accepted capabilities/relation families, backend, slot/epoch, and semantic-pack state | Existing fields remain; reporting is additive and bounded. No separate schema/capability tool is introduced. |
| `atlas_reset_index`, `atlas_strip_legacy_purpose`, and registered aliases/legacy entries | Slot-aware derived-data reset where relevant | Existing names, preview/confirmation/safety behavior, authored-data preservation, formats, and errors remain exact; legacy entries are tested, not silently dropped. |
| `atlas_scan`, `atlas_symbols_build`, `atlas_watch_once`, `atlas_watch_status`, `atlas_task_status`, `atlas_task_cancel` | Staged full publication, active-slot deltas, coverage, and separate optional-pack tasks | Existing orchestration and progress/cancellation contracts remain; structural completion does not wait for an optional semantic task unless explicitly required. |
| `atlas_overview`, `atlas_folders`, `atlas_files`, `atlas_next` | Slot/epoch and bounded graph/capability digests plus graph-aware ranking reasons | No new required argument or workflow call; exact path and purpose behavior wins over inferred signals. |
| `atlas_file_summary`, `atlas_outline`, `atlas_search`, `atlas_slice`, `atlas_symbols`, `atlas_symbol_relations` | Richer coverage, stable identities, conditional FTS, relation filters, evidence, and bound cursors | Old requests and defaults retain their core shape and meaning; literal/regex/fuzzy completeness and repository-relative source safety remain exact. |
| `atlas_health`, `atlas_health_resolve`, `atlas_purpose_queue`, `atlas_purpose_set`, `atlas_purpose_review` | Graph-aware diagnostics and relation-family purpose context where bounded | Existing purpose text, review state, ordering, levels, and authored persistence are never treated as disposable derived data. |
| `atlas_token_report`, `atlas_parity_report`, `atlas_session_brief` | Graph-aware reads avoided, accepted-capability status, and bounded next-step hints | Existing accounting definitions, response formats, and call sequences remain compatible; new measurements are separately labeled. |
| New `atlas_architecture`, `atlas_impact`, `atlas_trace` plus CLI equivalents | Bounded facets, evidence-backed affected identities, and node-simple paths | Typed filters, cursor bindings, and deterministic limits apply; there is no Cypher surface or unsupported risk claim. |

## Phased Delivery

Phase 0 ends with a no-claim implementation-readiness scoreboard that reports frozen contracts, decision-enabling local feasibility, and every unresolved release blocker. Each implementation phase ends with a one-page generated scoreboard rather than a prose-only status. Those scoreboards report accepted-capability correctness, full/incremental performance, RSS/writes/bytes, query latency, agent quality/tokens/reads/calls, production code/dependencies, and public tool growth. Reviewers must be able to answer: what became measurably better, what complexity was added, what was deleted or deferred, and whether the next phase is still the smallest releasable step. A feature that cannot justify its cost remains optional/off or is removed. Focused tests travel with each stable phase behavior; after local gates pass, a commit-bound hosted GitHub Actions checkpoint and its issue-linked run evidence close the phase. The mapped repository-wide coverage/mutation program starts only after the product architecture and feature phases stabilize.

| Phase | Deliverable | Gate to continue |
|---|---|---|
| 0. Truth and baselines | Generated architecture/capability inventory, pinned corpus and prior-release identities, preregistered benchmark/calibration contracts, naming/independence rules, local SQLite/FTS architecture-selection labs, parser/semantic candidate gates, plugin-host feasibility, lifecycle ownership, preliminary native/FFI inventory | Reproducible implementation-readiness contracts and recorded architecture decisions; full calibrated pinned campaigns remain in Section 11; no implementation or release claim. |
| 1. Storage and lexical foundation | First-touch module extraction where needed, ledger-assigned transactional migrations, stable IDs, typed evidence/resolution/coverage, conditional file/symbol FTS, compatibility adapters | Generated all-surface golden tests, migration/purpose preservation, lexical equivalence with and without FTS, no performance regression. |
| 2. Broad structural intelligence | Generated registry, broad pack, normalized extraction/embedded parsing, package/manifests, non-vacuous fixtures | Every accepted capability passes its declared tier; platform shards; no phantom/docs/SBOM drift. |
| 3. Semantic graph and incrementality | Project registries, accepted semantic providers, inbound invalidation, supervised worker, full/incremental equivalence | Per-language precision/recall, 1/N determinism, mutation equivalence, Unicode/crash/resource gates. |
| 4. Agent analysis | Automatic graph ranking/summary digest, enriched relations, architecture/impact/trace | Old-call compatibility, bounded traversal, agent workflow same/fewer calls/tokens, query latency gates. |
| 5. Typed enrichments and federation | Routes/services, tests, config/env, IaC/K8s, history, explicit-root cross-repository matching | Per-protocol positive/negative metrics, root isolation, freshness, no false global writes. |
| 6. Optional semantic retrieval/snapshots | Evaluated ANN/model pack, clones if useful, safe compressed graph artifacts | Labeled IR/clone value, deterministic updates, resource/package/privacy/integrity gates. |
| 7. Plugin-store lifecycle | One-action clean-host install, typed managed-artifact journal, update/repair/rollback/remove/reinstall, thin platform bootstraps | Real packaged E2E on every supported host, last-known-good recovery, project data preserved, no hidden manual configuration. |
| 8. Functional stabilization and evaluation | Full paired-baseline evaluation, docs/support matrix, SBOM/attestation, rollback rehearsal, resolved architecture/API/storage/security/performance/KISS reviews | All repository-intelligence behavior and focused risk-based tests pass; interfaces, migrations, and ownership boundaries are stable with no planned broad refactor. |
| 9. Repository-wide quality closure and release | Legacy/refactored test saturation plus the mapped nextest, doctest, adjusted-coverage, full source-mutation, IssueOps, and commit-bound release-evidence program | The mapped Rust test-quality change is complete on the stabilized release commit; every normative acceptance gate passes and public claims are generated from eligible artifacts. |

## Risks / Trade-offs

### Pre-Mortem

Assume the program failed after substantial engineering effort. The most likely causes and prevention evidence are:

| ID | Failure mode | Early warning | Mitigation actions / required evidence | Mitigation task IDs |
|---|---|---|---|---|
| PM-01 | Language count becomes marketing rather than capability | Registry count differs from tests/docs; zero-definition fixtures pass | Generate one registry-backed manifest, require non-vacuous tier fixtures, and fail documentation/SBOM drift. | `ARRI-5.1`, `ARRI-5.2`, `ARRI-5.3`, `ARRI-5.12`, `ARRI-5.13`, `ARRI-5.18` |
| PM-02 | Broad grammar support bloats builds/releases | CI compile time, binary/install bytes, cold startup, idle RSS, or no-pack scan RSS grow rapidly | Keep broad grammars outside default core and enforce dependency, binary, install, startup, RSS, and scan ceilings with packaged evidence. | `ARRI-2.9`, `ARRI-2.14`, `ARRI-5.6`, `ARRI-5.7`, `ARRI-5.8`, `ARRI-5.14` |
| PM-03 | “Rust-native” is used to hide generated/native dependencies | SBOM lacks parser/runtime components or safety claims ignore native/FFI code | Inventory provenance, SBOM, unsafe/native/FFI/dynamic-library/advisory boundaries, containment, and no-runtime-compilation behavior. | `ARRI-2.28`, `ARRI-5.16`, `ARRI-11.4`, `ARRI-11.28` |
| PM-04 | Semantic providers become another 40K-line monolith | Shared interface accumulates language branches and pseudo-types | Keep provider-local modules, prefer canonical crates, measure providers independently, and abstain instead of emulating compilers. | `ARRI-6.1`, `ARRI-6.14`, `ARRI-6.15`, `ARRI-6.17` |
| PM-05 | Typed graph becomes speculative taxonomy | Many unused kinds/properties; migrations outpace queries | Require each kind to have an owner, producer, consumer, fixture, persisted trace row, and real query use. | `ARRI-4.4`, `ARRI-4.5`, `ARRI-6.16` |
| PM-06 | Stable IDs churn on formatting/line movement | Full scan changes identities for unchanged qualified symbols | Bind stable-key encoding, exclude incidental line movement, and run identity and canonical mutation fixtures. | `ARRI-4.2`, `ARRI-4.3`, `ARRI-7.8`, `ARRI-7.9` |
| PM-07 | Full and incremental graphs diverge | Mutation corpus mismatch or stale inbound edges | Re-resolve inbound dependents and require every mutation sequence to converge to the clean canonical graph. | `ARRI-7.4`, `ARRI-7.5`, `ARRI-7.8`, `ARRI-7.9` |
| PM-08 | Incremental work still rewrites the graph | DB/WAL bytes scale with whole repo for one-file change | Mutate only affected rows, measure write amplification, and coalesce unchanged dirty state. | `ARRI-4.14`, `ARRI-7.7`, `ARRI-7.18`, `ARRI-11.6` |
| PM-09 | Failed scan replaces valid data | Queries see partial counts after timeout/crash | Keep workers away from activation, reconcile staging into the inactive slot, flip atomically, and serve the last valid slot during failure. | `ARRI-4.12`, `ARRI-4.13`, `ARRI-7.6`, `ARRI-7.13`, `ARRI-7.17` |
| PM-10 | SQLite corruption becomes plausible empty output | Count/query contradictions or swallowed terminal step status | Centralize terminal-status propagation and fail loudly on corruption, partial iteration, or integrity contradictions. | `ARRI-4.15`, `ARRI-4.16`, `ARRI-4.17`, `ARRI-4.19` |
| PM-11 | Migration loses purposes or telemetry | Rebuilt DB passes graph tests but authored rows disappear | Preflight and ledger migrations, keep authored tables outside derived slots, reconcile before/after state, and rehearse rollback. | `ARRI-4.6`, `ARRI-4.7`, `ARRI-4.9`, `ARRI-4.18`, `ARRI-4.20`, `ARRI-10.15`, `ARRI-11.11` |
| PM-12 | Parallel output is nondeterministic | Stable checkout yields changing semantic/similarity edges | Sort inputs/candidates/ties, seed algorithms, and compare one worker, N workers, and repeated canonical runs. | `ARRI-6.2`, `ARRI-7.11` |
| PM-13 | Unicode works in unit tests but fails in worker/release | Windows/CJK subprocess or truncation regressions | Exercise UTF-8 boundaries, normalization, long paths, and packaged real-process behavior on every release platform. | `ARRI-8.16`, `ARRI-11.1`, `ARRI-11.23` |
| PM-14 | FTS introduces false negatives | Literal results differ for punctuation/short/case queries | Use FTS only for candidates, exact-verify results, fall back by mode, and run differential completeness tests. | `ARRI-2.12`, `ARRI-4.8`, `ARRI-8.1`, `ARRI-8.2`, `ARRI-8.3` |
| PM-15 | Graph ranking buries exact paths | Related but weak matches outrank explicit file/path query | Keep exact path/name priority invariant, emit reason codes, and gate deterministic top-five navigation. | `ARRI-8.7`, `ARRI-8.8` |
| PM-16 | Existing MCP clients break or require more calls | Schema deserialization/golden flow failures | Replay the frozen public inventory and require every normal workflow to preserve names, payloads, defaults, calls, reads, and token budgets. | `ARRI-2.4`, `ARRI-8.17`, `ARRI-8.18`, `ARRI-8.19`, `ARRI-11.12` |
| PM-17 | Summary responses become graph dumps | Default TOON grows with edge count | Cap response/workflow enrichment and expose bounded counters plus next-call hints instead of default edge dumps. | `ARRI-8.9`, `ARRI-8.19` |
| PM-18 | Traversal becomes a denial-of-service vector | Cyclic fixture consumes client timeout/RSS | Enforce node-simple traversal, typed depth/node/edge/time/memory/output/cancellation budgets, and adversarial cycle tests. | `ARRI-8.13`, `ARRI-8.14`, `ARRI-8.15`, `ARRI-11.2` |
| PM-19 | Dead-code analysis causes unsafe deletion | Framework/dynamic symbol is marked certain | Emit candidates only, cover dynamic/framework/unresolved states, require precision evidence, and never auto-delete. | `ARRI-8.22`, `ARRI-8.23` |
| PM-20 | Route/service graph overfits strings | CI paths, regex, docs create endpoints | Use protocol-aware typed extractors, adversarial negative fixtures, traceability, and per-relation accuracy gates. | `ARRI-9.3`, `ARRI-9.4`, `ARRI-9.8`, `ARRI-9.17` |
| PM-21 | Cross-repository identities miss across languages | gRPC/package prefixes do not rendezvous | Normalize protocol/package identities while retaining original evidence and prefer ambiguity over guessed exact matches. | `ARRI-9.4`, `ARRI-9.12`, `ARRI-9.15`, `ARRI-9.17` |
| PM-22 | Federation leaks, mutates, or partially omits a requested root | Same relative paths/root mismatch cause wrong reads/writes or plausible partial answers | Require explicit ordered roots, validate all read-only participants before execution, fail the whole call, and discard cross-root state. | `ARRI-9.9`, `ARRI-9.10`, `ARRI-9.11`, `ARRI-9.12`, `ARRI-9.13`, `ARRI-9.14` |
| PM-23 | Optional semantic search adds cost but no value | MRR/nDCG flat; RSS/package/query cost high | Keep the feature disabled until labeled value/resource gates pass and preserve clean removal plus lexical/graph fallback. | `ARRI-2.15`, `ARRI-10.3`, `ARRI-10.4`, `ARRI-10.9`, `ARRI-10.11` |
| PM-24 | Model/vector results are unstable | Repeated runs change top-k/edges | Version preprocessing/model inputs, fix ANN seeds/ties, enforce numeric tolerances, and compare incremental/full membership and retrieval quality. | `ARRI-10.5`, `ARRI-10.6`, `ARRI-10.7` |
| PM-25 | Snapshot sharing serves torn/stale data | Imported counts differ or dirty commit appears clean | Export from a coherent SQLite snapshot, bind digest and generation metadata, validate on a temporary path, and activate atomically. | `ARRI-10.12`, `ARRI-10.13`, `ARRI-10.14`, `ARRI-10.15` |
| PM-26 | Supply-chain check “passes” while checking zero files | Missing/absolute manifest paths only warn | Require a nonzero expected inventory and fail modified, missing, extra, absolute, stale, advisory, license, or attestation evidence. | `ARRI-5.16`, `ARRI-11.4`, `ARRI-11.18` |
| PM-27 | Benchmarks are cherry-picked | One run, unpinned repos, suppressed errors, aggregate hides weak language | Pin inputs and commands, retain failures/raw samples, use paired repetitions, and decide every language/resource dimension independently. | `ARRI-2.5`, `ARRI-2.10`, `ARRI-11.5`, `ARRI-11.6` |
| PM-28 | New source looks mechanically mirror-shaped | Similar module/function/pass/test layout appears without ProjectAtlas ownership reasons | Use need-driven ProjectAtlas ownership, independent naming and mechanisms, source-shape review, and architecture/KISS gates. | `ARRI-3.1`, `ARRI-3.9`, `ARRI-11.25`, `ARRI-11.26`, `ARRI-11.29` |
| PM-29 | Scope never ships | All phases coupled to embeddings/UI/all languages at once | Enforce independent phase exits, ship structural/lexical value first, and keep semantic/UI work optional until its own evidence passes. | `ARRI-2.18`, `ARRI-2.19`, `ARRI-2.20`, `ARRI-10.1`, `ARRI-11.31` |

### Trade-Offs

- A typed graph requires more deliberate migrations and adapter code than string labels, but pays back in correctness, query speed, schema validation, and maintainability.
- A separate full-scan staging database plus exactly two live derived-data slots temporarily increases disk usage. This is accepted within a configured multiplier because it protects the active and immediately prior full publication; cleanup and free-space preflight are mandatory.
- Explicit ambiguity/unresolved outcomes may reduce apparent recall compared with first-name matching. Precision and honest abstention are more valuable for agent decisions.
- Broad grammar packs increase supply-chain work. Pack separation and a generated registry make that cost visible and controllable.
- WASM parser isolation may be slower than native grammar artifacts. Phase 0 freezes the candidates and fail-closed decision contract; representative parser implementation and the Section 11 campaign decide the shipping host, and no purity claim overrides the performance/correctness gate.
- Optional real ANN/model support introduces dependency and package complexity. It remains absent unless labeled evaluation proves agent value beyond lexical/graph retrieval.

## Migration Plan

1. Record current schema, scan/search/ranking/tool golden outputs, package sizes, purposes, telemetry, and full/incremental snapshots.
2. Before any migration DDL or writable database open, run a read-only preflight that validates canonical root/database binding, current schema support, runtime compatibility, SQLite `quick_check`/required integrity checks, required free disk, backup feasibility and digest plan, and compiled/runtime FTS support when an FTS migration is proposed. Any failed or unknown check leaves the database byte-for-byte untouched and returns an actionable error.
3. Assign ordered migration IDs and immutable checksums from the repository migration ledger when implementation lands; this design does not reserve numeric schema versions. The runtime refuses an unknown future migration, missing predecessor, reordered ID, or checksum mismatch.
4. Create and verify a consistent backup after preflight and before the first writable open. Apply each ledger migration transactionally, run table/count/authored-row/root/integrity reconciliation, and record that migration's ledger row only after reconciliation passes in the same transaction.
5. Add stable identity and typed resolution/evidence/coverage storage while retaining current name/path response fields and compatibility readers. Extract touched storage modules only when needed by this work.
6. Add exactly two derived-data slots plus `(active_slot, active_epoch)` metadata. Reconcile existing derived rows into one active slot and leave the other available for the first full publication; authored purpose/review/settings/telemetry tables remain outside both slots.
7. Add conditional FTS acceleration only through a ledger migration whose preflight succeeds on the selected build. Keep deterministic lexical search complete on databases/builds without FTS, and prove with/without-FTS result equivalence before recording readiness.
8. Introduce the generated registry behind the current parser adapter; prove every accepted existing capability before enabling a broad pack.
9. Add structural adapters and provider/enrichment phases behind visible capabilities/config; publish only completed tiers.
10. Add automatic graph ranking/digests to existing services, then the three analysis tools, with generated all-surface compatibility and bounded-query tests.
11. Add call-only federation and optional semantic/snapshot packs only after their prerequisite gates and independent lifecycle/resource tests pass.
12. Rehearse every ledger path against verified copies of databases from each supported prior migration state and release platform; never use a user's only database for rehearsal. Generate support matrices and claims from validated artifacts and rehearse rollback before release.
13. Once architecture, migrations, public contracts, and feature behavior are stable, execute the separately mapped repository-wide Rust quality change: saturate legacy/refactored coverage, meet every adjusted target, run the complete source-mutation campaign, reconcile task evidence, and block v0.4 until that final evidence passes on the release commit.

### Rollback

- Before each schema migration, create a consistent local backup subject to disk preflight and digest verification.
- A migration failure rolls back the SQLite transaction and leaves the prior runtime/index usable.
- Full staging, import, or reconciliation failure never changes `(active_slot, active_epoch)`. After a successful full flip, the retained previous slot may be reactivated with a new atomic epoch only while it remains intact and schema-compatible; the next full publication may reuse it, so no arbitrary publication history is promised.
- Incremental publication is one transaction; failure rolls it back. Post-release rollback restores the compatible backup or forces a derived-graph rebuild while importing/reconciling authored purpose/telemetry/settings.
- Optional grammar/model packs can be disabled/removed independently; core parser/search behavior remains available and capability output updates honestly.
- New MCP tools are additive. If a new analysis path is faulty, it can be disabled without renaming/removing existing calls or changing the index ownership model.

## Acceptance Gate Summary

The detailed normative gates live in the seven capability specs. Program completion requires at minimum:

- Every accepted capability independently passes its declared tier, compatibility, provenance/SBOM, and per-capability correctness/coverage gate; raw language/parser/tool counts and aggregate averages cannot hide a failure.
- Core extraction precision is at least 95% and recall at least 90%; semantic-edge precision is at least 90% and recall at least 80% for every accepted benchmarked language/relation family, with no aggregate masking.
- The exact two-slot/epoch model passes canonical full/incremental and 1/N-worker equivalence, atomic flip/delta/rollback tests, ten-run deterministic enrichment checks, and Unicode real-process tests on Windows/macOS/Linux.
- The generated manifest proves compatibility for every registered CLI command, MCP tool, alias, format, default, root-isolation rule, exit/error contract, task behavior, and recorded workflow; no existing workflow gains a required call and no more than three analysis tools are added.
- Read-only migration preflight, ledger ID/checksum validation, transactional reconciliation-before-recording, loud SQLite/integrity failures, safe snapshots, and authored metadata preservation pass against every supported prior migration state.
- The default core stays at or below 48 MiB stripped executable, 64 MiB installed payload, 250 ms cold `runtime-info` p95, 500 ms cold MCP-ready p95, 96 MiB idle MCP RSS, and 512 MiB no-pack large-corpus scan peak RSS. Broad grammar/WASM/ANN/model runtimes are absent from its dependency graph and initialization path.
- Warm SQLite/service p95 has 1 ms/50 ms reference-host goals and warm end-to-end MCP p95 has 50 ms/150 ms reference-host goals. An eligible pinned runner class, calibration envelope, and pre-result tolerance factor no greater than 1.25 make “same vicinity” mechanical; raw and paired same-host results remain visible and separate from cold startup.
- Statistical decisions follow Decision 13: accepted capabilities cannot be pooled away; performance requires every hard budget, paired geometric-mean time and peak-RSS ratio upper bounds at most 0.80, and every required-corpus runtime-ratio upper bound at most 1.10; agent-quality superiority always requires the paired five-point/corrected-confidence rule, while a high-baseline absolute-quality result is reported separately.
- The blinded agent-experience gate records time and calls to first useful context, wrong or redundant selections, backtracking caused by unclear or untrusted output, next-action usefulness, evidence traceability, task completion, and paired workflow preference; no dimension may regress, correctness remains mandatory, and an independent agent-workflow review must conclude that ProjectAtlas is the preferred first repository tool before v0.4 closes.
- Deterministic lexical retrieval remains complete with FTS disabled or unsupported; optional semantic capability passes its full install-to-removal state machine, and vector outputs use declared absolute/relative tolerances plus frozen top-k/recall gates rather than bit equality.
- Node-simple traversal, cursor bindings, cancellation, and call-only explicit-root read-only/query-only fail-closed federation pass negative and concurrency tests. No implicit network, repository-code, grammar, model, or federation background execution occurs.

### Test Strategy And Task Traceability

Create a version-controlled verification matrix keyed by every OpenSpec task ID. Phase 0 defines its schema, risk classification, ownership, and required evidence layers; exact commands, assertions, covered inputs, and successful runs are populated as stable behaviors land and are finalized during release closure. The evidence infrastructure is not a prerequisite to writing feature code, but no task may be checked and no implementation PR may be proposed for review without the required current evidence. Each row names the requirement/scenario, owner, changed artifacts, highest risk class, applicable layers, justified inapplicable layers, timeout, and result artifact. Evidence is risk-based and cumulative where the affected boundary requires it:

- **L1 - pure/domain:** unit tests and property tests for parsing, typed state, identity, resolution, ranking, serialization, scoring, limits, and deterministic algorithms.
- **L2 - persistence/cross-crate:** applicable L1 evidence plus integration tests for SQLite transactions/migrations/slots, worker protocols, service/database joins, full/incremental equivalence, pack adapters, snapshots, and federation.
- **L3 - public/concurrency:** applicable L1/L2 evidence plus real CLI and MCP E2E for public request/response/error contracts, old-client compatibility, cancellation, cursor staleness, process supervision, and concurrent reads/tasks.
- **L4 - migration/package/security/platform:** all relevant preceding layers plus packaged-binary/pack smoke flows, supported-platform shards, corruption/adversarial/fuzz or mutation evidence, and benchmark/resource gates appropriate to the change.

A narrow task does not acquire unrelated integration, E2E, packaged-smoke, platform, benchmark, property, or fuzz layers merely because it changes runtime code, but every omitted layer requires a reviewed matrix justification. Every task, including planning, documentation, benchmark-policy, and GitHub-only work, still owns at least one task-specific automated unit-level test: runtime tasks name a focused unit test for the owning logic, while non-runtime tasks name a focused validator test that asserts the promised schema, artifact, drift, reproduction, or policy behavior. Packaged smoke tests verify a small real repository flow, not only `--help` or exit zero.

Focused tests for a new stable behavior run in the same implementation slice. The final quality phase then audits the stabilized repository as a whole, fills legacy/refactored gaps, proves the agreed adjusted coverage targets, and runs the complete mutation inventory; it does not replace or retroactively postpone those feature tests.

The machine-readable task-evidence record contains the OpenSpec task ID, unit-test ID, exact bounded command, asserted behavior, commit SHA, successful run URL or retained artifact identity, completion state, and any additional risk-required layers. GitHub IssueOps renders or links that evidence beside the corresponding task in an umbrella or bounded phase issue, and the authoritative issue map prevents duplicated or missing ownership when GitHub body limits require multiple issues. The task checklist/IssueOps validator SHALL reject a checked local or GitHub task whose unit test or successful run is missing, failing, skipped, flaky, timed out, stale for the checked commit, or vacuous; it also rejects local/GitHub state drift and a PR proposed for review while mapped tasks or evidence remain incomplete. Each command has an explicit timeout and preserves stdout/stderr plus machine-readable results; placeholder evidence is prohibited.

Repository instructions plus the validated global Rust skill are the agent-orchestration contract for this program. Another repository-local implementation skill is not added unless a measured enforcement gap receives a separate approved specification; product behavior and release gates never depend on local agent folders.

## Open Questions

These are explicitly owned decisions, not permission to weaken the normative gates. Phase 0 closes only the choices required to start implementation; parser-pack and optional-semantic selections remain blocked until their owning implementation/evaluation phases:

1. Which broad parser-pack host wins measured safety/performance/package trade-offs after representative candidates exist: tree-sitter WASM, vetted generated native artifacts, or a split by language tier?
2. Which checked-in corpus manifests and hardware runner classes form the permanent small/medium/large and kernel-scale benchmark suite?
3. Does FTS5 trigram support every target platform/build, and what exact query classes require the current fallback?
4. Which canonical Rust metadata/parser crates are mature enough for each semantic provider before ProjectAtlas-specific logic begins?
5. Which ANN backend and local model, if any, provides enough labeled agent value in Section 10 and the final campaign to justify shipping an optional pack?
6. Should deterministic architecture communities use a maintained graph crate/algorithm or remain out of the first architecture release?
7. What measured scale evidence would justify a separate future specification for a derived federation cache without changing this program's call-only contract?
8. Which phases map to separate releases/milestones after eligible Section 11 measurements establish realistic sequencing?

## Final Architecture Acceptance Criteria

The program is not complete until an independent final review verifies all of the following:

1. **Clear crate graph:** `projectatlas-core` owns shared domain contracts; `db`, `fs`, and `symbols` remain focused infrastructure/extraction boundaries; `service` owns use-case queries; `cli` composes adapters. The dependency graph is acyclic; broad grammar/WASM and semantic ANN/model packs remain separate outer processes and are neither linked nor initialized by core; no core crate depends on CLI/MCP/UI/install concerns.
2. **One durable owner per contract:** language capabilities, stable identities, relation kinds, resolution states, coverage, slot/epoch publication, query types, migration-ledger entries, and serialized MCP/TOON values each have one smallest correct owning module. Active files, modules, crates, types and traits, methods/functions, constants/statics, durable variables, commands, serialized contracts and schemas, fixtures, and tests name that concrete responsibility rather than a phase, codename, migration order, temporary or predecessor identity, or catch-all such as `common`, `admin`, `manager`, `helper`, `utils`, `phaseN`, or `scaffold`; only documented external protocol/algorithm, frozen compatibility, versioned release/evidence, or genuine domain-operation/lifecycle exceptions remain. This is a responsibility audit, not a ban on short local bindings. `atlas_settings` is the existing public owner for graph schema/capability reporting. No parallel string/schema implementation exists.
3. **Rust-native data model:** domain state uses documented structs, enums, newtypes, `Result`, validated constructors, iterators, slices, and ownership/borrowing rather than generic JSON bags, sentinel strings, inheritance emulation, or mutable global state.
4. **Purposeful traits/generics:** open provider or adapter variation may use a narrow trait; closed state/kinds use enums; one injected operation prefers a closure; generics provide demonstrated compile-time reuse or measured static-dispatch value. Single-implementation speculative traits/factories and unconstrained type complexity are removed. Dynamic dispatch is kept outside measured hot loops unless a runtime-extension or ownership requirement wins and its cost passes benchmarks.
5. **GoF intent adapted to Rust:** Command is expressed as typed query/task structs, State as enums or narrowly justified typestate, Strategy as closures/generics/traits selected by openness, Adapter as wrappers at parser/model/pack boundaries, and Builder only for genuinely multi-field validated configuration. The existing store facade remains a ProjectAtlas architecture boundary rather than a claim that Repository is a GoF pattern. Singleton, service locator, inheritance-heavy Template Method/Visitor, and pattern ceremony are rejected.
6. **Fast by layout and work avoided:** compact typed arenas/vectors, interned repeated strings/paths, numeric internal IDs, bounded worker batches, indexed adjacency, batched staged writes, and dependency-driven incremental deltas meet the hard default-core size/startup/RSS budgets and separate warm service/MCP latency gates without losing facts.
7. **Stable modern Rust:** the workspace uses supported stable Rust, Edition 2024/resolver 3 or a reviewed stable successor, canonical maintained crates, explicit minimal features, a locked dependency graph, and no nightly-only architecture requirement.
8. **Strict safety and diagnostics:** every ProjectAtlas-owned crate keeps `unsafe_code = "forbid"` without an exception path; typed errors and actionable context replace panic/unwrap/expect; cancellation, limits, integrity, partial coverage, and recovery are explicit; secrets and repository code never leave/execute implicitly. Safe wrapper crates are preferred for native platform boundaries, and transitive unsafe/native/FFI/dynamic-library/advisory plus containment evidence is reported rather than hidden behind a Rust safety claim.
9. **No warnings or broad suppressions:** `cargo fmt`, locked all-target/all-feature `cargo check`, strict Clippy `-D warnings`, all tests/doctests, and rustdoc `-D warnings` pass. Existing workspace deny lints are not weakened and new broad `allow` attributes are rejected unless narrowly justified and reviewed.
10. **Tests trace every task by risk:** each verification-matrix row carries an L1-L4 risk class and every applicable unit/property, integration, CLI/MCP E2E, packaged/platform, negative/fuzz, and benchmark layer. Separately gated `cargo nextest`, `cargo test --doc`, `cargo llvm-cov`, and `cargo mutants` evidence must pass the mapped Rust test-quality change, including its exclusion, timeout, and coverage/mutation ratchet policies. Justified inapplicable layers are reviewed; no placeholder, flaky, skipped, or timeout-only evidence counts. Phase exit also proves that every inaccurate initiative/provisional identity for completed behavior was removed or replaced by its final behavioral test or validator, not cosmetically renamed; genuine bootstrap operations and typed partial-result/lifecycle states remain valid.
11. **All existing agent surfaces remain compatible:** the generated manifest covers every CLI command, MCP tool, alias, format, default, root rule, exit/error behavior, task contract, and recorded workflow. Normal workflows require no extra calls, default responses remain bounded/TOON-first, only three analysis tools are added, and optional complexity remains configured once and automatic behind existing calls.
12. **Two-slot integrity:** full scans publish only through a separate staging database into the inactive slot and one atomic slot/epoch flip; incremental deltas update the active slot transactionally; normal queries use `active_slot` only; rollback never implies more than the retained second slot or a verified backup.
13. **Retrieval and federation fail honestly:** lexical retrieval is always available, FTS is conditional acceleration with differential equivalence, vector determinism uses declared tolerances, cursors bind all result-defining state, and federation is explicit-root, call-only, read-only/query-only, non-persistent, and fail-closed for the whole call.
14. **Measured superiority without hidden cost:** accepted-capability correctness, full/incremental equivalence, hard budgets, statistically defined speed/resource/query/agent-quality gates, and the phase scoreboard expose code/dependency/package/public-surface cost. Features that fail that value test are removed, deferred, or remain optional with an honest lifecycle state.
15. **Clean installation and agent use:** on clean supported hosts, one official plugin-store installation action provisions and verifies the matching runtime, ProjectAtlas skill, and MCP registration without manual downloads, PATH edits, MCP JSON, version pins, or database-path wiring. Update, rollback, repair, and remove use one typed managed-artifact journal with compensating rollback/last-known-good recovery and preserve project-local indexes/authored metadata; `atlas_init` followed by ordinary atlas-first calls is proven by real packaged E2E.
16. **Objectively cleaner implementation:** a generated architecture scorecard reports production and generated code separately, crate/dependency growth, dependency cycles, ProjectAtlas-owned unsafe lines, transitive native/unsafe/FFI boundaries, duplicated schema/protocol owners, custom storage/query infrastructure, public tool/config/install steps, warnings, and unresolved review findings. The program may claim a cleaner and more modern architecture only when every hard Rust/ownership/safety gate passes, no metric is hidden, every increase has a capability/value justification, and independent Rust, storage, security, performance, KISS, and agent-workflow reviewers resolve all blocking findings.
